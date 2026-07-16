use std::collections::HashMap;

use super::*;
use crate::core::Stats;

fn make_day_stats(models: &[(&str, i64)]) -> DayStats {
    let mut ds = DayStats::default();
    for &(model, tokens) in models {
        let stats = Stats {
            input_tokens: tokens,
            output_tokens: tokens / 2,
            count: 1,
            ..Default::default()
        };
        ds.add_stats(model.to_string(), &stats);
    }
    ds
}

#[test]
fn csv_escape_plain() {
    assert_eq!(csv_escape("hello"), "hello");
}

#[test]
fn csv_escape_comma() {
    assert_eq!(csv_escape("a,b"), "\"a,b\"");
}

#[test]
fn csv_escape_quotes() {
    assert_eq!(csv_escape("say \"hi\""), "\"say \"\"hi\"\"\"");
}

#[test]
fn period_csv_daily_no_cost() {
    let mut day_stats = HashMap::new();
    day_stats.insert(
        "2025-01-01".to_string(),
        make_day_stats(&[("sonnet", 1000)]),
    );

    let db = PricingDb::default();
    let csv = output_period_csv(
        &day_stats,
        Period::Day,
        &db,
        SortOrder::Asc,
        false,
        false,
        None,
    );

    let lines: Vec<&str> = csv.lines().collect();
    assert_eq!(
        lines[0],
        "date,input_tokens,output_tokens,reasoning_tokens,cache_creation_tokens,cache_read_tokens,cache_hit_rate,total_tokens"
    );
    assert!(lines[1].starts_with("2025-01-01,1000,500,"));
    assert!(!lines[0].contains("cost"));
}

#[test]
fn period_csv_daily_with_cost() {
    let mut day_stats = HashMap::new();
    day_stats.insert(
        "2025-01-01".to_string(),
        make_day_stats(&[("sonnet", 1000)]),
    );

    let db = PricingDb::default();
    let csv = output_period_csv(
        &day_stats,
        Period::Day,
        &db,
        SortOrder::Asc,
        false,
        true,
        None,
    );

    let lines: Vec<&str> = csv.lines().collect();
    assert!(lines[0].ends_with(",cost,pricing_source"));
    assert_eq!(lines.len(), 2);
    assert!(lines[1].ends_with(",fallback"));
}

#[test]
fn period_csv_converts_cost_when_currency_is_set() {
    let mut day_stats = HashMap::new();
    day_stats.insert(
        "2025-01-01".to_string(),
        make_day_stats(&[("sonnet", 1_000_000)]),
    );
    let converter = CurrencyConverter::from_rate_for_test("CNY", 7.0, "CNY ");

    let db = PricingDb::default();
    let csv = output_period_csv(
        &day_stats,
        Period::Day,
        &db,
        SortOrder::Asc,
        false,
        true,
        Some(&converter),
    );

    let lines: Vec<&str> = csv.lines().collect();
    assert_eq!(
        lines[1],
        "2025-01-01,1000000,500000,0,0,0,0.00,1500000,73.500000,fallback"
    );
}

#[test]
fn period_csv_sort_desc() {
    let mut day_stats = HashMap::new();
    day_stats.insert("2025-01-01".to_string(), make_day_stats(&[("sonnet", 100)]));
    day_stats.insert("2025-01-02".to_string(), make_day_stats(&[("sonnet", 200)]));

    let db = PricingDb::default();
    let csv = output_period_csv(
        &day_stats,
        Period::Day,
        &db,
        SortOrder::Desc,
        false,
        false,
        None,
    );

    let lines: Vec<&str> = csv.lines().collect();
    assert!(lines[1].starts_with("2025-01-02"));
    assert!(lines[2].starts_with("2025-01-01"));
}

#[test]
fn session_csv_structure() {
    let sessions = vec![SessionStats {
        session_key: "abc-123".to_string(),
        session_id: "abc-123".to_string(),
        project_path: "/home/user/project".to_string(),
        first_timestamp: "2025-01-01T00:00:00Z".to_string(),
        last_timestamp: "2025-01-01T01:00:00Z".to_string(),
        stats: Stats {
            input_tokens: 500,
            output_tokens: 200,
            count: 1,
            ..Default::default()
        },
        models: HashMap::new(),
    }];

    let db = PricingDb::default();
    let csv = output_session_csv(&sessions, &db, SortOrder::Asc, false, true, None);

    let lines: Vec<&str> = csv.lines().collect();
    assert_eq!(
        lines[0],
        "session_id,project_path,first_timestamp,last_timestamp,input_tokens,output_tokens,reasoning_tokens,cache_creation_tokens,cache_read_tokens,cache_hit_rate,total_tokens"
    );
    assert!(lines[1].starts_with("abc-123,/home/user/project,"));
}

#[test]
fn session_csv_includes_reasoning_and_cache_tokens() {
    let sessions = vec![SessionStats {
        session_key: "reasoning".to_string(),
        session_id: "reasoning".to_string(),
        project_path: String::new(),
        first_timestamp: "2025-01-01T00:00:00Z".to_string(),
        last_timestamp: "2025-01-01T01:00:00Z".to_string(),
        stats: Stats {
            input_tokens: 1000,
            output_tokens: 300,
            reasoning_tokens: 200,
            cache_creation: 50,
            cache_read: 100,
            count: 1,
            ..Default::default()
        },
        models: HashMap::new(),
    }];

    let db = PricingDb::default();
    let csv = output_session_csv(&sessions, &db, SortOrder::Asc, false, true, None);
    let lines: Vec<&str> = csv.lines().collect();

    assert_eq!(
        lines[1],
        "reasoning,,2025-01-01T00:00:00Z,2025-01-01T01:00:00Z,1000,300,200,50,100,8.70,1650"
    );
}

#[test]
fn project_csv_structure() {
    let projects = vec![ProjectStats {
        project_path: "/home/user/proj".to_string(),
        project_name: "proj".to_string(),
        session_count: 3,
        stats: Stats {
            input_tokens: 1000,
            output_tokens: 500,
            count: 5,
            ..Default::default()
        },
        models: HashMap::new(),
    }];

    let db = PricingDb::default();
    let csv = output_project_csv(&projects, &db, SortOrder::Asc, true, true, None);

    let lines: Vec<&str> = csv.lines().collect();
    assert!(lines[0].ends_with(",cost,pricing_source"));
    assert!(lines[1].starts_with("proj,"));
    assert!(lines[1].ends_with(",fallback"));
}

#[test]
fn block_csv_structure() {
    let blocks = vec![BlockStats {
        block_start: "2025-01-01 00:00".to_string(),
        block_end: "2025-01-01 05:00".to_string(),
        stats: Stats {
            input_tokens: 800,
            output_tokens: 300,
            cache_creation: 50,
            cache_read: 100,
            ..Default::default()
        },
        models: HashMap::new(),
    }];

    let db = PricingDb::default();
    let csv = output_block_csv(&blocks, &db, SortOrder::Asc, false, true, None);

    let lines: Vec<&str> = csv.lines().collect();
    assert_eq!(
        lines[0],
        "block_start,block_end,input_tokens,output_tokens,cache_creation_tokens,cache_read_tokens,cache_hit_rate,total_tokens"
    );
    assert_eq!(lines.len(), 2);
}

#[test]
fn empty_data_returns_header_only() {
    let db = PricingDb::default();
    let csv = output_period_csv(
        &HashMap::new(),
        Period::Day,
        &db,
        SortOrder::Asc,
        false,
        false,
        None,
    );
    let lines: Vec<&str> = csv.lines().collect();
    assert_eq!(lines.len(), 1);
}

#[test]
fn breakdown_csv_header_includes_model() {
    let mut day_stats = HashMap::new();
    day_stats.insert(
        "2025-01-01".to_string(),
        make_day_stats(&[("sonnet", 1000)]),
    );

    let db = PricingDb::default();
    let csv = output_period_csv(
        &day_stats,
        Period::Day,
        &db,
        SortOrder::Asc,
        true,
        false,
        None,
    );

    let lines: Vec<&str> = csv.lines().collect();
    assert_eq!(
        lines[0],
        "date,model,input_tokens,output_tokens,reasoning_tokens,cache_creation_tokens,cache_read_tokens,cache_hit_rate,total_tokens"
    );
}

#[test]
fn breakdown_csv_one_row_per_model() {
    let mut day_stats = HashMap::new();
    day_stats.insert(
        "2025-01-01".to_string(),
        make_day_stats(&[("sonnet", 1000), ("opus", 500)]),
    );

    let db = PricingDb::default();
    let csv = output_period_csv(
        &day_stats,
        Period::Day,
        &db,
        SortOrder::Asc,
        true,
        false,
        None,
    );

    let lines: Vec<&str> = csv.lines().collect();
    assert_eq!(lines.len(), 3);
    assert!(lines[1].starts_with("2025-01-01,opus,"));
    assert!(lines[2].starts_with("2025-01-01,sonnet,"));
}

#[test]
fn breakdown_csv_with_cost() {
    let mut day_stats = HashMap::new();
    day_stats.insert(
        "2025-01-01".to_string(),
        make_day_stats(&[("sonnet", 1000)]),
    );

    let db = PricingDb::default();
    let csv = output_period_csv(
        &day_stats,
        Period::Day,
        &db,
        SortOrder::Asc,
        true,
        true,
        None,
    );

    let lines: Vec<&str> = csv.lines().collect();
    assert!(lines[0].ends_with(",cost,pricing_source"));
    let fields: Vec<&str> = lines[1].split(',').collect();
    assert_eq!(fields.len(), 11);
    assert_eq!(fields[10], "fallback");
}

#[test]
fn breakdown_csv_empty_data() {
    let db = PricingDb::default();
    let csv = output_period_csv(
        &HashMap::new(),
        Period::Day,
        &db,
        SortOrder::Asc,
        true,
        false,
        None,
    );
    let lines: Vec<&str> = csv.lines().collect();
    assert_eq!(lines.len(), 1);
    assert!(lines[0].contains(",model,"));
}

#[test]
fn breakdown_csv_weekly_aggregation() {
    let mut day_stats = HashMap::new();
    day_stats.insert("2025-01-06".to_string(), make_day_stats(&[("sonnet", 100)]));
    day_stats.insert("2025-01-08".to_string(), make_day_stats(&[("sonnet", 200)]));

    let db = PricingDb::default();
    let csv = output_period_csv(
        &day_stats,
        Period::Week,
        &db,
        SortOrder::Asc,
        true,
        false,
        None,
    );

    let lines: Vec<&str> = csv.lines().collect();
    assert_eq!(lines.len(), 2);
    assert!(lines[0].starts_with("week,model,"));
    assert!(lines[1].starts_with("2025-01-06,sonnet,"));
    assert!(lines[1].contains(",300,"));
}

#[test]
fn csv_float_and_cost_handle_nan() {
    assert_eq!(csv_float(f64::NAN), "N/A");
    assert_eq!(csv_float(1.5), "1.500000");
    assert_eq!(csv_cost(f64::NAN, None), "N/A");
}
