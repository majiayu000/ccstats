#![allow(clippy::module_name_repetitions)]

use std::cmp::Ordering;
use std::collections::HashMap;
use std::str::FromStr;

use chrono::{Datelike, Days, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::core::{DateFilter, DayStats, Stats};
use crate::pricing::{CurrencyConverter, PricingDb, calculate_cost, sum_model_costs};
use crate::source::{get_source, load_daily};
use crate::utils::Timezone;

/// Supported local usage sources.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageSource {
    /// Claude Code logs under `~/.claude/projects`.
    Claude,
    /// `OpenAI` Codex logs under `~/.codex/sessions`, or `CODEX_HOME`.
    Codex,
    /// Cursor composer usage data.
    Cursor,
}

impl UsageSource {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            UsageSource::Claude => "claude",
            UsageSource::Codex => "codex",
            UsageSource::Cursor => "cursor",
        }
    }
}

impl FromStr for UsageSource {
    type Err = SdkError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "claude" | "cc" => Ok(UsageSource::Claude),
            "codex" | "cx" => Ok(UsageSource::Codex),
            "cursor" | "cur" => Ok(UsageSource::Cursor),
            source => Err(SdkError::InvalidSource {
                name: source.to_string(),
            }),
        }
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
    fn resolve(
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
    /// Optional display currency. Falls back to USD if rates are unavailable.
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
    pub tokens: TokenBreakdown,
    pub models: Vec<ModelCostSummary>,
    pub valid_entries: i64,
    pub skipped_entries: i64,
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
    let pricing_db = PricingDb::load_quiet(options.offline, options.strict_pricing);
    let currency = options
        .currency
        .as_deref()
        .and_then(|code| CurrencyConverter::load(code, options.offline));
    let currency_code = currency.as_ref().map_or_else(
        || "USD".to_string(),
        |conv| conv.currency_code().to_string(),
    );

    let result = load_daily(source, &filter, timezone, true, false);
    let (stats, models) = merge_days(&result.day_stats);
    let cost_usd = finite_cost(sum_model_costs(&models, &pricing_db));
    let model_summaries = summarize_models(&models, &pricing_db, currency.as_ref());

    Ok(CostSummary {
        source: options.source,
        source_name: source.name().to_string(),
        display_name: source.display_name().to_string(),
        range: options.range,
        since,
        until,
        currency: currency_code,
        cost: convert_cost(cost_usd, currency.as_ref()),
        cost_usd,
        tokens: TokenBreakdown::from_stats(&stats),
        models: model_summaries,
        valid_entries: result.valid,
        skipped_entries: result.skipped,
        elapsed_ms: result.elapsed_ms,
    })
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

fn convert_cost(cost_usd: Option<f64>, currency: Option<&CurrencyConverter>) -> Option<f64> {
    match (cost_usd, currency) {
        (Some(cost), Some(converter)) => Some(converter.convert(cost)),
        (Some(cost), None) => Some(cost),
        (None, _) => None,
    }
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
            ModelCostSummary {
                model: model.clone(),
                cost: convert_cost(cost_usd, currency),
                cost_usd,
                tokens: TokenBreakdown::from_stats(stats),
            }
        })
        .collect();

    rows.sort_by(|a, b| match (b.cost_usd, a.cost_usd) {
        (Some(left), Some(right)) => left.partial_cmp(&right).unwrap_or(Ordering::Equal),
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

    #[test]
    fn usage_source_accepts_names_and_aliases() {
        assert_eq!(
            "claude".parse::<UsageSource>().unwrap(),
            UsageSource::Claude
        );
        assert_eq!("cc".parse::<UsageSource>().unwrap(), UsageSource::Claude);
        assert_eq!("codex".parse::<UsageSource>().unwrap(), UsageSource::Codex);
        assert_eq!("cx".parse::<UsageSource>().unwrap(), UsageSource::Codex);
        assert!("unknown".parse::<UsageSource>().is_err());
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
}
