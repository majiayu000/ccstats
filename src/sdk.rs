#![allow(clippy::module_name_repetitions)]

mod batch;

use std::cmp::Ordering;
use std::collections::HashMap;
use std::str::FromStr;

use chrono::{Datelike, Days, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::config::Config;
use crate::core::{DateFilter, DayStats, LoadResult, Stats};
use crate::pricing::{
    CurrencyConverter, PricingDb, calculate_cost, calculate_estimated_proxy_cost, model_cost_kind,
    sum_estimated_proxy_model_costs, sum_model_costs,
};
use crate::source::{Source, get_source, load_daily};
use crate::utils::Timezone;

pub use batch::{
    MultiCostSummary, MultiSummaryOptions, summarize_cost_ranges,
    summarize_cost_ranges_with_cli_config,
};

/// Supported local usage sources.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum UsageSource {
    /// Claude Code logs under `~/.claude/projects`.
    Claude,
    /// `OpenAI` Codex logs under `~/.codex/sessions`, or `CODEX_HOME`.
    Codex,
    /// Cursor composer usage data.
    Cursor,
    /// Grok session signal summaries under `~/.grok/sessions`, or `GROK_HOME`.
    Grok,
}

impl UsageSource {
    #[cfg(test)]
    pub(crate) const VARIANTS: [Self; 4] = [
        UsageSource::Claude,
        UsageSource::Codex,
        UsageSource::Cursor,
        UsageSource::Grok,
    ];

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            UsageSource::Claude => "claude",
            UsageSource::Codex => "codex",
            UsageSource::Cursor => "cursor",
            UsageSource::Grok => "grok",
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        match name {
            "claude" => Some(UsageSource::Claude),
            "codex" => Some(UsageSource::Codex),
            "cursor" => Some(UsageSource::Cursor),
            "grok" => Some(UsageSource::Grok),
            _ => None,
        }
    }
}

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

/// Date range to summarize.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageRange {
    /// Current local day in the selected timezone.
    #[default]
    Today,
    /// Monday through today in the selected timezone.
    ThisWeek,
    /// First day of the current month through today in the selected timezone.
    ThisMonth,
    /// Explicit inclusive date range. `None` means unbounded on that side.
    DateRange {
        since: Option<NaiveDate>,
        until: Option<NaiveDate>,
    },
}

impl UsageRange {
    pub(in crate::sdk) fn resolve(
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
    /// Date range to summarize.
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
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenBreakdown {
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub reasoning_tokens: i64,
    pub cache_creation_tokens: i64,
    pub cache_read_tokens: i64,
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
    pub tokens: TokenBreakdown,
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

    #[error("{0}")]
    Configuration(String),
}

/// Summarize local token usage and estimated cost.
///
/// # Errors
///
/// Returns an error when the source or timezone is invalid, or when an explicit
/// date range has `since` after `until`.
pub fn summarize_cost(options: SummaryOptions) -> Result<CostSummary, SdkError> {
    let timezone = Timezone::parse(options.timezone.as_deref())
        .map_err(|err| SdkError::Configuration(err.to_string()))?;
    let today = timezone.to_fixed_offset(Utc::now()).date_naive();
    let (since, until) = options.range.resolve(today)?;
    let filter = DateFilter::new(since, until);

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
/// explicit date range has `since` after `until`.
pub fn summarize_cost_with_cli_config(options: SummaryOptions) -> Result<CostSummary, SdkError> {
    let config = load_cli_config()?;
    summarize_cost(apply_cli_config(options, &config))
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
    fn from_stats(stats: &Stats) -> Self {
        Self {
            input_tokens: stats.input_tokens,
            output_tokens: stats.output_tokens,
            reasoning_tokens: stats.reasoning_tokens,
            cache_creation_tokens: stats.cache_creation,
            cache_read_tokens: stats.cache_read,
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
) -> CostSummary {
    let (stats, models) = merge_days(&result.day_stats);
    let cost_usd = finite_cost(sum_model_costs(&models, pricing_db));
    let estimated_cost_usd =
        finite_positive_cost(sum_estimated_proxy_model_costs(&models, pricing_db));

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
        tokens: TokenBreakdown::from_stats(&stats),
        models: summarize_models(&models, pricing_db, currency),
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
                tokens: TokenBreakdown::from_stats(stats),
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
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn usage_source_accepts_registry_names_and_aliases() {
        for source in crate::source::all_sources() {
            let expected = UsageSource::from_name(source.name()).expect("SDK source variant");
            assert_eq!(source.name().parse::<UsageSource>().unwrap(), expected);
            assert_eq!(
                source
                    .name()
                    .to_ascii_uppercase()
                    .parse::<UsageSource>()
                    .unwrap(),
                expected
            );
            assert_eq!(
                format!(" {} ", source.name())
                    .parse::<UsageSource>()
                    .unwrap(),
                expected
            );

            for alias in source.aliases() {
                assert_eq!(alias.parse::<UsageSource>().unwrap(), expected);
            }
        }

        let err = " unknown ".parse::<UsageSource>().unwrap_err();
        assert!(matches!(err, SdkError::InvalidSource { name } if name == "unknown"));
    }

    #[test]
    fn registry_concrete_sources_match_sdk_usage_sources() {
        let registry_sources = crate::source::all_sources()
            .map(Source::name)
            .collect::<BTreeSet<_>>();
        let sdk_sources = UsageSource::VARIANTS
            .iter()
            .map(|source| source.as_str())
            .collect::<BTreeSet<_>>();

        assert_eq!(registry_sources, sdk_sources);
        assert!(crate::source::source_choices().contains(&crate::source::ALL_SOURCES));
        assert!(crate::source::ALL_SOURCES.parse::<UsageSource>().is_err());

        for source in crate::source::all_sources() {
            assert_eq!(
                source.name().parse::<UsageSource>().unwrap().as_str(),
                source.name()
            );
            for alias in source.aliases() {
                assert_eq!(
                    alias.parse::<UsageSource>().unwrap().as_str(),
                    source.name()
                );
            }
        }
    }

    #[test]
    fn usage_range_this_week_starts_on_monday() {
        let today = NaiveDate::from_ymd_opt(2026, 5, 9).unwrap();
        let (since, until) = UsageRange::ThisWeek.resolve(today).unwrap();
        assert_eq!(since, Some(NaiveDate::from_ymd_opt(2026, 5, 4).unwrap()));
        assert_eq!(until, Some(today));
    }

    #[test]
    fn usage_range_rejects_reversed_dates() {
        let range = UsageRange::DateRange {
            since: Some(NaiveDate::from_ymd_opt(2026, 5, 10).unwrap()),
            until: Some(NaiveDate::from_ymd_opt(2026, 5, 9).unwrap()),
        };
        assert!(
            range
                .resolve(NaiveDate::from_ymd_opt(2026, 5, 9).unwrap())
                .is_err()
        );
    }

    #[test]
    fn model_summaries_use_model_name_as_equal_cost_tiebreaker() {
        let pricing_db = PricingDb::default();
        let mut models = HashMap::new();
        models.insert(
            "gpt-5-zeta".to_string(),
            Stats {
                input_tokens: 10,
                ..Stats::default()
            },
        );
        models.insert(
            "gpt-5-alpha".to_string(),
            Stats {
                input_tokens: 10,
                ..Stats::default()
            },
        );

        let rows = summarize_models(&models, &pricing_db, None);

        assert_eq!(rows[0].model, "gpt-5-alpha");
        assert_eq!(rows[1].model, "gpt-5-zeta");
        assert_eq!(rows[0].cost_usd, rows[1].cost_usd);
    }

    #[test]
    fn cli_config_fills_sdk_summary_defaults() {
        let config = Config {
            offline: true,
            strict_pricing: true,
            timezone: Some("Asia/Shanghai".to_string()),
            currency: Some("CNY".to_string()),
            ..Config::default()
        };

        let options = apply_cli_config(
            SummaryOptions {
                source: UsageSource::Codex,
                range: UsageRange::Today,
                ..SummaryOptions::default()
            },
            &config,
        );

        assert_eq!(options.source, UsageSource::Codex);
        assert_eq!(options.range, UsageRange::Today);
        assert!(options.offline);
        assert!(options.strict_pricing);
        assert_eq!(options.timezone.as_deref(), Some("Asia/Shanghai"));
        assert_eq!(options.currency.as_deref(), Some("CNY"));
    }

    #[test]
    fn explicit_sdk_summary_options_win_over_cli_config() {
        let config = Config {
            timezone: Some("Asia/Shanghai".to_string()),
            currency: Some("CNY".to_string()),
            ..Config::default()
        };

        let options = apply_cli_config(
            SummaryOptions {
                timezone: Some("UTC".to_string()),
                currency: Some("EUR".to_string()),
                ..SummaryOptions::default()
            },
            &config,
        );

        assert_eq!(options.timezone.as_deref(), Some("UTC"));
        assert_eq!(options.currency.as_deref(), Some("EUR"));
    }

    #[test]
    fn requested_currency_requires_available_rate() {
        let err = load_requested_currency(Some("ZZZ"), true).expect_err("currency should fail");
        assert!(err.to_string().contains("failed to load exchange rate"));
    }
}
