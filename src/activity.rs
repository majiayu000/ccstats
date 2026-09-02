//! Per-model-turn and tool-call evidence for graphical and SDK consumers.

use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::catalog::{AnalyticsQuality, apply_cli_config};
use crate::core::{DateFilter, RawEntry, aggregate_tools};
use crate::sdk::{SdkError, SummaryOptions, TokenBreakdown, UsageRange, UsageSource};
use crate::source::{get_source, load_entries, load_tool_calls};
use crate::utils::Timezone;

const MAX_RECENT_TURNS: usize = 100;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolUsage {
    pub name: String,
    pub calls: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelTurnUsage {
    pub timestamp: String,
    pub session_id: String,
    pub message_id: Option<String>,
    pub project_path: String,
    pub model: String,
    pub model_call_count: i64,
    pub tokens: TokenBreakdown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TurnToolBreakdown {
    pub source: UsageSource,
    pub source_name: String,
    pub display_name: String,
    pub range: UsageRange,
    pub total_turns: usize,
    pub turns: Vec<ModelTurnUsage>,
    pub tool_calls_supported: bool,
    pub tool_calls_total: u64,
    pub tools: Vec<ToolUsage>,
    pub quality: AnalyticsQuality,
}

fn turn_usage(entry: RawEntry, supports_cache_read: bool) -> ModelTurnUsage {
    let stats = entry.to_stats();
    ModelTurnUsage {
        timestamp: entry.timestamp,
        session_id: entry.session_id,
        message_id: entry.message_id,
        project_path: entry.project_path,
        model: entry.model,
        model_call_count: stats.count,
        tokens: TokenBreakdown {
            input_tokens: stats.input_tokens,
            output_tokens: stats.output_tokens,
            reasoning_tokens: stats.reasoning_tokens,
            cache_creation_tokens: stats.cache_creation,
            cache_creation_1h_tokens: stats.cache_creation_1h,
            cache_read_tokens: stats.cache_read,
            reported_total_adjustment: stats.reported_total_adjustment,
            cache_hit_rate: stats.cache_hit_rate(supports_cache_read),
            total_tokens: stats.total_tokens(),
        },
    }
}

/// Returns recent deduplicated model turns and independently counted tool calls.
///
/// Tool calls are intentionally not assigned token counts because the supported
/// source logs do not provide trustworthy per-tool token attribution.
///
/// # Errors
///
/// Returns an error when the source, date range, timezone, or local usage data
/// cannot be loaded.
pub fn turn_tool_breakdown(options: &SummaryOptions) -> Result<TurnToolBreakdown, SdkError> {
    let source = get_source(options.source.as_str()).ok_or_else(|| SdkError::InvalidSource {
        name: options.source.as_str().to_string(),
    })?;
    let timezone = Timezone::parse(options.timezone.as_deref())
        .map_err(|error| SdkError::Configuration(error.to_string()))?;
    let today = timezone.to_fixed_offset(Utc::now()).date_naive();
    let (since, until) = options.range.resolve(today)?;
    let mut filter = DateFilter::new(since, until);
    if let Some((since_timestamp, until_timestamp)) = options.range.timestamp_bounds() {
        filter = filter.with_exact_timestamp_range(since_timestamp, until_timestamp);
    }

    let (mut entries, dedup_skipped_entries, parse_error_entries) =
        load_entries(source, &filter, timezone);
    let total_turns = entries.len();
    entries.sort_by(|left, right| {
        right
            .timestamp_ms
            .cmp(&left.timestamp_ms)
            .then_with(|| left.session_id.cmp(&right.session_id))
            .then_with(|| left.message_id.cmp(&right.message_id))
    });
    entries.truncate(MAX_RECENT_TURNS);

    let capabilities = source.capabilities();
    let tool_summary = aggregate_tools(&load_tool_calls(source, &filter, timezone));
    let mut tools = tool_summary
        .tools
        .into_iter()
        .map(|tool| ToolUsage {
            name: tool.name,
            calls: tool.calls,
        })
        .collect::<Vec<_>>();
    tools.sort_by(|left, right| {
        right
            .calls
            .cmp(&left.calls)
            .then_with(|| left.name.cmp(&right.name))
    });

    Ok(TurnToolBreakdown {
        source: options.source,
        source_name: source.name().to_string(),
        display_name: source.display_name().to_string(),
        range: options.range.clone(),
        total_turns,
        turns: entries
            .into_iter()
            .map(|entry| turn_usage(entry, capabilities.has_cache_read))
            .collect(),
        tool_calls_supported: capabilities.has_tool_calls,
        tool_calls_total: tool_summary.total,
        tools,
        quality: AnalyticsQuality {
            valid_entries: i64::try_from(total_turns).unwrap_or(i64::MAX),
            dedup_skipped_entries,
            parse_error_entries,
        },
    })
}

/// Returns turn and tool evidence using persisted CLI configuration.
///
/// # Errors
///
/// Returns an error when configuration or source usage data cannot be loaded.
pub fn turn_tool_breakdown_with_cli_config(
    options: SummaryOptions,
) -> Result<TurnToolBreakdown, SdkError> {
    turn_tool_breakdown(&apply_cli_config(options)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{CostKind, Endpoint};

    #[test]
    fn turn_preserves_source_authoritative_total() {
        let turn = turn_usage(
            RawEntry {
                timestamp: "2026-09-02T08:00:00Z".to_string(),
                timestamp_ms: 1,
                date_str: "2026-09-02".to_string(),
                message_id: Some("message-1".to_string()),
                session_key: "session-1".to_string(),
                session_id: "session-1".to_string(),
                project_path: "/work/ccstats".to_string(),
                model: "test-model".to_string(),
                input_tokens: 10,
                output_tokens: 5,
                cache_creation: 0,
                cache_creation_1h: 0,
                cache_read: 3,
                reasoning_tokens: 2,
                reported_total_tokens: Some(25),
                stop_reason: Some("end_turn".to_string()),
                cost_kind: CostKind::Real,
                endpoint: Endpoint::Unknown,
                call_count: 1,
                recorded_cost_usd: None,
                api_equivalent_priced_tokens: 0,
                api_equivalent_coverage_tokens: 0,
            },
            true,
        );

        assert_eq!(turn.tokens.total_tokens, 25);
        assert_eq!(turn.model_call_count, 1);
        assert_eq!(turn.tokens.cache_hit_rate, Some(3.0 / 13.0 * 100.0));
    }
}
