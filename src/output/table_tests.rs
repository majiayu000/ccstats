use super::*;
use crate::cli::SortOrder;
use crate::output::format::NumberFormat;
use crate::output::period::Period;

fn default_opts() -> TokenTableOptions<'static> {
    TokenTableOptions {
        order: SortOrder::Asc,
        use_color: false,
        compact: false,
        show_cost: false,
        number_format: NumberFormat::default(),
        show_reasoning: false,
        show_cache_creation: false,
        currency: None,
    }
}

#[test]
fn header_compact_daily_with_cost() {
    let cfg = period_config(Period::Day);
    let opts = TokenTableOptions {
        compact: true,
        show_cost: true,
        ..default_opts()
    };
    let h = build_header(&cfg, false, &opts);
    assert_eq!(h.len(), 6);
}

#[test]
fn header_compact_weekly_no_calls() {
    let cfg = period_config(Period::Week);
    let opts = TokenTableOptions {
        compact: true,
        show_cost: false,
        ..default_opts()
    };
    let h = build_header(&cfg, false, &opts);
    assert_eq!(h.len(), 4);
}

#[test]
fn header_breakdown_daily_all_columns() {
    let cfg = period_config(Period::Day);
    let opts = TokenTableOptions {
        show_cost: true,
        show_reasoning: true,
        show_cache_creation: true,
        ..default_opts()
    };
    let h = build_header(&cfg, true, &opts);
    assert_eq!(h.len(), 9);
    assert_eq!(h[6].content(), "Cache Creation");
    assert_eq!(h[7].content(), "Cache Read");
}

#[test]
fn header_breakdown_monthly_minimal() {
    let cfg = period_config(Period::Month);
    let opts = default_opts();
    let h = build_header(&cfg, true, &opts);
    assert_eq!(h.len(), 5);
}

#[test]
fn header_standard_daily_all_columns() {
    let cfg = period_config(Period::Day);
    let opts = TokenTableOptions {
        show_cost: true,
        show_reasoning: true,
        show_cache_creation: true,
        ..default_opts()
    };
    let h = build_header(&cfg, false, &opts);
    assert_eq!(h.len(), 10);
    assert_eq!(h[6].content(), "Cache Creation");
    assert_eq!(h[7].content(), "Cache Read");
}

#[test]
fn header_standard_weekly_minimal() {
    let cfg = period_config(Period::Week);
    let opts = default_opts();
    let h = build_header(&cfg, false, &opts);
    assert_eq!(h.len(), 6);
}

#[test]
fn sort_keys_asc() {
    let a = "2026-02-01".to_string();
    let b = "2026-02-03".to_string();
    let c = "2026-02-02".to_string();
    let mut keys = vec![&a, &b, &c];
    sort_keys(&mut keys, SortOrder::Asc);
    assert_eq!(keys, vec![&a, &c, &b]);
}

#[test]
fn sort_keys_desc() {
    let a = "2026-02-01".to_string();
    let b = "2026-02-03".to_string();
    let c = "2026-02-02".to_string();
    let mut keys = vec![&a, &b, &c];
    sort_keys(&mut keys, SortOrder::Desc);
    assert_eq!(keys, vec![&b, &c, &a]);
}

#[test]
fn period_config_day() {
    let cfg = period_config(Period::Day);
    assert_eq!(cfg.label, "Date");
    assert!(cfg.show_calls);
}

#[test]
fn period_config_week() {
    let cfg = period_config(Period::Week);
    assert_eq!(cfg.label, "Week");
    assert!(!cfg.show_calls);
}

#[test]
fn period_config_month() {
    let cfg = period_config(Period::Month);
    assert_eq!(cfg.label, "Month");
    assert!(!cfg.show_calls);
}

fn make_day_stats() -> DayStats {
    let mut day = DayStats::default();
    let stats = Stats {
        input_tokens: 1000,
        output_tokens: 500,
        reasoning_tokens: 100,
        cache_creation: 50,
        cache_read: 200,
        count: 3,
        skipped_chunks: 0,
    };
    day.stats = stats.clone();
    day.models.insert("claude-sonnet".to_string(), stats);
    day
}

#[test]
fn add_compact_rows_returns_cost() {
    let mut table = create_styled_table();
    let cfg = period_config(Period::Day);
    let opts = TokenTableOptions {
        compact: true,
        show_cost: true,
        ..default_opts()
    };
    let data = make_day_stats();
    let cost = add_compact_rows(
        &mut table,
        "2026-02-12",
        &data,
        &cfg,
        &opts,
        None,
        &PricingDb::default(),
    );
    assert!(cost.is_finite());
}

#[test]
fn add_breakdown_rows_returns_cost() {
    let mut table = create_styled_table();
    let cfg = period_config(Period::Day);
    let opts = TokenTableOptions {
        show_cost: true,
        show_reasoning: true,
        show_cache_creation: true,
        ..default_opts()
    };
    let data = make_day_stats();
    let cost = add_breakdown_rows(
        &mut table,
        "2026-02-12",
        &data,
        &cfg,
        &opts,
        None,
        &PricingDb::default(),
    );
    assert!(cost.is_finite());
}

#[test]
fn add_standard_rows_returns_cost() {
    let mut table = create_styled_table();
    let cfg = period_config(Period::Day);
    let opts = TokenTableOptions {
        show_cost: true,
        ..default_opts()
    };
    let data = make_day_stats();
    let cost = add_standard_rows(
        &mut table,
        "2026-02-12",
        &data,
        &cfg,
        &opts,
        None,
        &PricingDb::default(),
    );
    assert!(cost.is_finite());
}

#[test]
fn add_breakdown_rows_multi_model() {
    let mut table = create_styled_table();
    let cfg = period_config(Period::Day);
    let opts = default_opts();
    let mut data = make_day_stats();
    let extra = Stats {
        input_tokens: 200,
        output_tokens: 100,
        count: 1,
        ..Default::default()
    };
    data.models
        .insert("claude-haiku".to_string(), extra.clone());
    data.stats.add(&extra);
    add_breakdown_rows(
        &mut table,
        "2026-02-12",
        &data,
        &cfg,
        &opts,
        None,
        &PricingDb::default(),
    );
    assert_eq!(table.row_count(), 2);
}

#[test]
fn add_total_row_compact_mode() {
    let mut table = create_styled_table();
    let cfg = period_config(Period::Day);
    let opts = TokenTableOptions {
        compact: true,
        show_cost: true,
        ..default_opts()
    };
    let stats = Stats {
        input_tokens: 5000,
        output_tokens: 2000,
        count: 10,
        ..Default::default()
    };
    add_total_row(&mut table, &stats, 1.50, &cfg, false, &opts);
    assert_eq!(table.row_count(), 1);
}

#[test]
fn add_total_row_compact_no_cost_no_calls() {
    let mut table = create_styled_table();
    let cfg = period_config(Period::Week);
    let opts = TokenTableOptions {
        compact: true,
        show_cost: false,
        ..default_opts()
    };
    let stats = Stats {
        input_tokens: 3000,
        output_tokens: 1000,
        ..Default::default()
    };
    add_total_row(&mut table, &stats, 0.0, &cfg, false, &opts);
    assert_eq!(table.row_count(), 1);
}

#[test]
fn add_compact_rows_weekly_no_calls() {
    let mut table = create_styled_table();
    let cfg = period_config(Period::Week);
    let opts = TokenTableOptions {
        compact: true,
        show_cost: false,
        ..default_opts()
    };
    let data = make_day_stats();
    let cost = add_compact_rows(
        &mut table,
        "2026-W07",
        &data,
        &cfg,
        &opts,
        None,
        &PricingDb::default(),
    );
    assert!(cost.is_finite());
    assert_eq!(table.row_count(), 1);
}

#[test]
fn add_total_row_breakdown_mode_omits_total_column() {
    let mut table = create_styled_table();
    let cfg = period_config(Period::Day);
    let opts = TokenTableOptions {
        show_cost: true,
        show_reasoning: true,
        ..default_opts()
    };
    let stats = Stats {
        input_tokens: 8000,
        output_tokens: 3000,
        reasoning_tokens: 500,
        ..Default::default()
    };
    add_total_row(&mut table, &stats, 2.00, &cfg, true, &opts);
    assert_eq!(table.row_count(), 1);
}

#[test]
fn add_total_row_standard_mode() {
    let mut table = create_styled_table();
    let cfg = period_config(Period::Week);
    let opts = TokenTableOptions {
        show_cost: true,
        show_reasoning: true,
        ..default_opts()
    };
    let stats = Stats {
        input_tokens: 10000,
        output_tokens: 5000,
        reasoning_tokens: 1000,
        ..Default::default()
    };
    add_total_row(&mut table, &stats, 3.25, &cfg, false, &opts);
    assert_eq!(table.row_count(), 1);
}
