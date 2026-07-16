//! Unit tests for `aggregate_by_endpoint`.
//!
//! Kept in a separate file because `aggregator.rs` is at the module size limit.

use crate::core::aggregate_by_endpoint;
use crate::core::types::{CostKind, Endpoint, RawEntry};

fn entry(model: &str, endpoint: Endpoint, input: i64) -> RawEntry {
    RawEntry {
        timestamp: "2025-01-01T00:00:00Z".to_string(),
        timestamp_ms: 0,
        date_str: "2025-01-01".to_string(),
        message_id: None,
        session_key: "s".to_string(),
        session_id: "s".to_string(),
        project_path: String::new(),
        model: model.to_string(),
        input_tokens: input,
        output_tokens: 0,
        cache_creation: 0,
        cache_creation_1h: 0,
        cache_read: 0,
        reasoning_tokens: 0,
        stop_reason: Some("end_turn".to_string()),
        cost_kind: CostKind::Real,
        endpoint,
    }
}

#[test]
fn groups_orders_and_sums_across_endpoints() {
    // Insert out of canonical order to prove ORDER is applied, not insertion order.
    let entries = vec![
        entry("m", Endpoint::Proxy, 100),
        entry("m", Endpoint::Native, 10),
        entry("m", Endpoint::Unknown, 1),
        entry("m", Endpoint::Native, 20),
    ];

    let result = aggregate_by_endpoint(entries);

    // Canonical order: Native, Proxy, Unknown.
    assert_eq!(
        result.iter().map(|e| e.endpoint).collect::<Vec<_>>(),
        vec![Endpoint::Native, Endpoint::Proxy, Endpoint::Unknown]
    );

    let native = &result[0];
    assert_eq!(native.stats.count, 2);
    assert_eq!(native.stats.input_tokens, 30);
    // Per-model map accumulates within the endpoint.
    assert_eq!(native.models.get("m").map(|s| s.input_tokens), Some(30));

    assert_eq!(result[1].stats.count, 1);
    assert_eq!(result[1].stats.input_tokens, 100);
    assert_eq!(result[2].stats.count, 1);
}

#[test]
fn empty_input_yields_no_endpoints() {
    assert!(aggregate_by_endpoint(Vec::new()).is_empty());
}

#[test]
fn omits_absent_endpoints() {
    // Only proxy present -> result has exactly one row, not three.
    let result = aggregate_by_endpoint(vec![entry("m", Endpoint::Proxy, 5)]);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].endpoint, Endpoint::Proxy);
}
