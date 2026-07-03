use comfy_table::{Cell, Color, Table};
use std::collections::HashMap;

use crate::cli::SortOrder;
use crate::core::{DayStats, Stats};
use crate::output::format::{
    NumberFormat, create_styled_table, format_compact, format_cost, format_number, header_cell,
    right_cell, styled_cell,
};
use crate::output::period::{Period, aggregate_day_stats_by_period};
use crate::output::pricing_meta;
use crate::pricing::{
    CostDisplayMode, CurrencyConverter, PricingDb, calculate_display_cost, model_cost_kind,
    sum_display_model_costs, sum_estimated_proxy_model_costs,
};

#[derive(Debug, Clone, Copy)]
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct TokenTableOptions<'a> {
    pub(crate) order: SortOrder,
    pub(crate) use_color: bool,
    pub(crate) compact: bool,
    pub(crate) show_cost: bool,
    pub(crate) number_format: NumberFormat,
    pub(crate) show_reasoning: bool,
    pub(crate) show_cache_creation: bool,
    pub(crate) currency: Option<&'a CurrencyConverter>,
    pub(crate) cost_mode: CostDisplayMode,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct PeriodSummaryFooter {
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
            println!("\n  {stats_text} | \x1b[36m{ms:.0}ms\x1b[0m\n");
        } else {
            println!("\n  {stats_text} | {ms:.0}ms\n");
        }
    } else {
        println!("\n  {stats_text}\n");
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

fn build_header(cfg: &PeriodConfig, breakdown: bool, opts: &TokenTableOptions<'_>) -> Vec<Cell> {
    let c = opts.use_color;
    if opts.compact {
        let mut h = vec![header_cell(cfg.label, c)];
        if cfg.show_calls {
            h.push(header_cell("Calls", c));
        }
        h.extend([
            header_cell("In", c),
            header_cell("Out", c),
            header_cell("Total", c),
        ]);
        if opts.show_cost {
            h.push(header_cell("Cost", c));
        }
        h
    } else if breakdown {
        let mut h = vec![header_cell(cfg.label, c), header_cell("Model", c)];
        if cfg.show_calls {
            h.push(header_cell("Calls", c));
        }
        h.extend([header_cell("Input", c), header_cell("Output", c)]);
        if opts.show_reasoning {
            h.push(header_cell("Reason", c));
        }
        if opts.show_cache_creation {
            h.push(header_cell("Cache Creation", c));
        }
        h.push(header_cell("Cache Read", c));
        if opts.show_cost {
            h.push(header_cell("Cost", c));
        }
        h
    } else {
        let mut h = vec![header_cell(cfg.label, c), header_cell("Models", c)];
        if cfg.show_calls {
            h.push(header_cell("Calls", c));
        }
        h.extend([header_cell("Input", c), header_cell("Output", c)]);
        if opts.show_reasoning {
            h.push(header_cell("Reason", c));
        }
        if opts.show_cache_creation {
            h.push(header_cell("Cache Creation", c));
        }
        h.extend([header_cell("Cache Read", c), header_cell("Total", c)]);
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
    opts: &TokenTableOptions<'_>,
    cost_color: Option<Color>,
    pricing_db: &PricingDb,
) -> f64 {
    let cost = sum_display_model_costs(&data.models, pricing_db, opts.cost_mode);
    let nf = opts.number_format;
    let mut row = vec![Cell::new(key)];
    if cfg.show_calls {
        row.push(right_cell(
            &format_compact(data.stats.count, nf),
            None,
            false,
        ));
    }
    row.extend([
        right_cell(&format_compact(data.stats.input_tokens, nf), None, false),
        right_cell(&format_compact(data.stats.output_tokens, nf), None, false),
        right_cell(&format_compact(data.stats.total_tokens(), nf), None, false),
    ]);
    if opts.show_cost {
        row.push(right_cell(
            &format_cost(cost, opts.currency),
            cost_color,
            false,
        ));
    }
    table.add_row(row);
    cost
}

fn add_breakdown_rows(
    table: &mut Table,
    key: &str,
    data: &DayStats,
    cfg: &PeriodConfig,
    opts: &TokenTableOptions<'_>,
    cost_color: Option<Color>,
    pricing_db: &PricingDb,
) -> f64 {
    let mut models: Vec<_> = data.models.keys().collect();
    models.sort();
    let nf = opts.number_format;
    let mut period_cost = 0.0;

    for (i, model) in models.iter().enumerate() {
        let stats = &data.models[*model];
        let cost = calculate_display_cost(stats, model, pricing_db, opts.cost_mode);
        period_cost += cost;

        let mut row = vec![Cell::new(if i == 0 { key } else { "" }), Cell::new(*model)];
        if cfg.show_calls {
            row.push(right_cell(&format_number(stats.count, nf), None, false));
        }
        row.extend([
            right_cell(&format_number(stats.input_tokens, nf), None, false),
            right_cell(&format_number(stats.output_tokens, nf), None, false),
        ]);
        if opts.show_reasoning {
            row.push(right_cell(
                &format_number(stats.reasoning_tokens, nf),
                None,
                false,
            ));
        }
        if opts.show_cache_creation {
            row.push(right_cell(
                &format_number(stats.cache_creation, nf),
                None,
                false,
            ));
        }
        row.push(right_cell(
            &format_number(stats.cache_read, nf),
            None,
            false,
        ));
        if opts.show_cost {
            row.push(right_cell(
                &format_cost(cost, opts.currency),
                cost_color,
                false,
            ));
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
    opts: &TokenTableOptions<'_>,
    cost_color: Option<Color>,
    pricing_db: &PricingDb,
) -> f64 {
    let mut models: Vec<_> = data.models.keys().collect();
    models.sort();
    let models_str = models
        .iter()
        .map(|s| s.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let cost = sum_display_model_costs(&data.models, pricing_db, opts.cost_mode);
    let nf = opts.number_format;

    let mut row = vec![Cell::new(key), Cell::new(&models_str)];
    if cfg.show_calls {
        row.push(right_cell(
            &format_number(data.stats.count, nf),
            None,
            false,
        ));
    }
    row.extend([
        right_cell(&format_number(data.stats.input_tokens, nf), None, false),
        right_cell(&format_number(data.stats.output_tokens, nf), None, false),
    ]);
    if opts.show_reasoning {
        row.push(right_cell(
            &format_number(data.stats.reasoning_tokens, nf),
            None,
            false,
        ));
    }
    if opts.show_cache_creation {
        row.push(right_cell(
            &format_number(data.stats.cache_creation, nf),
            None,
            false,
        ));
    }
    row.push(right_cell(
        &format_number(data.stats.cache_read, nf),
        None,
        false,
    ));
    row.push(right_cell(
        &format_number(data.stats.total_tokens(), nf),
        None,
        false,
    ));
    if opts.show_cost {
        row.push(right_cell(
            &format_cost(cost, opts.currency),
            cost_color,
            false,
        ));
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
    opts: &TokenTableOptions<'_>,
) {
    let cyan = if opts.use_color {
        Some(Color::Cyan)
    } else {
        None
    };
    let green = if opts.use_color {
        Some(Color::Green)
    } else {
        None
    };
    let nf = opts.number_format;

    if opts.compact {
        let mut row = vec![styled_cell("TOTAL", cyan, true)];
        if cfg.show_calls {
            row.push(right_cell(
                &format_compact(total_stats.count, nf),
                cyan,
                true,
            ));
        }
        row.extend([
            right_cell(&format_compact(total_stats.input_tokens, nf), cyan, true),
            right_cell(&format_compact(total_stats.output_tokens, nf), cyan, true),
            right_cell(&format_compact(total_stats.total_tokens(), nf), cyan, true),
        ]);
        if opts.show_cost {
            row.push(right_cell(
                &format_cost(total_cost, opts.currency),
                green,
                true,
            ));
        }
        table.add_row(row);
    } else {
        let mut row = vec![styled_cell("TOTAL", cyan, true), Cell::new("")];
        if cfg.show_calls {
            row.push(right_cell(
                &format_number(total_stats.count, nf),
                cyan,
                true,
            ));
        }
        row.extend([
            right_cell(&format_number(total_stats.input_tokens, nf), cyan, true),
            right_cell(&format_number(total_stats.output_tokens, nf), cyan, true),
        ]);
        if opts.show_reasoning {
            row.push(right_cell(
                &format_number(total_stats.reasoning_tokens, nf),
                cyan,
                true,
            ));
        }
        if opts.show_cache_creation {
            row.push(right_cell(
                &format_number(total_stats.cache_creation, nf),
                cyan,
                true,
            ));
        }
        row.push(right_cell(
            &format_number(total_stats.cache_read, nf),
            cyan,
            true,
        ));
        if !breakdown {
            row.push(right_cell(
                &format_number(total_stats.total_tokens(), nf),
                cyan,
                true,
            ));
        }
        if opts.show_cost {
            row.push(right_cell(
                &format_cost(total_cost, opts.currency),
                green,
                true,
            ));
        }
        table.add_row(row);
    }
}

pub(crate) fn print_period_table(
    day_stats: &HashMap<String, DayStats>,
    period: Period,
    breakdown: bool,
    summary: PeriodSummaryFooter,
    pricing_db: &PricingDb,
    options: TokenTableOptions<'_>,
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

    let cost_color = if options.use_color {
        Some(Color::Green)
    } else {
        None
    };
    let mut total_stats = Stats::default();
    let mut total_cost = 0.0;
    let mut estimated_proxy_cost = 0.0;
    let mut has_estimated_proxy = false;

    for key in &keys {
        let data = &stats_ref[*key];
        let row_estimated_cost = sum_estimated_proxy_model_costs(&data.models, pricing_db);
        if row_estimated_cost > 0.0 {
            has_estimated_proxy = true;
            estimated_proxy_cost += row_estimated_cost;
        }
        if model_cost_kind(&data.models).as_str() != "real" {
            has_estimated_proxy = true;
        }
        let cost = if options.compact {
            add_compact_rows(
                &mut table, key, data, &cfg, &options, cost_color, pricing_db,
            )
        } else if breakdown {
            add_breakdown_rows(
                &mut table, key, data, &cfg, &options, cost_color, pricing_db,
            )
        } else {
            add_standard_rows(
                &mut table, key, data, &cfg, &options, cost_color, pricing_db,
            )
        };
        total_cost += cost;
        total_stats.add(&data.stats);
    }

    add_total_row(
        &mut table,
        &total_stats,
        total_cost,
        &cfg,
        breakdown,
        &options,
    );

    println!("\n  {}\n", cfg.title);
    println!("{table}");
    if options.show_cost && has_estimated_proxy {
        match options.cost_mode {
            CostDisplayMode::RealOnly => println!(
                "\n  Estimated proxy cost excluded from Cost total: {}",
                format_cost(estimated_proxy_cost, options.currency)
            ),
            CostDisplayMode::Total => println!(
                "\n  Cost includes estimated proxy values: {}",
                format_cost(estimated_proxy_cost, options.currency)
            ),
        }
    }
    if options.show_cost
        && let Some(note) =
            pricing_meta::note_for_maps(stats_ref.values().map(|data| &data.models), pricing_db)
    {
        println!("\n  {note}");
    }
    print_summary_line(
        summary.valid,
        summary.skipped,
        options.number_format,
        summary.elapsed_ms,
        options.use_color,
    );
}

#[cfg(test)]
#[path = "table_tests.rs"]
mod table_tests;
