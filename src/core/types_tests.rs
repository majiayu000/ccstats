use super::*;
use chrono::NaiveDate;

fn make_stats(input: i64, output: i64, cache_c: i64, cache_r: i64, reason: i64) -> Stats {
    Stats {
        input_tokens: input,
        output_tokens: output,
        cache_creation: cache_c,
        cache_creation_1h: 0,
        cache_read: cache_r,
        reasoning_tokens: reason,
        count: 1,
        skipped_chunks: 0,
        estimated_proxy: CostTokens::default(),
        ..Default::default()
    }
}

#[test]
fn cache_hit_rate_uses_all_input_side_tokens() {
    let stats = make_stats(100, 50, 20, 80, 10);
    assert_eq!(stats.cache_hit_rate(true), Some(40.0));
}

#[test]
fn cache_hit_rate_is_zero_when_supported_without_hits() {
    let stats = make_stats(100, 50, 0, 0, 0);
    assert_eq!(stats.cache_hit_rate(true), Some(0.0));
}

#[test]
fn cache_hit_rate_is_unavailable_without_input_or_source_support() {
    assert_eq!(Stats::default().cache_hit_rate(true), None);
    assert_eq!(make_stats(100, 0, 0, 50, 0).cache_hit_rate(false), None);
}

#[test]
fn cache_hit_rate_clamps_negative_components() {
    let stats = make_stats(-100, 0, -20, 80, 0);
    assert_eq!(stats.cache_hit_rate(true), Some(100.0));
}

// --- Stats ---

#[test]
fn stats_default_all_zero() {
    let s = Stats::default();
    assert_eq!(s.input_tokens, 0);
    assert_eq!(s.output_tokens, 0);
    assert_eq!(s.cache_creation, 0);
    assert_eq!(s.cache_read, 0);
    assert_eq!(s.reasoning_tokens, 0);
    assert_eq!(s.count, 0);
    assert_eq!(s.skipped_chunks, 0);
}

#[test]
fn stats_total_tokens_sums_five_fields() {
    let s = make_stats(100, 200, 50, 30, 20);
    assert_eq!(s.total_tokens(), 400); // 100+200+50+30+20
}

#[test]
fn stats_total_tokens_excludes_count_and_skipped() {
    let s = Stats {
        input_tokens: 10,
        output_tokens: 5,
        cache_creation: 0,
        cache_creation_1h: 0,
        cache_read: 0,
        reasoning_tokens: 0,
        count: 999,
        skipped_chunks: 42,
        estimated_proxy: CostTokens::default(),
        ..Default::default()
    };
    assert_eq!(s.total_tokens(), 15);
}

#[test]
fn stats_total_tokens_zero_when_default() {
    assert_eq!(Stats::default().total_tokens(), 0);
}

#[test]
fn stats_aggregation_saturates_at_i64_max() {
    let mut total = Stats {
        input_tokens: i64::MAX,
        count: i64::MAX,
        records: i64::MAX,
        ..Default::default()
    };
    total.add(&Stats {
        input_tokens: 1,
        count: 1,
        records: 1,
        ..Default::default()
    });

    assert_eq!(total.input_tokens, i64::MAX);
    assert_eq!(total.count, i64::MAX);
    assert_eq!(total.records, i64::MAX);
    assert_eq!(total.total_tokens(), i64::MAX);
}

#[test]
fn stats_add_accumulates_all_fields() {
    let mut a = make_stats(10, 20, 5, 3, 1);
    a.skipped_chunks = 2;
    let b = Stats {
        input_tokens: 100,
        output_tokens: 200,
        cache_creation: 50,
        cache_creation_1h: 0,
        cache_read: 30,
        reasoning_tokens: 10,
        count: 3,
        skipped_chunks: 5,
        estimated_proxy: CostTokens::default(),
        ..Default::default()
    };
    a.add(&b);
    assert_eq!(a.input_tokens, 110);
    assert_eq!(a.output_tokens, 220);
    assert_eq!(a.cache_creation, 55);
    assert_eq!(a.cache_read, 33);
    assert_eq!(a.reasoning_tokens, 11);
    assert_eq!(a.count, 4);
    assert_eq!(a.skipped_chunks, 7);
}

#[test]
fn stats_add_to_default() {
    let mut a = Stats::default();
    let b = make_stats(5, 10, 15, 20, 25);
    a.add(&b);
    assert_eq!(a.input_tokens, 5);
    assert_eq!(a.output_tokens, 10);
    assert_eq!(a.total_tokens(), 75);
}

// --- DayStats ---

#[test]
fn day_stats_add_single_model() {
    let mut ds = DayStats::default();
    let s = make_stats(100, 200, 0, 0, 0);
    ds.add_stats("claude-3".into(), &s);
    assert_eq!(ds.stats.input_tokens, 100);
    assert_eq!(ds.stats.output_tokens, 200);
    assert_eq!(ds.stats.count, 1);
    assert_eq!(ds.models.len(), 1);
    assert_eq!(ds.models["claude-3"].input_tokens, 100);
}

#[test]
fn day_stats_add_same_model_twice() {
    let mut ds = DayStats::default();
    ds.add_stats("gpt-4".into(), &make_stats(10, 20, 0, 0, 0));
    ds.add_stats("gpt-4".into(), &make_stats(30, 40, 0, 0, 0));
    assert_eq!(ds.stats.input_tokens, 40);
    assert_eq!(ds.stats.output_tokens, 60);
    assert_eq!(ds.stats.count, 2);
    assert_eq!(ds.models.len(), 1);
    assert_eq!(ds.models["gpt-4"].input_tokens, 40);
}

#[test]
fn day_stats_add_multiple_models() {
    let mut ds = DayStats::default();
    ds.add_stats("a".into(), &make_stats(10, 0, 0, 0, 0));
    ds.add_stats("b".into(), &make_stats(20, 0, 0, 0, 0));
    ds.add_stats("c".into(), &make_stats(30, 0, 0, 0, 0));
    assert_eq!(ds.stats.input_tokens, 60);
    assert_eq!(ds.models.len(), 3);
}

// --- RawEntry ---

#[test]
fn raw_entry_to_stats() {
    let entry = RawEntry {
        timestamp: String::new(),
        timestamp_ms: 0,
        date_str: String::new(),
        message_id: None,
        session_key: String::new(),
        session_id: String::new(),
        project_path: String::new(),
        model: String::new(),
        input_tokens: 100,
        output_tokens: 200,
        cache_creation: 50,
        cache_creation_1h: 0,
        cache_read: 30,
        reasoning_tokens: 10,
        stop_reason: None,
        cost_kind: CostKind::Real,
        endpoint: Endpoint::Unknown,
        call_count: 1,
        reported_total_tokens: None,
        recorded_cost_usd: None,
        api_equivalent_priced_tokens: 0,
        api_equivalent_coverage_tokens: 0,
    };
    let s = entry.to_stats();
    assert_eq!(s.input_tokens, 100);
    assert_eq!(s.output_tokens, 200);
    assert_eq!(s.cache_creation, 50);
    assert_eq!(s.cache_read, 30);
    assert_eq!(s.reasoning_tokens, 10);
    assert_eq!(s.count, 1);
    assert_eq!(s.records, 1);
    assert_eq!(s.skipped_chunks, 0);
    assert_eq!(s.recorded_cost_entries, 0);
    assert_eq!(s.priced_tokens.input_tokens, 100);
}

#[test]
fn raw_entry_to_stats_uses_call_count_and_recorded_cost() {
    let entry = RawEntry {
        timestamp: String::new(),
        timestamp_ms: 0,
        date_str: String::new(),
        message_id: None,
        session_key: String::new(),
        session_id: String::new(),
        project_path: String::new(),
        model: String::new(),
        input_tokens: 10,
        output_tokens: 2,
        cache_creation: 0,
        cache_creation_1h: 0,
        cache_read: 0,
        reasoning_tokens: 0,
        stop_reason: None,
        cost_kind: CostKind::Real,
        endpoint: Endpoint::Unknown,
        call_count: 5,
        reported_total_tokens: None,
        recorded_cost_usd: Some(1.25),
        api_equivalent_priced_tokens: 0,
        api_equivalent_coverage_tokens: 0,
    };
    let stats = entry.to_stats();
    assert_eq!(stats.count, 5);
    assert_eq!(stats.records, 1);
    assert!((stats.recorded_cost_usd - 1.25).abs() < 1e-12);
    assert_eq!(stats.recorded_cost_entries, 1);
    assert!(!stats.priced_tokens.has_entries());
}

#[test]
fn raw_entry_preserves_independent_reported_total_without_repricing_the_delta() {
    let entry = RawEntry {
        timestamp: String::new(),
        timestamp_ms: 0,
        date_str: String::new(),
        message_id: None,
        session_key: String::new(),
        session_id: String::new(),
        project_path: String::new(),
        model: "provider/model".to_string(),
        input_tokens: 3,
        output_tokens: 4,
        cache_creation: 0,
        cache_creation_1h: 0,
        cache_read: 0,
        reasoning_tokens: 0,
        reported_total_tokens: Some(9),
        stop_reason: Some("complete".to_string()),
        cost_kind: CostKind::Real,
        endpoint: Endpoint::Unknown,
        call_count: 1,
        recorded_cost_usd: None,
        api_equivalent_priced_tokens: 0,
        api_equivalent_coverage_tokens: 0,
    };

    let stats = entry.to_stats();
    assert_eq!(stats.total_tokens(), 9);
    assert_eq!(stats.input_tokens, 3);
    assert_eq!(stats.output_tokens, 4);
    assert_eq!(stats.priced_tokens.input_tokens, 3);
    assert_eq!(stats.priced_tokens.output_tokens, 4);
}

// --- DateFilter ---

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

#[test]
fn date_filter_no_bounds() {
    let f = DateFilter::new(None, None);
    assert!(f.contains(d(2020, 1, 1)));
    assert!(f.contains(d(2099, 12, 31)));
}

#[test]
fn date_filter_since_only() {
    let f = DateFilter::new(Some(d(2025, 6, 1)), None);
    assert!(!f.contains(d(2025, 5, 31)));
    assert!(f.contains(d(2025, 6, 1))); // inclusive
    assert!(f.contains(d(2025, 6, 2)));
}

#[test]
fn date_filter_until_only() {
    let f = DateFilter::new(None, Some(d(2025, 6, 30)));
    assert!(f.contains(d(2025, 6, 29)));
    assert!(f.contains(d(2025, 6, 30))); // inclusive
    assert!(!f.contains(d(2025, 7, 1)));
}

#[test]
fn date_filter_both_bounds() {
    let f = DateFilter::new(Some(d(2025, 3, 1)), Some(d(2025, 3, 31)));
    assert!(!f.contains(d(2025, 2, 28)));
    assert!(f.contains(d(2025, 3, 1)));
    assert!(f.contains(d(2025, 3, 15)));
    assert!(f.contains(d(2025, 3, 31)));
    assert!(!f.contains(d(2025, 4, 1)));
}

#[test]
fn date_filter_single_day_range() {
    let f = DateFilter::new(Some(d(2025, 1, 15)), Some(d(2025, 1, 15)));
    assert!(!f.contains(d(2025, 1, 14)));
    assert!(f.contains(d(2025, 1, 15)));
    assert!(!f.contains(d(2025, 1, 16)));
}
