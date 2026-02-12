use comfy_table::{Cell, Color, Table};
use std::collections::HashMap;

use crate::cli::SortOrder;
use crate::core::{DayStats, Stats};
use crate::output::format::{
    NumberFormat, create_styled_table, format_compact, format_cost, format_number, header_cell,
    right_cell, styled_cell,
};
use crate::output::period::{Period, aggregate_day_stats_by_period};
use crate::pricing::{PricingDb, calculate_cost, sum_model_costs};

#[derive(Debug, Clone, Copy)]
pub(crate) struct TokenTableOptions {
    pub(crate) order: SortOrder,
    pub(crate) use_color: bool,
    pub(crate) compact: bool,
    pub(crate) show_cost: bool,
    pub(crate) number_format: NumberFormat,
    pub(crate) show_reasoning: bool,
    pub(crate) show_cache_creation: bool,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct SummaryOptions {
    pub(crate) skipped: i64,
    pub(crate) valid: i64,
    pub(crate) elapsed_ms: Option<f64>,
}

/// Print the summary line with optional timing
pub(crate) fn print_summary_line(
    valid: i64,
    skipped: i64,
    number_format: NumberFormat,
    elapsed_ms: Option<f64>,
    use_color: bool,
) {
    let stats_text = format!(
        "{} unique API calls ({} streaming entries deduplicated)",
        format_number(valid, number_format),
        format_number(skipped, number_format)
    );

    if let Some(ms) = elapsed_ms {
        if use_color {
            println!("\n  {} | \x1b[36m{:.0}ms\x1b[0m\n", stats_text, ms);
        } else {
            println!("\n  {} | {:.0}ms\n", stats_text, ms);
        }
    } else {
        println!("\n  {}\n", stats_text);
    }
}

fn sort_keys(keys: &mut Vec<&String>, order: SortOrder) {
    match order {
        SortOrder::Asc => keys.sort(),
        SortOrder::Desc => keys.sort_by(|a, b| b.cmp(a)),
    }
}

struct PeriodConfig {
    label: &'static str,
    title: &'static str,
    show_calls: bool,
}

fn period_config(period: Period) -> PeriodConfig {
    match period {
        Period::Day => PeriodConfig {
            label: "Date",
            title: "Token Usage",
            show_calls: true,
        },
        Period::Week => PeriodConfig {
            label: "Week",
            title: "Weekly Token Usage",
            show_calls: false,
        },
        Period::Month => PeriodConfig {
            label: "Month",
            title: "Monthly Token Usage",
            show_calls: false,
        },
    }
}

fn build_header(
    cfg: &PeriodConfig,
    breakdown: bool,
    opts: &TokenTableOptions,
) -> Vec<Cell> {
    let c = opts.use_color;
    if opts.compact {
        let mut h = vec![header_cell(cfg.label, c)];
        if cfg.show_calls {
            h.push(header_cell("Calls", c));
        }
        h.extend([header_cell("In", c), header_cell("Out", c), header_cell("Total", c)]);
        if opts.show_cost {
            h.push(header_cell("Cost", c));
        }
        h
    } else if breakdown {
        let mut h = vec![
            header_cell(cfg.label, c),
            header_cell("Model", c),
        ];
        if cfg.show_calls {
            h.push(header_cell("Calls", c));
        }
        h.extend([
            header_cell("Input", c),
            header_cell("Output", c),
        ]);
        if opts.show_reasoning {
            h.push(header_cell("Reason", c));
        }
        if opts.show_cache_creation {
            h.push(header_cell("Cache W", c));
        }
        h.push(header_cell("Cache R", c));
        if opts.show_cost {
            h.push(header_cell("Cost", c));
        }
        h
    } else {
        let mut h = vec![
            header_cell(cfg.label, c),
            header_cell("Models", c),
        ];
        if cfg.show_calls {
            h.push(header_cell("Calls", c));
        }
        h.extend([
            header_cell("Input", c),
            header_cell("Output", c),
        ]);
        if opts.show_reasoning {
            h.push(header_cell("Reason", c));
        }
        if opts.show_cache_creation {
            h.push(header_cell("Cache W", c));
        }
        h.extend([header_cell("Cache R", c), header_cell("Total", c)]);
        if opts.show_cost {
            h.push(header_cell("Cost", c));
        }
        h
    }
}

fn add_compact_rows(
    table: &mut Table,
    key: &str,
    data: &DayStats,
    cfg: &PeriodConfig,
    opts: &TokenTableOptions,
    cost_color: Option<Color>,
    pricing_db: &PricingDb,
) -> f64 {
    let cost = sum_model_costs(&data.models, pricing_db);
    let nf = opts.number_format;
    let mut row = vec![Cell::new(key)];
    if cfg.show_calls {
        row.push(right_cell(&format_compact(data.stats.count, nf), None, false));
    }
    row.extend([
        right_cell(&format_compact(data.stats.input_tokens, nf), None, false),
        right_cell(&format_compact(data.stats.output_tokens, nf), None, false),
        right_cell(&format_compact(data.stats.total_tokens(), nf), None, false),
    ]);
    if opts.show_cost {
        row.push(right_cell(&format_cost(cost), cost_color, false));
    }
    table.add_row(row);
    cost
}

fn add_breakdown_rows(
    table: &mut Table,
    key: &str,
    data: &DayStats,
    cfg: &PeriodConfig,
    opts: &TokenTableOptions,
    cost_color: Option<Color>,
    pricing_db: &PricingDb,
) -> f64 {
    let mut models: Vec<_> = data.models.keys().collect();
    models.sort();
    let nf = opts.number_format;
    let mut period_cost = 0.0;

    for (i, model) in models.iter().enumerate() {
        let stats = &data.models[*model];
        let cost = calculate_cost(stats, model, pricing_db);
        period_cost += cost;

        let mut row = vec![
            Cell::new(if i == 0 { key } else { "" }),
            Cell::new(*model),
        ];
        if cfg.show_calls {
            row.push(right_cell(&format_number(stats.count, nf), None, false));
        }
        row.extend([
            right_cell(&format_number(stats.input_tokens, nf), None, false),
            right_cell(&format_number(stats.output_tokens, nf), None, false),
        ]);
        if opts.show_reasoning {
            row.push(right_cell(&format_number(stats.reasoning_tokens, nf), None, false));
        }
        if opts.show_cache_creation {
            row.push(right_cell(&format_number(stats.cache_creation, nf), None, false));
        }
        row.push(right_cell(&format_number(stats.cache_read, nf), None, false));
        if opts.show_cost {
            row.push(right_cell(&format_cost(cost), cost_color, false));
        }
        table.add_row(row);
    }
    period_cost
}

fn add_standard_rows(
    table: &mut Table,
    key: &str,
    data: &DayStats,
    cfg: &PeriodConfig,
    opts: &TokenTableOptions,
    cost_color: Option<Color>,
    pricing_db: &PricingDb,
) -> f64 {
    let mut models: Vec<_> = data.models.keys().collect();
    models.sort();
    let models_str = models.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ");
    let cost = sum_model_costs(&data.models, pricing_db);
    let nf = opts.number_format;

    let mut row = vec![
        Cell::new(key),
        Cell::new(&models_str),
    ];
    if cfg.show_calls {
        row.push(right_cell(&format_number(data.stats.count, nf), None, false));
    }
    row.extend([
        right_cell(&format_number(data.stats.input_tokens, nf), None, false),
        right_cell(&format_number(data.stats.output_tokens, nf), None, false),
    ]);
    if opts.show_reasoning {
        row.push(right_cell(&format_number(data.stats.reasoning_tokens, nf), None, false));
    }
    if opts.show_cache_creation {
        row.push(right_cell(&format_number(data.stats.cache_creation, nf), None, false));
    }
    row.push(right_cell(&format_number(data.stats.cache_read, nf), None, false));
    row.push(right_cell(&format_number(data.stats.total_tokens(), nf), None, false));
    if opts.show_cost {
        row.push(right_cell(&format_cost(cost), cost_color, false));
    }
    table.add_row(row);
    cost
}

fn add_total_row(
    table: &mut Table,
    total_stats: &Stats,
    total_cost: f64,
    cfg: &PeriodConfig,
    breakdown: bool,
    opts: &TokenTableOptions,
) {
    let cyan = if opts.use_color { Some(Color::Cyan) } else { None };
    let green = if opts.use_color { Some(Color::Green) } else { None };
    let nf = opts.number_format;

    if opts.compact {
        let mut row = vec![styled_cell("TOTAL", cyan, true)];
        if cfg.show_calls {
            row.push(right_cell(&format_compact(total_stats.count, nf), cyan, true));
        }
        row.extend([
            right_cell(&format_compact(total_stats.input_tokens, nf), cyan, true),
            right_cell(&format_compact(total_stats.output_tokens, nf), cyan, true),
            right_cell(&format_compact(total_stats.total_tokens(), nf), cyan, true),
        ]);
        if opts.show_cost {
            row.push(right_cell(&format_cost(total_cost), green, true));
        }
        table.add_row(row);
    } else {
        let mut row = vec![
            styled_cell("TOTAL", cyan, true),
            Cell::new(""),
        ];
        if cfg.show_calls {
            row.push(right_cell(&format_number(total_stats.count, nf), cyan, true));
        }
        row.extend([
            right_cell(&format_number(total_stats.input_tokens, nf), cyan, true),
            right_cell(&format_number(total_stats.output_tokens, nf), cyan, true),
        ]);
        if opts.show_reasoning {
            row.push(right_cell(&format_number(total_stats.reasoning_tokens, nf), cyan, true));
        }
        if opts.show_cache_creation {
            row.push(right_cell(&format_number(total_stats.cache_creation, nf), cyan, true));
        }
        row.push(right_cell(&format_number(total_stats.cache_read, nf), cyan, true));
        if !breakdown {
            row.push(right_cell(&format_number(total_stats.total_tokens(), nf), cyan, true));
        }
        if opts.show_cost {
            row.push(right_cell(&format_cost(total_cost), green, true));
        }
        table.add_row(row);
    }
}

pub(crate) fn print_period_table(
    day_stats: &HashMap<String, DayStats>,
    period: Period,
    breakdown: bool,
    summary: SummaryOptions,
    pricing_db: &PricingDb,
    options: TokenTableOptions,
) {
    let cfg = period_config(period);
    let aggregated;
    let stats_ref = if period == Period::Day {
        day_stats
    } else {
        aggregated = aggregate_day_stats_by_period(day_stats, period);
        &aggregated
    };

    let mut keys: Vec<_> = stats_ref.keys().collect();
    sort_keys(&mut keys, options.order);

    let mut table = create_styled_table();
    table.set_header(build_header(&cfg, breakdown, &options));

    let cost_color = if options.use_color { Some(Color::Green) } else { None };
    let mut total_stats = Stats::default();
    let mut total_cost = 0.0;

    for key in &keys {
        let data = &stats_ref[*key];
        let cost = if options.compact {
            add_compact_rows(&mut table, key, data, &cfg, &options, cost_color, pricing_db)
        } else if breakdown {
            add_breakdown_rows(&mut table, key, data, &cfg, &options, cost_color, pricing_db)
        } else {
            add_standard_rows(&mut table, key, data, &cfg, &options, cost_color, pricing_db)
        };
        total_cost += cost;
        total_stats.add(&data.stats);
    }

    add_total_row(&mut table, &total_stats, total_cost, &cfg, breakdown, &options);

    println!("\n  {}\n", cfg.title);
    println!("{table}");
    print_summary_line(
        summary.valid,
        summary.skipped,
        options.number_format,
        summary.elapsed_ms,
        options.use_color,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::SortOrder;
    use crate::output::format::NumberFormat;
    use crate::output::period::Period;

    fn default_opts() -> TokenTableOptions {
        TokenTableOptions {
            order: SortOrder::Asc,
            use_color: false,
            compact: false,
            show_cost: false,
            number_format: NumberFormat::default(),
            show_reasoning: false,
            show_cache_creation: false,
        }
    }

    // --- build_header tests ---

    #[test]
    fn header_compact_daily_with_cost() {
        let cfg = period_config(Period::Day);
        let opts = TokenTableOptions { compact: true, show_cost: true, ..default_opts() };
        let h = build_header(&cfg, false, &opts);
        // Date, Calls, In, Out, Total, Cost
        assert_eq!(h.len(), 6);
    }

    #[test]
    fn header_compact_weekly_no_calls() {
        let cfg = period_config(Period::Week);
        let opts = TokenTableOptions { compact: true, show_cost: false, ..default_opts() };
        let h = build_header(&cfg, false, &opts);
        // Week, In, Out, Total (no Calls, no Cost)
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
        // Date, Model, Calls, Input, Output, Reason, Cache W, Cache R, Cost
        assert_eq!(h.len(), 9);
    }

    #[test]
    fn header_breakdown_monthly_minimal() {
        let cfg = period_config(Period::Month);
        let opts = default_opts();
        let h = build_header(&cfg, true, &opts);
        // Month, Model, Input, Output, Cache R (no Calls, no Reason, no Cache W, no Cost)
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
        // Date, Models, Calls, Input, Output, Reason, Cache W, Cache R, Total, Cost
        assert_eq!(h.len(), 10);
    }

    #[test]
    fn header_standard_weekly_minimal() {
        let cfg = period_config(Period::Week);
        let opts = default_opts();
        let h = build_header(&cfg, false, &opts);
        // Week, Models, Input, Output, Cache R, Total (no Calls, no Reason, no Cache W, no Cost)
        assert_eq!(h.len(), 6);
    }

    // --- sort_keys tests ---

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

    // --- period_config tests ---

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

    // --- add_*_rows tests ---

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
        let opts = TokenTableOptions { compact: true, show_cost: true, ..default_opts() };
        let data = make_day_stats();
        let cost = add_compact_rows(&mut table, "2026-02-12", &data, &cfg, &opts, None, &PricingDb::default());
        // With default pricing db, cost should be a finite number
        assert!(cost.is_finite());
    }

    #[test]
    fn add_breakdown_rows_returns_cost() {
        let mut table = create_styled_table();
        let cfg = period_config(Period::Day);
        let opts = TokenTableOptions { show_cost: true, show_reasoning: true, show_cache_creation: true, ..default_opts() };
        let data = make_day_stats();
        let cost = add_breakdown_rows(&mut table, "2026-02-12", &data, &cfg, &opts, None, &PricingDb::default());
        assert!(cost.is_finite());
    }

    #[test]
    fn add_standard_rows_returns_cost() {
        let mut table = create_styled_table();
        let cfg = period_config(Period::Day);
        let opts = TokenTableOptions { show_cost: true, ..default_opts() };
        let data = make_day_stats();
        let cost = add_standard_rows(&mut table, "2026-02-12", &data, &cfg, &opts, None, &PricingDb::default());
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
        data.models.insert("claude-haiku".to_string(), extra.clone());
        data.stats.add(&extra);
        // Should add one row per model (2 models)
        add_breakdown_rows(&mut table, "2026-02-12", &data, &cfg, &opts, None, &PricingDb::default());
        // Table should have 2 rows (one per model)
        assert_eq!(table.row_count(), 2);
    }

    #[test]
    fn add_total_row_compact_mode() {
        let mut table = create_styled_table();
        let cfg = period_config(Period::Day);
        let opts = TokenTableOptions { compact: true, show_cost: true, ..default_opts() };
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
    fn add_total_row_standard_mode() {
        let mut table = create_styled_table();
        let cfg = period_config(Period::Week);
        let opts = TokenTableOptions { show_cost: true, show_reasoning: true, ..default_opts() };
        let stats = Stats {
            input_tokens: 10000,
            output_tokens: 5000,
            reasoning_tokens: 1000,
            ..Default::default()
        };
        add_total_row(&mut table, &stats, 3.25, &cfg, false, &opts);
        assert_eq!(table.row_count(), 1);
    }
}
