#![allow(clippy::module_name_repetitions)]

mod batch;
mod codex_weekly;

use std::cmp::Ordering;
use std::collections::HashMap;
use std::str::FromStr;

use chrono::{DateTime, Datelike, Days, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::config::Config;
use crate::core::{DateFilter, DayStats, LoadResult, Stats};
use crate::pricing::{
    CurrencyConverter, PricingDb, calculate_cost, calculate_estimated_proxy_cost, model_cost_kind,
    pricing_source_for_model_stats, pricing_source_for_models, sum_estimated_proxy_model_costs,
    sum_model_costs,
};
use crate::source::{CostCoverage, Source, get_source, load_daily};
use crate::utils::Timezone;

pub use crate::source::{CodexQuotaError, CodexQuotaStatus, CodexWeeklyQuota, UsageSource};

pub use batch::{
    MultiCostSummary, MultiSummaryOptions, summarize_cost_ranges,
    summarize_cost_ranges_with_cli_config,
};
pub(crate) use codex_weekly::estimate_codex_weekly_value_with_pricing;
pub use codex_weekly::{
    CodexWeeklyValueError, CodexWeeklyValueEstimate, CodexWeeklyValueWindow,
    CodexWeeklyValueWindowError, estimate_codex_weekly_value,
    estimate_codex_weekly_value_for_window, load_codex_weekly_quota,
};

impl FromStr for UsageSource {
    type Err = SdkError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let source_name = value.trim().to_ascii_lowercase();
        let Some(source) = get_source(&source_name) else {
            return Err(SdkError::InvalidSource { name: source_name });
        };

        Self::from_name(source.name()).ok_or_else(|| SdkError::InvalidSource {
            name: source.name().to_string(),
        })
    }
}

/// Date or exact UTC timestamp range to summarize.
///
/// [`Self::ThisWeek`] and [`Self::ThisMonth`] are **current-period date
/// filters** in the selected timezone. They are not the CLI `weekly` /
/// `monthly` commands, which only change aggregation grain over already-filtered
/// history (bound those with `--since` / `--until`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageRange {
    /// Current local day in the selected timezone.
    #[default]
    Today,
    /// Current week in the selected timezone: Monday through today.
    ///
    /// This is a date filter, not the CLI `weekly` command. `ccstats weekly`
    /// groups already-filtered history by week; bound dates there with
    /// `--since` / `--until`.
    ThisWeek,
    /// Current month in the selected timezone: the 1st through today.
    ///
    /// This is a date filter, not the CLI `monthly` command. `ccstats monthly`
    /// groups already-filtered history by month; bound dates there with
    /// `--since` / `--until`.
    ThisMonth,
    /// Explicit inclusive date range. `None` means unbounded on that side.
    DateRange {
        since: Option<NaiveDate>,
        until: Option<NaiveDate>,
    },
    /// Explicit inclusive UTC timestamp range.
    TimestampRange {
        since: DateTime<Utc>,
        until: DateTime<Utc>,
    },
}

/// Absolute UTC boundaries for a rolling usage range in the configured timezone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CurrentUsageWindow {
    pub range: UsageRange,
    pub since: NaiveDate,
    pub until: NaiveDate,
    pub since_utc_ms: i64,
    pub until_exclusive_utc_ms: i64,
}

impl UsageRange {
    pub(crate) fn timestamp_bounds(&self) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
        match self {
            UsageRange::TimestampRange { since, until } => Some((*since, *until)),
            _ => None,
        }
    }

    pub(crate) fn resolve(
        &self,
        today: NaiveDate,
    ) -> Result<(Option<NaiveDate>, Option<NaiveDate>), SdkError> {
        let range = match self {
            UsageRange::Today => (Some(today), Some(today)),
            UsageRange::ThisWeek => {
                let start = today
                    .checked_sub_days(Days::new(u64::from(today.weekday().num_days_from_monday())))
                    .unwrap_or(today);
                (Some(start), Some(today))
            }
            UsageRange::ThisMonth => {
                let start = today.with_day(1).unwrap_or(today);
                (Some(start), Some(today))
            }
            UsageRange::DateRange { since, until } => (*since, *until),
            UsageRange::TimestampRange { since, until } => {
                if since > until {
                    return Err(SdkError::InvalidTimestampRange {
                        since: *since,
                        until: *until,
                    });
                }
                (Some(since.date_naive()), Some(until.date_naive()))
            }
        };

        if let (Some(since), Some(until)) = range
            && since > until
        {
            return Err(SdkError::InvalidDateRange { since, until });
        }

        Ok(range)
    }
}

/// Options for [`summarize_cost`].
///
/// Use [`summarize_cost_with_cli_config`] when SDK output should follow the
/// same persisted defaults as the CLI for timezone, pricing, and currency.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SummaryOptions {
    /// Usage source to read.
    pub source: UsageSource,
    /// Date or exact UTC timestamp range to summarize.
    pub range: UsageRange,
    /// Optional timezone name, such as `UTC` or `Asia/Shanghai`.
    pub timezone: Option<String>,
    /// Use cached pricing only.
    pub offline: bool,
    /// Return unknown model costs as `None` instead of using fallback pricing.
    pub strict_pricing: bool,
    /// Optional display currency. Returns an error if rates are unavailable.
    pub currency: Option<String>,
}

impl Default for SummaryOptions {
    fn default() -> Self {
        Self {
            source: UsageSource::Claude,
            range: UsageRange::Today,
            timezone: None,
            offline: false,
            strict_pricing: false,
            currency: None,
        }
    }
}

/// Token totals for a summary or model row.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TokenBreakdown {
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub reasoning_tokens: i64,
    pub cache_creation_tokens: i64,
    /// Portion of `cache_creation_tokens` written with a 1-hour TTL.
    /// This is a subset of `cache_creation_tokens`, not an extra additive bucket.
    // Default keeps summaries serialized before this field existed loadable.
    #[serde(default)]
    pub cache_creation_1h_tokens: i64,
    pub cache_read_tokens: i64,
    /// Difference between the source-authoritative total and the named buckets.
    pub reported_total_adjustment: i64,
    /// Reported prompt-cache hit rate as a percentage from 0 to 100.
    /// `None` means the source does not expose trustworthy cache-read data or
    /// the summary has no input-side tokens.
    // Default keeps summaries serialized before this field existed loadable.
    #[serde(default)]
    pub cache_hit_rate: Option<f64>,
    pub total_tokens: i64,
}

/// Per-model usage and cost summary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelCostSummary {
    pub model: String,
    pub cost: Option<f64>,
    pub cost_usd: Option<f64>,
    pub estimated_cost: Option<f64>,
    pub estimated_cost_usd: Option<f64>,
    pub cost_kind: String,
    pub pricing_source: String,
    pub tokens: TokenBreakdown,
}

/// Coverage of API-equivalent pricing for sources with partial request telemetry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApiEquivalentCostCoverage {
    pub total_tokens: i64,
    pub priced_tokens: i64,
    pub percent: f64,
    pub complete: bool,
    pub cost_is_lower_bound: bool,
}

impl From<CostCoverage> for ApiEquivalentCostCoverage {
    fn from(coverage: CostCoverage) -> Self {
        Self {
            total_tokens: coverage.total_tokens,
            priced_tokens: coverage.priced_tokens,
            percent: coverage.percent(),
            complete: !coverage.is_partial(),
            cost_is_lower_bound: coverage.cost_is_lower_bound(),
        }
    }
}

/// Structured usage and cost summary for SDK consumers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CostSummary {
    pub source: UsageSource,
    pub source_name: String,
    pub display_name: String,
    pub range: UsageRange,
    pub since: Option<NaiveDate>,
    pub until: Option<NaiveDate>,
    pub currency: String,
    pub cost: Option<f64>,
    pub cost_usd: Option<f64>,
    pub estimated_cost: Option<f64>,
    pub estimated_cost_usd: Option<f64>,
    pub cost_kind: String,
    pub pricing_source: String,
    pub api_equivalent_cost_coverage: Option<ApiEquivalentCostCoverage>,
    pub tokens: TokenBreakdown,
    pub models: Vec<ModelCostSummary>,
    pub valid_entries: i64,
    pub skipped_entries: i64,
    pub parse_error_entries: usize,
    pub elapsed_ms: f64,
}

/// Errors returned by the public SDK API.
#[derive(Debug, Error)]
pub enum SdkError {
    #[error("invalid usage source: {name}")]
    InvalidSource { name: String },

    #[error("invalid date range: since {since} is after until {until}")]
    InvalidDateRange { since: NaiveDate, until: NaiveDate },

    #[error("invalid timestamp range: since {since} is after until {until}")]
    InvalidTimestampRange {
        since: DateTime<Utc>,
        until: DateTime<Utc>,
    },

    #[error("{0}")]
    Configuration(String),
}

/// Summarize local token usage and estimated cost.
///
/// # Errors
///
/// Returns an error when the source or timezone is invalid, or when an explicit
/// date or timestamp range has `since` after `until`.
pub fn summarize_cost(options: SummaryOptions) -> Result<CostSummary, SdkError> {
    let timezone = Timezone::parse(options.timezone.as_deref())
        .map_err(|err| SdkError::Configuration(err.to_string()))?;
    let today = timezone.to_fixed_offset(Utc::now()).date_naive();
    let (since, until) = options.range.resolve(today)?;
    let mut filter = DateFilter::new(since, until);
    if let Some((since_timestamp, until_timestamp)) = options.range.timestamp_bounds() {
        filter = filter.with_exact_timestamp_range(since_timestamp, until_timestamp);
    }

    let source = get_source(options.source.as_str()).ok_or_else(|| SdkError::InvalidSource {
        name: options.source.as_str().to_string(),
    })?;
    let pricing_db = PricingDb::try_load_quiet(options.offline, options.strict_pricing)
        .map_err(|err| SdkError::Configuration(err.to_string()))?;
    let currency = load_requested_currency(options.currency.as_deref(), options.offline)?;
    let currency_code = currency.as_ref().map_or_else(
        || "USD".to_string(),
        |conv| conv.currency_code().to_string(),
    );

    let result = load_daily(source, &filter, timezone, true, false);
    let cost_coverage = CostCoverage::from_stats(result.day_stats.values().map(|day| &day.stats));
    Ok(build_cost_summary(
        options.source,
        source,
        options.range,
        since,
        until,
        &result,
        &pricing_db,
        currency.as_ref(),
        &currency_code,
        cost_coverage,
    ))
}

/// Summarize local token usage using the same reusable config defaults as the CLI.
///
/// This preserves the explicit SDK source and range, then fills unset timezone
/// and currency from config and applies config-enabled pricing flags. That makes
/// calls like `ccstats codex today` and SDK `Codex + Today` use the same date
/// boundary and pricing mode by default.
///
/// # Errors
///
/// Returns an error when the resolved source or timezone is invalid, or when an
/// explicit date or timestamp range has `since` after `until`.
pub fn summarize_cost_with_cli_config(options: SummaryOptions) -> Result<CostSummary, SdkError> {
    let config = load_cli_config()?;
    summarize_cost(apply_cli_config(options, &config))
}

/// Resolves today's date using the persisted CLI timezone.
///
/// # Errors
///
/// Returns an error when CLI configuration cannot be loaded or its timezone is invalid.
pub fn current_usage_date_with_cli_config() -> Result<NaiveDate, SdkError> {
    let config = load_cli_config()?;
    let timezone = Timezone::parse(config.timezone.as_deref())
        .map_err(|error| SdkError::Configuration(error.to_string()))?;
    Ok(timezone.to_fixed_offset(Utc::now()).date_naive())
}

fn current_usage_windows(
    timezone: Timezone,
    now: DateTime<Utc>,
) -> Result<Vec<CurrentUsageWindow>, SdkError> {
    let today = timezone.to_fixed_offset(now).date_naive();
    [
        UsageRange::Today,
        UsageRange::ThisWeek,
        UsageRange::ThisMonth,
    ]
    .into_iter()
    .map(|range| {
        let (Some(since), Some(until)) = range.resolve(today)? else {
            return Err(SdkError::Configuration(
                "rolling usage range did not resolve to bounded dates".to_string(),
            ));
        };
        let until_exclusive = until.checked_add_days(Days::new(1)).ok_or_else(|| {
            SdkError::Configuration("usage window end is not representable".to_string())
        })?;
        let since_utc_ms = timezone.date_start_utc_millis(since).ok_or_else(|| {
            SdkError::Configuration("usage window start is not representable".to_string())
        })?;
        let until_exclusive_utc_ms =
            timezone
                .date_start_utc_millis(until_exclusive)
                .ok_or_else(|| {
                    SdkError::Configuration("usage window end is not representable".to_string())
                })?;
        Ok(CurrentUsageWindow {
            range,
            since,
            until,
            since_utc_ms,
            until_exclusive_utc_ms,
        })
    })
    .collect()
}

/// Resolves the current day, week, and month with absolute UTC boundaries using
/// the persisted CLI timezone.
///
/// # Errors
///
/// Returns an error when CLI configuration or its timezone is invalid, or a
/// date boundary cannot be represented.
pub fn current_usage_windows_with_cli_config() -> Result<Vec<CurrentUsageWindow>, SdkError> {
    let config = load_cli_config()?;
    let timezone = Timezone::parse(config.timezone.as_deref())
        .map_err(|error| SdkError::Configuration(error.to_string()))?;
    current_usage_windows(timezone, Utc::now())
}

pub(super) fn load_cli_config() -> Result<Config, SdkError> {
    Config::try_load_quiet().map_err(|err| SdkError::Configuration(err.to_string()))
}

fn apply_cli_config(mut options: SummaryOptions, config: &Config) -> SummaryOptions {
    if !options.offline && config.offline {
        options.offline = true;
    }
    if !options.strict_pricing && config.strict_pricing {
        options.strict_pricing = true;
    }
    if options.timezone.is_none() {
        options.timezone.clone_from(&config.timezone);
    }
    if options.currency.is_none() {
        options.currency.clone_from(&config.currency);
    }

    options
}

impl TokenBreakdown {
    fn from_stats(stats: &Stats, supports_cache_read: bool) -> Self {
        Self {
            input_tokens: stats.input_tokens,
            output_tokens: stats.output_tokens,
            reasoning_tokens: stats.reasoning_tokens,
            cache_creation_tokens: stats.cache_creation,
            cache_creation_1h_tokens: stats.cache_creation_1h,
            cache_read_tokens: stats.cache_read,
            reported_total_adjustment: stats.reported_total_adjustment,
            cache_hit_rate: stats.cache_hit_rate(supports_cache_read),
            total_tokens: stats.total_tokens(),
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(in crate::sdk) fn build_cost_summary(
    usage_source: UsageSource,
    source: &dyn Source,
    range: UsageRange,
    since: Option<NaiveDate>,
    until: Option<NaiveDate>,
    result: &LoadResult,
    pricing_db: &PricingDb,
    currency: Option<&CurrencyConverter>,
    currency_code: &str,
    cost_coverage: Option<CostCoverage>,
) -> CostSummary {
    let (stats, models) = merge_days(&result.day_stats);
    let cost_usd = finite_cost(sum_model_costs(&models, pricing_db));
    let estimated_cost_usd =
        finite_positive_cost(sum_estimated_proxy_model_costs(&models, pricing_db));
    let supports_cache_read = source.capabilities().has_cache_read;

    CostSummary {
        source: usage_source,
        source_name: source.name().to_string(),
        display_name: source.display_name().to_string(),
        range,
        since,
        until,
        currency: currency_code.to_string(),
        cost: convert_cost(cost_usd, currency),
        cost_usd,
        estimated_cost: convert_cost(estimated_cost_usd, currency),
        estimated_cost_usd,
        cost_kind: model_cost_kind(&models).as_str().to_string(),
        pricing_source: pricing_source_for_models(&models, pricing_db)
            .as_str()
            .to_string(),
        api_equivalent_cost_coverage: cost_coverage.map(Into::into),
        tokens: TokenBreakdown::from_stats(&stats, supports_cache_read),
        models: summarize_models(&models, pricing_db, currency, supports_cache_read),
        valid_entries: result.valid,
        skipped_entries: result.skipped,
        parse_error_entries: result.parse_errors,
        elapsed_ms: result.elapsed_ms,
    }
}

fn merge_days(day_stats: &HashMap<String, DayStats>) -> (Stats, HashMap<String, Stats>) {
    let mut stats = Stats::default();
    let mut models = HashMap::new();

    for day in day_stats.values() {
        stats.add(&day.stats);
        for (model, model_stats) in &day.models {
            models
                .entry(model.clone())
                .or_insert_with(Stats::default)
                .add(model_stats);
        }
    }

    (stats, models)
}

fn finite_cost(cost: f64) -> Option<f64> {
    cost.is_finite().then_some(cost)
}

fn finite_positive_cost(cost: f64) -> Option<f64> {
    (cost.is_finite() && cost > 0.0).then_some(cost)
}

fn convert_cost(cost_usd: Option<f64>, currency: Option<&CurrencyConverter>) -> Option<f64> {
    match (cost_usd, currency) {
        (Some(cost), Some(converter)) => Some(converter.convert(cost)),
        (Some(cost), None) => Some(cost),
        (None, _) => None,
    }
}

pub(in crate::sdk) fn load_requested_currency(
    currency: Option<&str>,
    offline: bool,
) -> Result<Option<CurrencyConverter>, SdkError> {
    let Some(code) = currency else {
        return Ok(None);
    };
    CurrencyConverter::load(code, offline).map(Some).ok_or_else(|| {
        SdkError::Configuration(format!(
            "failed to load exchange rate for '{code}'; use a supported currency with cached rates, refresh rates online, or omit currency"
        ))
    })
}

fn summarize_models(
    models: &HashMap<String, Stats>,
    pricing_db: &PricingDb,
    currency: Option<&CurrencyConverter>,
    supports_cache_read: bool,
) -> Vec<ModelCostSummary> {
    let mut rows: Vec<_> = models
        .iter()
        .map(|(model, stats)| {
            let cost_usd = finite_cost(calculate_cost(stats, model, pricing_db));
            let estimated_cost_usd =
                finite_positive_cost(calculate_estimated_proxy_cost(stats, model, pricing_db));
            ModelCostSummary {
                model: model.clone(),
                cost: convert_cost(cost_usd, currency),
                cost_usd,
                estimated_cost: convert_cost(estimated_cost_usd, currency),
                estimated_cost_usd,
                cost_kind: stats.cost_kind().as_str().to_string(),
                pricing_source: pricing_source_for_model_stats(model, stats, pricing_db)
                    .as_str()
                    .to_string(),
                tokens: TokenBreakdown::from_stats(stats, supports_cache_read),
            }
        })
        .collect();

    rows.sort_by(|a, b| match (b.cost_usd, a.cost_usd) {
        (Some(left), Some(right)) => left
            .partial_cmp(&right)
            .unwrap_or(Ordering::Equal)
            .then_with(|| a.model.cmp(&b.model)),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => b
            .tokens
            .total_tokens
            .cmp(&a.tokens.total_tokens)
            .then_with(|| a.model.cmp(&b.model)),
    });
    rows
}

#[cfg(test)]
#[path = "sdk/tests.rs"]
mod tests;
