//! Coherent, exactly filtered projections of the same deduplicated usage facts.

use super::{
    AnalysisContext, AnalyticsQuality, DailyUsagePoint, Deserialize, HashMap, HistoryCostStatus,
    NaiveDate, ProjectDrilldown, ProjectDrilldownSummary, SdkError, Serialize, SessionDrilldown,
    Stats, SummaryOptions, TokenBreakdown, UsageHistory, UsageMetrics, UsageSource, Utc,
    aggregate_projects, analysis_context, apply_cli_config, converted, get_source, token_breakdown,
    usage_metrics,
};
use crate::core::{DayStats, RawEntry, aggregate_sessions};
use crate::source::load_entries_cancellable;
use chrono::{DateTime, Timelike};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AnalysisFilter {
    pub model: Option<String>,
    pub project: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AnalysisSummary {
    #[serde(flatten)]
    pub metrics: UsageMetrics,
    pub valid_entries: i64,
    pub parse_error_entries: usize,
    pub skipped_entries: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct HourlyUsagePoint {
    /// ISO timestamp including the configured timezone offset; DST hours stay distinct.
    pub hour: String,
    pub tokens: TokenBreakdown,
    pub records: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct UsageAnalysis {
    pub summary: AnalysisSummary,
    pub projects: ProjectDrilldownSummary,
    pub history: UsageHistory,
    pub hourly: Vec<HourlyUsagePoint>,
    pub available_models: Vec<String>,
    pub available_projects: Vec<String>,
    pub since: Option<NaiveDate>,
    pub until: Option<NaiveDate>,
    pub timezone: String,
}

/// Read each source once, then derive every projection after exact model/project filtering.
/// Missing project metadata stays empty and is never inferred from prompts.
///
/// # Errors
/// Returns configuration, range, pricing and invalid timestamp errors explicitly.
pub fn usage_analysis_with_cli_config(
    options: SummaryOptions,
    filter: &AnalysisFilter,
) -> Result<UsageAnalysis, SdkError> {
    usage_analysis_cancellable_with_cli_config(options, filter, &|| false)
}

/// Same analysis with cooperative cancellation between source files.
///
/// # Errors
/// Returns an explicit error on cancellation; partial aggregates are never returned.
pub fn usage_analysis_cancellable_with_cli_config(
    options: SummaryOptions,
    filter: &AnalysisFilter,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Result<UsageAnalysis, SdkError> {
    let check_cancelled = || {
        if cancelled() {
            Err(SdkError::Configuration("Analysis cancelled".to_string()))
        } else {
            Ok(())
        }
    };
    check_cancelled()?;
    let options = apply_cli_config(options)?;
    let source = get_source(options.source.as_str()).ok_or_else(|| SdkError::InvalidSource {
        name: options.source.as_str().to_string(),
    })?;
    let context = analysis_context(&options)?;
    let (entries, skipped, errors) =
        load_entries_cancellable(source, &context.filter, context.timezone, cancelled);
    check_cancelled()?;
    let result = project_entries(
        entries,
        skipped,
        errors,
        &options,
        filter,
        &context,
        source.capabilities().has_cache_read,
        source.display_name(),
    )?;
    check_cancelled()?;
    Ok(result)
}

#[expect(
    clippy::too_many_arguments,
    reason = "one internal projection boundary for already loaded facts"
)]
fn project_entries(
    mut entries: Vec<RawEntry>,
    skipped: i64,
    errors: usize,
    options: &SummaryOptions,
    filter: &AnalysisFilter,
    context: &AnalysisContext,
    cache_read: bool,
    display_name: &str,
) -> Result<UsageAnalysis, SdkError> {
    let available_models = entries
        .iter()
        .map(|entry| entry.model.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let available_projects = entries
        .iter()
        .filter(|entry| !entry.project_path.is_empty())
        .map(|entry| entry.project_path.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    entries.retain(|entry| {
        filter
            .model
            .as_ref()
            .is_none_or(|model| entry.model == *model)
            && filter
                .project
                .as_ref()
                .is_none_or(|project| entry.project_path == *project)
    });
    let mut total = DayStats::default();
    let mut days = BTreeMap::<NaiveDate, DayStats>::new();
    let mut hours = BTreeMap::<String, Stats>::new();
    for entry in &entries {
        let timestamp = DateTime::parse_from_rfc3339(&entry.timestamp).map_err(|error| {
            SdkError::Configuration(format!("invalid usage timestamp: {error}"))
        })?;
        let local = context
            .timezone
            .to_fixed_offset(timestamp.with_timezone(&Utc));
        let stats = entry.to_stats();
        total.add_stats(entry.model.clone(), &stats);
        days.entry(local.date_naive())
            .or_default()
            .add_stats(entry.model.clone(), &stats);
        let hour = local
            .with_minute(0)
            .and_then(|time| time.with_second(0))
            .and_then(|time| time.with_nanosecond(0))
            .ok_or_else(|| SdkError::Configuration("invalid usage hour".to_string()))?;
        hours.entry(hour.to_rfc3339()).or_default().add(&stats);
    }
    let quality = AnalyticsQuality {
        valid_entries: total.stats.records,
        dedup_skipped_entries: skipped,
        parse_error_entries: errors,
    };
    let projects = project_sessions(entries, options.source, context, cache_read);
    let points = daily_points(days, context, cache_read);
    let (since, until) = options.range.resolve(context.as_of_date)?;
    Ok(UsageAnalysis {
        summary: AnalysisSummary {
            metrics: usage_metrics(&total.stats, &total.models, context, cache_read),
            valid_entries: quality.valid_entries,
            parse_error_entries: errors,
            skipped_entries: skipped,
        },
        projects: ProjectDrilldownSummary {
            source: options.source,
            source_name: options.source.as_str().to_string(),
            display_name: display_name.to_string(),
            range: options.range.clone(),
            currency: context.currency_code.clone(),
            quality,
            projects,
        },
        history: UsageHistory {
            source: options.source,
            source_name: options.source.as_str().to_string(),
            display_name: display_name.to_string(),
            range: options.range.clone(),
            as_of_date: context.as_of_date,
            currency: context.currency_code.clone(),
            quality,
            points,
        },
        hourly: hours
            .into_iter()
            .map(|(hour, stats)| HourlyUsagePoint {
                hour,
                tokens: token_breakdown(&stats, cache_read),
                records: stats.records,
            })
            .collect(),
        available_models,
        available_projects,
        since,
        until,
        timezone: options
            .timezone
            .clone()
            .unwrap_or_else(|| "local".to_string()),
    })
}

fn project_sessions(
    mut entries: Vec<RawEntry>,
    source: UsageSource,
    context: &AnalysisContext,
    cache_read: bool,
) -> Vec<ProjectDrilldown> {
    // Deduplication keeps file provenance; display grouping uses the source session ID.
    if source == UsageSource::Codex {
        for entry in &mut entries {
            entry.session_key.clone_from(&entry.session_id);
        }
    }
    let sessions = aggregate_sessions(entries);
    let mut grouped = HashMap::<String, Vec<SessionDrilldown>>::new();
    for session in &sessions {
        grouped
            .entry(session.project_path.clone())
            .or_default()
            .push(SessionDrilldown {
                session_id: session.session_id.clone(),
                project_path: session.project_path.clone(),
                first_timestamp: session.first_timestamp.clone(),
                last_timestamp: session.last_timestamp.clone(),
                metrics: usage_metrics(&session.stats, &session.models, context, cache_read),
            });
    }
    let mut projects = aggregate_projects(sessions)
        .into_iter()
        .map(|project| {
            let mut sessions = grouped.remove(&project.project_path).unwrap_or_default();
            sessions.sort_by(|a, b| {
                b.last_timestamp
                    .cmp(&a.last_timestamp)
                    .then_with(|| a.session_id.cmp(&b.session_id))
            });
            ProjectDrilldown {
                project_name: if project.project_path.is_empty() {
                    "未记录项目".to_string()
                } else {
                    project.project_name
                },
                project_path: project.project_path,
                session_count: sessions.len(),
                sessions,
                metrics: usage_metrics(&project.stats, &project.models, context, cache_read),
            }
        })
        .collect::<Vec<_>>();
    projects.sort_by(|a, b| {
        b.metrics
            .tokens
            .total_tokens
            .cmp(&a.metrics.tokens.total_tokens)
            .then_with(|| a.project_path.cmp(&b.project_path))
    });
    projects
}

fn daily_points(
    days: BTreeMap<NaiveDate, DayStats>,
    context: &AnalysisContext,
    cache_read: bool,
) -> Vec<DailyUsagePoint> {
    days.into_iter()
        .map(|(date, day)| {
            let metrics = usage_metrics(&day.stats, &day.models, context, cache_read);
            let known = metrics
                .models
                .iter()
                .filter_map(|model| model.cost_usd)
                .collect::<Vec<_>>();
            let cost_usd = (!known.is_empty()).then(|| known.iter().sum());
            let exact = metrics.cost_usd.is_some()
                && metrics.cost_kind == "real"
                && matches!(
                    metrics.pricing_source.as_str(),
                    "recorded" | "live" | "cache"
                )
                && !metrics
                    .api_equivalent_cost_coverage
                    .as_ref()
                    .is_some_and(|coverage| coverage.cost_is_lower_bound);
            DailyUsagePoint {
                date,
                currency: metrics.currency,
                tokens: metrics.tokens,
                records: day.stats.records,
                cost: converted(cost_usd, context.currency.as_ref()),
                cost_usd,
                cost_status: if known.is_empty() {
                    HistoryCostStatus::Unknown
                } else if exact {
                    HistoryCostStatus::Known
                } else {
                    HistoryCostStatus::Partial
                },
                cost_kind: metrics.cost_kind,
                pricing_source: metrics.pricing_source,
                api_equivalent_cost_coverage: metrics.api_equivalent_cost_coverage,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(timestamp: &str, model: &str, project: &str, tokens: i64) -> RawEntry {
        serde_json::from_value(serde_json::json!({
            "timestamp": timestamp, "timestamp_ms": DateTime::parse_from_rfc3339(timestamp).unwrap().timestamp_millis(),
            "date_str": &timestamp[..10], "message_id": null, "session_id": "real-session-id", "project_path": project,
            "model": model, "input_tokens": tokens, "output_tokens": 0, "cache_creation": 0, "cache_read": 0, "reasoning_tokens": 0,
            "stop_reason": "complete"
        })).unwrap()
    }

    #[test]
    fn cancelled_query_does_not_return_partial_usage() {
        let result = usage_analysis_cancellable_with_cli_config(
            SummaryOptions::default(),
            &AnalysisFilter::default(),
            &|| true,
        );
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Analysis cancelled")
        );
    }

    #[test]
    fn exact_filters_reconcile_summary_sessions_days_and_hours_in_selected_timezone() {
        let options = SummaryOptions {
            source: UsageSource::Codex,
            timezone: Some("Asia/Shanghai".into()),
            offline: true,
            ..SummaryOptions::default()
        };
        let context = analysis_context(&options).unwrap();
        let entries = vec![
            entry("2026-09-01T17:15:00Z", "gpt-5", "/work/app", 100),
            entry("2026-09-01T17:40:00Z", "gpt-5", "/work/app", 20),
            entry("2026-09-02T02:00:00Z", "gpt-5", "/work/app", 30),
            entry("2026-09-01T17:15:00Z", "gpt-5-mini", "/work/app", 700),
            entry("2026-09-01T17:15:00Z", "gpt-5", "/work/app-extra", 800),
        ];
        let result = project_entries(
            entries,
            2,
            1,
            &options,
            &AnalysisFilter {
                model: Some("gpt-5".into()),
                project: Some("/work/app".into()),
            },
            &context,
            true,
            "Codex",
        )
        .unwrap();
        assert_eq!(result.summary.metrics.tokens.total_tokens, 150);
        assert_eq!(result.summary.valid_entries, 3);
        assert_eq!(result.summary.parse_error_entries, 1);
        assert_eq!(result.summary.skipped_entries, 2);
        assert_eq!(result.projects.projects.len(), 1);
        assert_eq!(
            result.projects.projects[0].sessions[0].session_id,
            "real-session-id"
        );
        assert_eq!(
            result.projects.projects[0].sessions[0]
                .metrics
                .tokens
                .total_tokens,
            150
        );
        assert_eq!(result.history.points[0].date.to_string(), "2026-09-02");
        assert_eq!(result.history.points[0].tokens.total_tokens, 150);
        assert_eq!(result.hourly.len(), 2);
        assert_eq!(result.hourly[0].hour, "2026-09-02T01:00:00+08:00");
        assert_eq!(result.hourly[0].tokens.total_tokens, 120);
        assert_eq!(result.available_models, ["gpt-5", "gpt-5-mini"]);
        assert_eq!(result.available_projects, ["/work/app", "/work/app-extra"]);
    }

    #[test]
    fn missing_metadata_and_unknown_prices_remain_explicit() {
        let options = SummaryOptions {
            offline: true,
            ..SummaryOptions::default()
        };
        let context = analysis_context(&options).unwrap();
        let result = project_entries(
            vec![entry("2026-09-02T02:00:00Z", "not-a-priced-model", "", 120)],
            0,
            0,
            &options,
            &AnalysisFilter::default(),
            &context,
            true,
            "Test",
        )
        .unwrap();
        assert_eq!(result.projects.projects[0].project_name, "未记录项目");
        assert!(result.available_projects.is_empty());
        assert_eq!(result.summary.metrics.cost_usd, None);
        assert_eq!(
            result.history.points[0].cost_status,
            HistoryCostStatus::Unknown
        );
        assert_eq!(result.history.points[0].cost_usd, None);
    }
}
