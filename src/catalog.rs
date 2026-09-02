//! Public projections for graphical and SDK consumers.

use std::cmp::Ordering;
use std::collections::HashMap;

use chrono::{NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::config::Config;
use crate::consts::DATE_FORMAT;
use crate::core::{DateFilter, SessionStats, Stats, aggregate_projects};
use crate::pricing::{
    CurrencyConverter, PricingDb, calculate_cost, calculate_estimated_proxy_cost, model_cost_kind,
    pricing_source_for_model_stats, pricing_source_for_models,
};
use crate::sdk::{
    ApiEquivalentCostCoverage, ModelCostSummary, SdkError, SummaryOptions, TokenBreakdown,
    UsageRange, UsageSource,
};
use crate::source::{
    CostCoverage, DiagnosticStatus, all_sources, get_source, load_daily, load_sessions,
};
use crate::utils::Timezone;

/// Stable source metadata for graphical and SDK consumers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "serialized source capability flags are independent"
)]
pub struct SourceDescriptor {
    pub source: UsageSource,
    pub name: String,
    pub display_name: String,
    pub aliases: Vec<String>,
    pub has_projects: bool,
    pub has_reasoning_tokens: bool,
    pub has_cache_creation: bool,
    pub has_cache_read: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceDiagnosticStatus {
    Detected,
    Configured,
    Missing,
}

impl From<DiagnosticStatus> for SourceDiagnosticStatus {
    fn from(status: DiagnosticStatus) -> Self {
        match status {
            DiagnosticStatus::Detected => Self::Detected,
            DiagnosticStatus::Configured => Self::Configured,
            DiagnosticStatus::Missing => Self::Missing,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceDiagnosticDescriptor {
    pub source: UsageSource,
    pub name: String,
    pub display_name: String,
    pub status: SourceDiagnosticStatus,
    pub files: usize,
    pub detail: String,
    pub setup: String,
}

/// List every source currently registered by ccstats.
///
/// # Errors
///
/// Returns [`SdkError::InvalidSource`] when a registry entry has no matching
/// public [`UsageSource`] variant.
pub fn list_usage_sources() -> Result<Vec<SourceDescriptor>, SdkError> {
    all_sources()
        .map(|source| {
            let capabilities = source.capabilities();
            Ok(SourceDescriptor {
                source: source.name().parse::<UsageSource>()?,
                name: source.name().to_string(),
                display_name: source.display_name().to_string(),
                aliases: source
                    .aliases()
                    .iter()
                    .map(|alias| (*alias).to_string())
                    .collect(),
                has_projects: capabilities.has_projects,
                has_reasoning_tokens: capabilities.has_reasoning_tokens,
                has_cache_creation: capabilities.has_cache_creation,
                has_cache_read: capabilities.has_cache_read,
            })
        })
        .collect()
}

/// Diagnose every registered source without parsing usage records.
///
/// This performs the same read-only discovery used by `ccstats doctor` and
/// does not contact remote providers.
///
/// # Errors
///
/// Returns [`SdkError::InvalidSource`] when a registry entry has no matching
/// public [`UsageSource`] variant.
pub fn diagnose_usage_sources() -> Result<Vec<SourceDiagnosticDescriptor>, SdkError> {
    all_sources()
        .map(|source| {
            let diagnostic = source.diagnose();
            Ok(SourceDiagnosticDescriptor {
                source: source.name().parse::<UsageSource>()?,
                name: source.name().to_string(),
                display_name: source.display_name().to_string(),
                status: diagnostic.status.into(),
                files: diagnostic.files,
                detail: diagnostic.detail,
                setup: source.setup_hint().to_string(),
            })
        })
        .collect()
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnalyticsQuality {
    pub valid_entries: i64,
    pub dedup_skipped_entries: i64,
    pub parse_error_entries: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UsageMetrics {
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
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionDrilldown {
    pub session_id: String,
    pub project_path: String,
    pub first_timestamp: String,
    pub last_timestamp: String,
    pub metrics: UsageMetrics,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectDrilldown {
    pub project_path: String,
    pub project_name: String,
    pub session_count: usize,
    pub metrics: UsageMetrics,
    pub sessions: Vec<SessionDrilldown>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectDrilldownSummary {
    pub source: UsageSource,
    pub source_name: String,
    pub display_name: String,
    pub range: UsageRange,
    pub currency: String,
    pub quality: AnalyticsQuality,
    pub projects: Vec<ProjectDrilldown>,
}

#[derive(Debug, Error)]
pub enum DrilldownError {
    #[error("usage source '{source_name}' does not support project drilldown")]
    ProjectsUnsupported {
        usage_source: UsageSource,
        source_name: String,
    },
    #[error(transparent)]
    Sdk(#[from] SdkError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoryCostStatus {
    Known,
    Partial,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DailyUsagePoint {
    pub date: NaiveDate,
    pub currency: String,
    pub tokens: TokenBreakdown,
    pub records: i64,
    pub cost: Option<f64>,
    pub cost_usd: Option<f64>,
    pub cost_status: HistoryCostStatus,
    pub cost_kind: String,
    pub pricing_source: String,
    pub api_equivalent_cost_coverage: Option<ApiEquivalentCostCoverage>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UsageHistory {
    pub source: UsageSource,
    pub source_name: String,
    pub display_name: String,
    pub range: UsageRange,
    pub as_of_date: NaiveDate,
    pub currency: String,
    pub points: Vec<DailyUsagePoint>,
    pub quality: AnalyticsQuality,
}

struct AnalysisContext {
    range: UsageRange,
    as_of_date: NaiveDate,
    filter: DateFilter,
    timezone: Timezone,
    pricing_db: PricingDb,
    currency: Option<CurrencyConverter>,
    currency_code: String,
}

pub(crate) fn apply_cli_config(mut options: SummaryOptions) -> Result<SummaryOptions, SdkError> {
    let config =
        Config::try_load_quiet().map_err(|error| SdkError::Configuration(error.to_string()))?;
    if !options.offline && config.offline {
        options.offline = true;
    }
    if !options.strict_pricing && config.strict_pricing {
        options.strict_pricing = true;
    }
    if options.timezone.is_none() {
        options.timezone = config.timezone;
    }
    if options.currency.is_none() {
        options.currency = config.currency;
    }
    Ok(options)
}

fn analysis_context(options: &SummaryOptions) -> Result<AnalysisContext, SdkError> {
    let timezone = Timezone::parse(options.timezone.as_deref())
        .map_err(|error| SdkError::Configuration(error.to_string()))?;
    let today = timezone.to_fixed_offset(Utc::now()).date_naive();
    let (since, until) = options.range.resolve(today)?;
    let mut filter = DateFilter::new(since, until);
    if let Some((since_timestamp, until_timestamp)) = options.range.timestamp_bounds() {
        filter = filter.with_exact_timestamp_range(since_timestamp, until_timestamp);
    }
    let pricing_db = PricingDb::try_load_quiet(options.offline, options.strict_pricing)
        .map_err(|error| SdkError::Configuration(error.to_string()))?;
    let currency = options
        .currency
        .as_deref()
        .map(|code| {
            CurrencyConverter::load(code, options.offline).ok_or_else(|| {
                SdkError::Configuration(format!("failed to load exchange rate for '{code}'"))
            })
        })
        .transpose()?;
    let currency_code = currency.as_ref().map_or_else(
        || "USD".to_string(),
        |converter| converter.currency_code().to_string(),
    );
    Ok(AnalysisContext {
        range: options.range.clone(),
        as_of_date: today,
        filter,
        timezone,
        pricing_db,
        currency,
        currency_code,
    })
}

fn finite(value: f64) -> Option<f64> {
    value.is_finite().then_some(value)
}

fn converted(value: Option<f64>, currency: Option<&CurrencyConverter>) -> Option<f64> {
    match (value, currency) {
        (Some(value), Some(converter)) => Some(converter.convert(value)),
        (value, None) => value,
        (None, Some(_)) => None,
    }
}

fn token_breakdown(stats: &Stats, supports_cache_read: bool) -> TokenBreakdown {
    TokenBreakdown {
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

fn model_summaries(
    models: &HashMap<String, Stats>,
    context: &AnalysisContext,
    supports_cache_read: bool,
) -> Vec<ModelCostSummary> {
    let mut rows = models
        .iter()
        .map(|(model, stats)| {
            let cost_usd = finite(calculate_cost(stats, model, &context.pricing_db));
            let estimated_cost_usd = finite(calculate_estimated_proxy_cost(
                stats,
                model,
                &context.pricing_db,
            ))
            .filter(|cost| *cost > 0.0);
            ModelCostSummary {
                model: model.clone(),
                cost: converted(cost_usd, context.currency.as_ref()),
                cost_usd,
                estimated_cost: converted(estimated_cost_usd, context.currency.as_ref()),
                estimated_cost_usd,
                cost_kind: stats.cost_kind().as_str().to_string(),
                pricing_source: pricing_source_for_model_stats(model, stats, &context.pricing_db)
                    .as_str()
                    .to_string(),
                tokens: token_breakdown(stats, supports_cache_read),
            }
        })
        .collect::<Vec<_>>();
    rows.sort_by(|left, right| {
        right
            .cost_usd
            .partial_cmp(&left.cost_usd)
            .unwrap_or(Ordering::Equal)
            .then_with(|| right.tokens.total_tokens.cmp(&left.tokens.total_tokens))
            .then_with(|| left.model.cmp(&right.model))
    });
    rows
}

fn usage_metrics(
    stats: &Stats,
    models: &HashMap<String, Stats>,
    context: &AnalysisContext,
    supports_cache_read: bool,
) -> UsageMetrics {
    let model_rows = model_summaries(models, context, supports_cache_read);
    let active_rows = model_rows
        .iter()
        .filter(|model| model.tokens.total_tokens > 0)
        .collect::<Vec<_>>();
    let known_cost = active_rows.iter().all(|model| model.cost_usd.is_some());
    let cost_usd = known_cost.then(|| active_rows.iter().filter_map(|model| model.cost_usd).sum());
    let estimated_cost_usd = finite(crate::pricing::sum_estimated_proxy_model_costs(
        models,
        &context.pricing_db,
    ))
    .filter(|cost| *cost > 0.0);
    UsageMetrics {
        currency: context.currency_code.clone(),
        cost: converted(cost_usd, context.currency.as_ref()),
        cost_usd,
        estimated_cost: converted(estimated_cost_usd, context.currency.as_ref()),
        estimated_cost_usd,
        cost_kind: model_cost_kind(models).as_str().to_string(),
        pricing_source: crate::pricing::pricing_source_for_models(models, &context.pricing_db)
            .as_str()
            .to_string(),
        api_equivalent_cost_coverage: CostCoverage::from_stats(std::iter::once(stats))
            .map(Into::into),
        tokens: token_breakdown(stats, supports_cache_read),
        models: model_rows,
    }
}

/// Summarizes project totals and their contributing sessions for one source.
///
/// # Errors
///
/// Returns an error when the source lacks project metadata or its usage data,
/// pricing, range, or configuration cannot be loaded.
pub fn summarize_project_drilldown(
    options: &SummaryOptions,
) -> Result<ProjectDrilldownSummary, DrilldownError> {
    let source = get_source(options.source.as_str()).ok_or_else(|| SdkError::InvalidSource {
        name: options.source.as_str().to_string(),
    })?;
    if !source.capabilities().has_projects {
        return Err(DrilldownError::ProjectsUnsupported {
            usage_source: options.source,
            source_name: source.name().to_string(),
        });
    }
    let context = analysis_context(options)?;
    let daily = load_daily(source, &context.filter, context.timezone, true, false);
    let sessions = load_sessions(source, &context.filter, context.timezone, true);
    let projects = aggregate_projects(sessions.clone());
    let mut sessions_by_project: HashMap<String, Vec<SessionStats>> = HashMap::new();
    for session in sessions {
        sessions_by_project
            .entry(session.project_path.clone())
            .or_default()
            .push(session);
    }
    let supports_cache_read = source.capabilities().has_cache_read;
    let projects = projects
        .into_iter()
        .map(|project| {
            let mut sessions = sessions_by_project
                .remove(&project.project_path)
                .unwrap_or_default();
            sessions.sort_by(|left, right| {
                right
                    .last_timestamp
                    .cmp(&left.last_timestamp)
                    .then_with(|| left.session_id.cmp(&right.session_id))
            });
            let sessions = sessions
                .into_iter()
                .map(|session| SessionDrilldown {
                    session_id: session.session_id,
                    project_path: session.project_path,
                    first_timestamp: session.first_timestamp,
                    last_timestamp: session.last_timestamp,
                    metrics: usage_metrics(
                        &session.stats,
                        &session.models,
                        &context,
                        supports_cache_read,
                    ),
                })
                .collect();
            ProjectDrilldown {
                project_path: project.project_path,
                project_name: project.project_name,
                session_count: project.session_count,
                metrics: usage_metrics(
                    &project.stats,
                    &project.models,
                    &context,
                    supports_cache_read,
                ),
                sessions,
            }
        })
        .collect();
    Ok(ProjectDrilldownSummary {
        source: options.source,
        source_name: source.name().to_string(),
        display_name: source.display_name().to_string(),
        range: context.range,
        currency: context.currency_code,
        quality: AnalyticsQuality {
            valid_entries: daily.valid,
            dedup_skipped_entries: daily.skipped,
            parse_error_entries: daily.parse_errors,
        },
        projects,
    })
}

/// Summarizes project and session usage using persisted CLI configuration.
///
/// # Errors
///
/// Returns an error when configuration cannot be loaded, the source lacks
/// project metadata, or its usage data cannot be summarized.
pub fn summarize_project_drilldown_with_cli_config(
    options: SummaryOptions,
) -> Result<ProjectDrilldownSummary, DrilldownError> {
    summarize_project_drilldown(&apply_cli_config(options)?)
}

/// Builds chronological daily usage points for one source and range.
///
/// # Errors
///
/// Returns an error when source data, pricing, range, or currency settings
/// cannot be loaded or interpreted.
pub fn usage_history(options: &SummaryOptions) -> Result<UsageHistory, SdkError> {
    let source = get_source(options.source.as_str()).ok_or_else(|| SdkError::InvalidSource {
        name: options.source.as_str().to_string(),
    })?;
    let context = analysis_context(options)?;
    let result = load_daily(source, &context.filter, context.timezone, true, false);
    let supports_cache_read = source.capabilities().has_cache_read;
    let mut points = result
        .day_stats
        .iter()
        .map(|(date, day)| {
            let date = NaiveDate::parse_from_str(date, DATE_FORMAT).map_err(|error| {
                SdkError::Configuration(format!(
                    "invalid aggregated history date '{date}': {error}"
                ))
            })?;
            let costs = day
                .models
                .iter()
                .map(|(model, stats)| finite(calculate_cost(stats, model, &context.pricing_db)))
                .collect::<Vec<_>>();
            let known_count = costs.iter().filter(|cost| cost.is_some()).count();
            let cost_kind = model_cost_kind(&day.models).as_str().to_string();
            let pricing_source = pricing_source_for_models(&day.models, &context.pricing_db)
                .as_str()
                .to_string();
            let coverage = CostCoverage::from_stats(std::iter::once(&day.stats)).map(Into::into);
            let exact = (day.models.is_empty() || known_count == day.models.len())
                && cost_kind == "real"
                && matches!(pricing_source.as_str(), "recorded" | "live" | "cache")
                && !coverage
                    .as_ref()
                    .is_some_and(|row: &ApiEquivalentCostCoverage| row.cost_is_lower_bound);
            let status = if known_count == 0 && !day.models.is_empty() {
                HistoryCostStatus::Unknown
            } else if exact {
                HistoryCostStatus::Known
            } else {
                HistoryCostStatus::Partial
            };
            let cost_usd = (known_count > 0 || day.models.is_empty())
                .then(|| costs.into_iter().flatten().sum());
            Ok(DailyUsagePoint {
                date,
                currency: context.currency_code.clone(),
                tokens: token_breakdown(&day.stats, supports_cache_read),
                records: day.stats.records,
                cost: converted(cost_usd, context.currency.as_ref()),
                cost_usd,
                cost_status: status,
                cost_kind,
                pricing_source,
                api_equivalent_cost_coverage: coverage,
            })
        })
        .collect::<Result<Vec<_>, SdkError>>()?;
    points.sort_by_key(|point| point.date);
    Ok(UsageHistory {
        source: options.source,
        source_name: source.name().to_string(),
        display_name: source.display_name().to_string(),
        range: context.range,
        as_of_date: context.as_of_date,
        currency: context.currency_code,
        points,
        quality: AnalyticsQuality {
            valid_entries: result.valid,
            dedup_skipped_entries: result.skipped,
            parse_error_entries: result.parse_errors,
        },
    })
}

/// Builds daily usage history using persisted CLI configuration.
///
/// # Errors
///
/// Returns an error when configuration or source usage data cannot be loaded.
pub fn usage_history_with_cli_config(options: SummaryOptions) -> Result<UsageHistory, SdkError> {
    usage_history(&apply_cli_config(options)?)
}

fn validate_history_floats(history: &UsageHistory) -> Result<(), SdkError> {
    let valid = history.points.iter().all(|point| {
        point.cost.is_none_or(f64::is_finite)
            && point.cost_usd.is_none_or(f64::is_finite)
            && point.tokens.cache_hit_rate.is_none_or(f64::is_finite)
    });
    if valid {
        Ok(())
    } else {
        Err(SdkError::Configuration(
            "history contains non-finite values".to_string(),
        ))
    }
}

fn csv_cell(value: &str) -> String {
    if value.contains([',', '"', '\r', '\n']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

impl UsageHistory {
    /// Serializes this history as deterministic, pretty-printed JSON.
    ///
    /// # Errors
    ///
    /// Returns an error when the history contains non-finite numeric values or
    /// JSON serialization fails.
    pub fn to_json(&self) -> Result<String, SdkError> {
        validate_history_floats(self)?;
        serde_json::to_string_pretty(self)
            .map_err(|error| SdkError::Configuration(error.to_string()))
    }

    /// Serializes this history as deterministic RFC 4180-style CSV.
    ///
    /// # Errors
    ///
    /// Returns an error when the history contains non-finite numeric values.
    pub fn to_csv(&self) -> Result<String, SdkError> {
        validate_history_floats(self)?;
        let mut csv = String::from(
            "date,currency,input_tokens,output_tokens,reasoning_tokens,cache_creation_tokens,cache_read_tokens,cache_hit_rate,total_tokens,records,cost,cost_usd,cost_status,cost_kind,pricing_source,cost_is_lower_bound,valid_entries,dedup_skipped_entries,parse_error_entries\r\n",
        );
        for point in &self.points {
            let values = [
                point.date.to_string(),
                csv_cell(&point.currency),
                point.tokens.input_tokens.to_string(),
                point.tokens.output_tokens.to_string(),
                point.tokens.reasoning_tokens.to_string(),
                point.tokens.cache_creation_tokens.to_string(),
                point.tokens.cache_read_tokens.to_string(),
                point
                    .tokens
                    .cache_hit_rate
                    .map_or_else(String::new, |value| value.to_string()),
                point.tokens.total_tokens.to_string(),
                point.records.to_string(),
                point
                    .cost
                    .map_or_else(|| "unknown".to_string(), |value| value.to_string()),
                point
                    .cost_usd
                    .map_or_else(|| "unknown".to_string(), |value| value.to_string()),
                format!("{:?}", point.cost_status).to_ascii_lowercase(),
                csv_cell(&point.cost_kind),
                csv_cell(&point.pricing_source),
                point
                    .api_equivalent_cost_coverage
                    .as_ref()
                    .is_some_and(|coverage| coverage.cost_is_lower_bound)
                    .to_string(),
                self.quality.valid_entries.to_string(),
                self.quality.dedup_skipped_entries.to_string(),
                self.quality.parse_error_entries.to_string(),
            ];
            csv.push_str(&values.join(","));
            csv.push_str("\r\n");
        }
        Ok(csv)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn source_diagnostics_project_every_registered_source() {
        let diagnostics = diagnose_usage_sources().expect("source diagnostics");

        assert_eq!(diagnostics.len(), 29);
        assert_eq!(
            diagnostics.first().map(|row| row.name.as_str()),
            Some("claude")
        );
        assert_eq!(diagnostics.last().map(|row| row.name.as_str()), Some("dsh"));
        assert!(diagnostics.iter().all(|row| !row.setup.is_empty()));
    }

    #[test]
    fn catalog_is_a_complete_unique_registry_projection() {
        let descriptors = list_usage_sources().expect("registry projection");
        let names = descriptors
            .iter()
            .map(|descriptor| descriptor.name.as_str())
            .collect::<HashSet<_>>();

        assert_eq!(descriptors.len(), 29);
        assert_eq!(names.len(), descriptors.len());
        assert_eq!(
            descriptors
                .iter()
                .map(|descriptor| descriptor.name.as_str())
                .collect::<Vec<_>>(),
            crate::source::all_sources()
                .map(crate::source::Source::name)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn project_drilldown_rejects_sources_without_project_capability() {
        let options = crate::sdk::SummaryOptions {
            source: UsageSource::Codex,
            range: crate::sdk::UsageRange::Today,
            offline: true,
            ..crate::sdk::SummaryOptions::default()
        };
        let error = summarize_project_drilldown(&options)
            .expect_err("Codex project drilldown must fail clearly");

        assert!(
            error
                .to_string()
                .contains("does not support project drilldown")
        );
    }

    #[test]
    fn history_exports_are_deterministic_and_preserve_unknown_cost() {
        let history = UsageHistory {
            source: UsageSource::Claude,
            source_name: "claude".to_string(),
            display_name: "Claude Code".to_string(),
            range: crate::sdk::UsageRange::ThisMonth,
            as_of_date: chrono::NaiveDate::from_ymd_opt(2026, 9, 2).unwrap(),
            currency: "USD".to_string(),
            points: vec![DailyUsagePoint {
                date: chrono::NaiveDate::from_ymd_opt(2026, 9, 1).unwrap(),
                currency: "USD".to_string(),
                tokens: crate::sdk::TokenBreakdown::default(),
                records: 2,
                cost: None,
                cost_usd: None,
                cost_status: HistoryCostStatus::Unknown,
                cost_kind: "unknown".to_string(),
                pricing_source: "unknown".to_string(),
                api_equivalent_cost_coverage: None,
            }],
            quality: AnalyticsQuality {
                valid_entries: 2,
                dedup_skipped_entries: 1,
                parse_error_entries: 0,
            },
        };

        assert_eq!(history.to_json().unwrap(), history.to_json().unwrap());
        let csv = history.to_csv().unwrap();
        assert!(csv.contains("cost_status"));
        assert!(csv.contains(",unknown,unknown,"));
        assert!(csv.ends_with("\r\n"));
    }
}
