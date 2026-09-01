//! Fixture tests for `--source all` real-only token totals.
//!
//! Kept out of `types.rs` / `aggregator.rs` because those modules are at the
//! size limit.

use std::collections::HashMap;

use crate::core::{
    CostKind, Endpoint, RawEntry, aggregate_daily, apply_real_token_totals_for_all_source,
    merge_day_stats,
};

fn entry(date: &str, model: &str, input: i64, cost_kind: CostKind) -> RawEntry {
    RawEntry {
        timestamp: "2025-01-15T12:00:00Z".to_string(),
        timestamp_ms: 1_737_000_000_000,
        date_str: date.to_string(),
        message_id: None,
        session_key: model.to_string(),
        session_id: model.to_string(),
        project_path: String::new(),
        model: model.to_string(),
        input_tokens: input,
        output_tokens: 0,
        cache_creation: 0,
        cache_creation_1h: 0,
        cache_read: 0,
        reasoning_tokens: 0,
        stop_reason: Some("end_turn".to_string()),
        cost_kind,
        endpoint: Endpoint::Unknown,
        call_count: 1,
        reported_total_tokens: None,
        recorded_cost_usd: None,
        api_equivalent_priced_tokens: 0,
        api_equivalent_coverage_tokens: 0,
    }
}

#[test]
fn all_source_helper_keeps_real_tokens_and_proxy_sidecar() {
    let date = "2025-01-15";
    let claude = aggregate_daily(vec![entry(date, "claude-sonnet", 100, CostKind::Real)]);
    let grok = aggregate_daily(vec![entry(date, "grok", 1000, CostKind::EstimatedProxy)]);

    let mut combined = claude;
    merge_day_stats(&mut combined, grok);
    assert_eq!(combined[date].stats.input_tokens, 1100);
    assert_eq!(combined[date].stats.total_tokens(), 1100);

    apply_real_token_totals_for_all_source(&mut combined);

    let day = &combined[date];
    assert_eq!(day.stats.input_tokens, 100);
    assert_eq!(day.stats.total_tokens(), 100);
    assert_eq!(day.stats.estimated_proxy.input_tokens, 1000);
    assert_eq!(day.models["claude-sonnet"].input_tokens, 100);
    assert_eq!(day.models["grok"].input_tokens, 0);
    assert_eq!(day.models["grok"].estimated_proxy.input_tokens, 1000);
    assert_eq!(day.models["claude-sonnet"].cost_kind(), CostKind::Real);
    assert_eq!(day.models["grok"].cost_kind(), CostKind::EstimatedProxy);
}

#[test]
fn grok_only_aggregation_keeps_proxy_tokens_without_helper() {
    let date = "2025-01-15";
    let grok = aggregate_daily(vec![entry(date, "grok", 1000, CostKind::EstimatedProxy)]);
    let mixed = {
        let claude = aggregate_daily(vec![entry(date, "claude-sonnet", 100, CostKind::Real)]);
        let mut days = claude;
        merge_day_stats(
            &mut days,
            aggregate_daily(vec![entry(date, "grok", 1000, CostKind::EstimatedProxy)]),
        );
        days
    };

    assert_eq!(grok[date].stats.input_tokens, 1000);
    assert_eq!(grok[date].stats.total_tokens(), 1000);
    assert_eq!(grok[date].stats.estimated_proxy.input_tokens, 1000);
    assert_eq!(grok[date].stats.cost_kind(), CostKind::EstimatedProxy);
    assert_eq!(mixed[date].stats.input_tokens, 1100);
    assert_eq!(mixed[date].stats.total_tokens(), 1100);
    assert_eq!(mixed[date].stats.estimated_proxy.input_tokens, 1000);
    assert_eq!(mixed[date].stats.cost_kind(), CostKind::Mixed);
}

#[test]
fn helper_is_a_no_op_on_empty_days() {
    let mut empty = HashMap::new();
    apply_real_token_totals_for_all_source(&mut empty);
    assert!(empty.is_empty());
}
