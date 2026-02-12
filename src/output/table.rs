use comfy_table::{
    Cell, Color, ContentArrangement, Table, modifiers::UTF8_SOLID_INNER_BORDERS, presets::UTF8_FULL,
};
use std::collections::HashMap;

use crate::cli::SortOrder;
use crate::core::{DayStats, Stats};
use crate::output::format::{
    NumberFormat, format_compact, format_cost, format_number, header_cell,
    normalize_header_separator, right_cell, styled_cell,
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

fn print_period_table(
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

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .apply_modifier(UTF8_SOLID_INNER_BORDERS)
        .set_content_arrangement(ContentArrangement::Dynamic);
    normalize_header_separator(&mut table);
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

pub(crate) fn print_daily_table(
    day_stats: &HashMap<String, DayStats>,
    breakdown: bool,
    summary: SummaryOptions,
    pricing_db: &PricingDb,
    options: TokenTableOptions,
) {
    print_period_table(day_stats, Period::Day, breakdown, summary, pricing_db, options);
}

pub(crate) fn print_weekly_table(
    day_stats: &HashMap<String, DayStats>,
    breakdown: bool,
    summary: SummaryOptions,
    pricing_db: &PricingDb,
    options: TokenTableOptions,
) {
    print_period_table(day_stats, Period::Week, breakdown, summary, pricing_db, options);
}

pub(crate) fn print_monthly_table(
    day_stats: &HashMap<String, DayStats>,
    breakdown: bool,
    summary: SummaryOptions,
    pricing_db: &PricingDb,
    options: TokenTableOptions,
) {
    print_period_table(day_stats, Period::Month, breakdown, summary, pricing_db, options);
}
