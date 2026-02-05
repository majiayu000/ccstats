use chrono::{Datelike, NaiveDate};
use comfy_table::{modifiers::UTF8_SOLID_INNER_BORDERS, presets::UTF8_FULL, Cell, Color, ContentArrangement, Table};
use std::collections::HashMap;

use crate::cli::SortOrder;
use crate::core::{DayStats, Stats};
use crate::output::format::{
    format_compact, format_number, header_cell, normalize_header_separator, right_cell,
    styled_cell, NumberFormat,
};
use crate::pricing::{calculate_cost, PricingDb};

/// Print the summary line with optional timing
pub fn print_summary_line(valid: i64, skipped: i64, number_format: NumberFormat, elapsed_ms: Option<f64>, use_color: bool) {
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

fn sort_keys<'a>(keys: &mut Vec<&'a String>, order: SortOrder) {
    match order {
        SortOrder::Asc => keys.sort(),
        SortOrder::Desc => keys.sort_by(|a, b| b.cmp(a)),
    }
}


pub fn print_daily_table(
    day_stats: &HashMap<String, DayStats>,
    breakdown: bool,
    skipped: i64,
    valid: i64,
    pricing_db: &PricingDb,
    order: SortOrder,
    use_color: bool,
    compact: bool,
    show_cost: bool,
    number_format: NumberFormat,
    show_reasoning: bool,
    elapsed_ms: Option<f64>,
) {
    let mut dates: Vec<_> = day_stats.keys().collect();
    sort_keys(&mut dates, order);

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .apply_modifier(UTF8_SOLID_INNER_BORDERS)
        .set_content_arrangement(ContentArrangement::Dynamic);
    normalize_header_separator(&mut table);


    if compact {
        // Compact mode: Date, Calls, In, Out, Total, Cost
        let mut header = vec![
            header_cell("Date", use_color),
            header_cell("Calls", use_color),
            header_cell("In", use_color),
            header_cell("Out", use_color),
            header_cell("Total", use_color),
        ];
        if show_cost {
            header.push(header_cell("Cost", use_color));
        }
        table.set_header(header);
    } else if breakdown {
        let mut header = vec![
            header_cell("Date", use_color),
            header_cell("Model", use_color),
            header_cell("Calls", use_color),
            header_cell("Input", use_color),
            header_cell("Output", use_color),
        ];
        if show_reasoning {
            header.push(header_cell("Reason", use_color));
        }
        header.push(header_cell("Cache W", use_color));
        header.push(header_cell("Cache R", use_color));
        if show_cost {
            header.push(header_cell("Cost", use_color));
        }
        table.set_header(header);
    } else {
        let mut header = vec![
            header_cell("Date", use_color),
            header_cell("Models", use_color),
            header_cell("Calls", use_color),
            header_cell("Input", use_color),
            header_cell("Output", use_color),
        ];
        if show_reasoning {
            header.push(header_cell("Reason", use_color));
        }
        header.push(header_cell("Cache W", use_color));
        header.push(header_cell("Cache R", use_color));
        header.push(header_cell("Total", use_color));
        if show_cost {
            header.push(header_cell("Cost", use_color));
        }
        table.set_header(header);
    }

    let cost_color = if use_color { Some(Color::Green) } else { None };

    let mut total_stats = Stats::default();
    let mut total_cost = 0.0;

    for date in &dates {
        let day = &day_stats[*date];

        if compact {
            let mut day_cost = 0.0;
            for (model, stats) in &day.models {
                day_cost += calculate_cost(stats, model, pricing_db);
            }
            total_cost += day_cost;

            let mut row = vec![
                Cell::new(*date),
                right_cell(&format_compact(day.stats.count, number_format), None, false),
                right_cell(&format_compact(day.stats.input_tokens, number_format), None, false),
                right_cell(&format_compact(day.stats.output_tokens, number_format), None, false),
                right_cell(&format_compact(day.stats.total_tokens(), number_format), None, false),
            ];
            if show_cost {
                row.push(right_cell(&format!("${:.2}", day_cost), cost_color, false));
            }
            table.add_row(row);
        } else if breakdown {
            let mut models: Vec<_> = day.models.keys().collect();
            models.sort();

            for (i, model) in models.iter().enumerate() {
                let stats = &day.models[*model];
                let cost = calculate_cost(stats, model, pricing_db);
                total_cost += cost;

                let mut row = vec![
                    Cell::new(if i == 0 { *date } else { "" }),
                    Cell::new(*model),
                    right_cell(&format_number(stats.count, number_format), None, false),
                    right_cell(&format_number(stats.input_tokens, number_format), None, false),
                    right_cell(&format_number(stats.output_tokens, number_format), None, false),
                ];
                if show_reasoning {
                    row.push(right_cell(&format_number(stats.reasoning_tokens, number_format), None, false));
                }
                row.push(right_cell(&format_number(stats.cache_creation, number_format), None, false));
                row.push(right_cell(&format_number(stats.cache_read, number_format), None, false));
                if show_cost {
                    row.push(right_cell(&format!("${:.2}", cost), cost_color, false));
                }
                table.add_row(row);
            }
        } else {
            let mut models: Vec<_> = day.models.keys().collect();
            models.sort();
            let models_str = models
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", ");

            let mut day_cost = 0.0;
            for (model, stats) in &day.models {
                day_cost += calculate_cost(stats, model, pricing_db);
            }
            total_cost += day_cost;

            let mut row = vec![
                Cell::new(*date),
                Cell::new(&models_str),
                right_cell(&format_number(day.stats.count, number_format), None, false),
                right_cell(&format_number(day.stats.input_tokens, number_format), None, false),
                right_cell(&format_number(day.stats.output_tokens, number_format), None, false),
            ];
            if show_reasoning {
                row.push(right_cell(&format_number(day.stats.reasoning_tokens, number_format), None, false));
            }
            row.push(right_cell(&format_number(day.stats.cache_creation, number_format), None, false));
            row.push(right_cell(&format_number(day.stats.cache_read, number_format), None, false));
            row.push(right_cell(&format_number(day.stats.total_tokens(), number_format), None, false));
            if show_cost {
                row.push(right_cell(&format!("${:.2}", day_cost), cost_color, false));
            }
            table.add_row(row);
        }

        total_stats.add(&day.stats);
    }

    let cyan = if use_color { Some(Color::Cyan) } else { None };
    let green = if use_color { Some(Color::Green) } else { None };

    // Add total row
    if compact {
        let mut row = vec![
            styled_cell("TOTAL", cyan, true),
            right_cell(&format_compact(total_stats.count, number_format), cyan, true),
            right_cell(&format_compact(total_stats.input_tokens, number_format), cyan, true),
            right_cell(&format_compact(total_stats.output_tokens, number_format), cyan, true),
            right_cell(&format_compact(total_stats.total_tokens(), number_format), cyan, true),
        ];
        if show_cost {
            row.push(right_cell(&format!("${:.2}", total_cost), green, true));
        }
        table.add_row(row);
    } else if breakdown {
        let mut row = vec![
            styled_cell("TOTAL", cyan, true),
            Cell::new(""),
            right_cell(&format_number(total_stats.count, number_format), cyan, true),
            right_cell(&format_number(total_stats.input_tokens, number_format), cyan, true),
            right_cell(&format_number(total_stats.output_tokens, number_format), cyan, true),
        ];
        if show_reasoning {
            row.push(right_cell(&format_number(total_stats.reasoning_tokens, number_format), cyan, true));
        }
        row.push(right_cell(&format_number(total_stats.cache_creation, number_format), cyan, true));
        row.push(right_cell(&format_number(total_stats.cache_read, number_format), cyan, true));
        if show_cost {
            row.push(right_cell(&format!("${:.2}", total_cost), green, true));
        }
        table.add_row(row);
    } else {
        let mut row = vec![
            styled_cell("TOTAL", cyan, true),
            Cell::new(""),
            right_cell(&format_number(total_stats.count, number_format), cyan, true),
            right_cell(&format_number(total_stats.input_tokens, number_format), cyan, true),
            right_cell(&format_number(total_stats.output_tokens, number_format), cyan, true),
        ];
        if show_reasoning {
            row.push(right_cell(&format_number(total_stats.reasoning_tokens, number_format), cyan, true));
        }
        row.push(right_cell(&format_number(total_stats.cache_creation, number_format), cyan, true));
        row.push(right_cell(&format_number(total_stats.cache_read, number_format), cyan, true));
        row.push(right_cell(&format_number(total_stats.total_tokens(), number_format), cyan, true));
        if show_cost {
            row.push(right_cell(&format!("${:.2}", total_cost), green, true));
        }
        table.add_row(row);
    }

    println!("\n  Token Usage\n");
    println!("{table}");
    print_summary_line(valid, skipped, number_format, elapsed_ms, use_color);
}

pub fn print_monthly_table(
    day_stats: &HashMap<String, DayStats>,
    breakdown: bool,
    skipped: i64,
    valid: i64,
    pricing_db: &PricingDb,
    order: SortOrder,
    use_color: bool,
    compact: bool,
    show_cost: bool,
    number_format: NumberFormat,
    show_reasoning: bool,
    elapsed_ms: Option<f64>,
) {
    // Aggregate by month
    let mut month_stats: HashMap<String, DayStats> = HashMap::new();

    for (date, stats) in day_stats {
        let month = &date[..7]; // YYYY-MM
        let month_entry = month_stats.entry(month.to_string()).or_default();

        for (model, model_stats) in &stats.models {
            month_entry.stats.add(model_stats);
            month_entry
                .models
                .entry(model.clone())
                .or_default()
                .add(model_stats);
        }
    }

    let mut months: Vec<_> = month_stats.keys().collect();
    sort_keys(&mut months, order);

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .apply_modifier(UTF8_SOLID_INNER_BORDERS)
        .set_content_arrangement(ContentArrangement::Dynamic);
    normalize_header_separator(&mut table);


    if compact {
        let mut header = vec![
            header_cell("Month", use_color),
            header_cell("In", use_color),
            header_cell("Out", use_color),
            header_cell("Total", use_color),
        ];
        if show_cost {
            header.push(header_cell("Cost", use_color));
        }
        table.set_header(header);
    } else if breakdown {
        let mut header = vec![
            header_cell("Month", use_color),
            header_cell("Model", use_color),
            header_cell("Input", use_color),
            header_cell("Output", use_color),
        ];
        if show_reasoning {
            header.push(header_cell("Reason", use_color));
        }
        header.push(header_cell("Cache W", use_color));
        header.push(header_cell("Cache R", use_color));
        if show_cost {
            header.push(header_cell("Cost", use_color));
        }
        table.set_header(header);
    } else {
        let mut header = vec![
            header_cell("Month", use_color),
            header_cell("Models", use_color),
            header_cell("Input", use_color),
            header_cell("Output", use_color),
        ];
        if show_reasoning {
            header.push(header_cell("Reason", use_color));
        }
        header.push(header_cell("Cache W", use_color));
        header.push(header_cell("Cache R", use_color));
        header.push(header_cell("Total", use_color));
        if show_cost {
            header.push(header_cell("Cost", use_color));
        }
        table.set_header(header);
    }

    let cost_color = if use_color { Some(Color::Green) } else { None };

    let mut total_stats = Stats::default();
    let mut total_cost = 0.0;

    for month in &months {
        let month_data = &month_stats[*month];

        if compact {
            let mut month_cost = 0.0;
            for (model, stats) in &month_data.models {
                month_cost += calculate_cost(stats, model, pricing_db);
            }
            total_cost += month_cost;

            let mut row = vec![
                Cell::new(*month),
                right_cell(&format_compact(month_data.stats.input_tokens, number_format), None, false),
                right_cell(&format_compact(month_data.stats.output_tokens, number_format), None, false),
                right_cell(&format_compact(month_data.stats.total_tokens(), number_format), None, false),
            ];
            if show_cost {
                row.push(right_cell(&format!("${:.2}", month_cost), cost_color, false));
            }
            table.add_row(row);
        } else if breakdown {
            let mut models: Vec<_> = month_data.models.keys().collect();
            models.sort();

            for (i, model) in models.iter().enumerate() {
                let stats = &month_data.models[*model];
                let cost = calculate_cost(stats, model, pricing_db);
                total_cost += cost;

                let mut row = vec![
                    Cell::new(if i == 0 { *month } else { "" }),
                    Cell::new(*model),
                    right_cell(&format_number(stats.input_tokens, number_format), None, false),
                    right_cell(&format_number(stats.output_tokens, number_format), None, false),
                ];
                if show_reasoning {
                    row.push(right_cell(&format_number(stats.reasoning_tokens, number_format), None, false));
                }
                row.push(right_cell(&format_number(stats.cache_creation, number_format), None, false));
                row.push(right_cell(&format_number(stats.cache_read, number_format), None, false));
                if show_cost {
                    row.push(right_cell(&format!("${:.2}", cost), cost_color, false));
                }
                table.add_row(row);
            }
        } else {
            let models: Vec<_> = month_data.models.keys().map(|s| s.as_str()).collect();
            let models_str = models.join(", ");

            let mut month_cost = 0.0;
            for (model, stats) in &month_data.models {
                month_cost += calculate_cost(stats, model, pricing_db);
            }
            total_cost += month_cost;

            let mut row = vec![
                Cell::new(*month),
                Cell::new(&models_str),
                right_cell(&format_number(month_data.stats.input_tokens, number_format), None, false),
                right_cell(&format_number(month_data.stats.output_tokens, number_format), None, false),
            ];
            if show_reasoning {
                row.push(right_cell(&format_number(month_data.stats.reasoning_tokens, number_format), None, false));
            }
            row.push(right_cell(&format_number(month_data.stats.cache_creation, number_format), None, false));
            row.push(right_cell(&format_number(month_data.stats.cache_read, number_format), None, false));
            row.push(right_cell(&format_number(month_data.stats.total_tokens(), number_format), None, false));
            if show_cost {
                row.push(right_cell(&format!("${:.2}", month_cost), cost_color, false));
            }
            table.add_row(row);
        }

        total_stats.add(&month_data.stats);
    }

    let cyan = if use_color { Some(Color::Cyan) } else { None };
    let green = if use_color { Some(Color::Green) } else { None };

    // Add total row
    if compact {
        let mut row = vec![
            styled_cell("TOTAL", cyan, true),
            right_cell(&format_compact(total_stats.input_tokens, number_format), cyan, true),
            right_cell(&format_compact(total_stats.output_tokens, number_format), cyan, true),
            right_cell(&format_compact(total_stats.total_tokens(), number_format), cyan, true),
        ];
        if show_cost {
            row.push(right_cell(&format!("${:.2}", total_cost), green, true));
        }
        table.add_row(row);
    } else if breakdown {
        let mut row = vec![
            styled_cell("TOTAL", cyan, true),
            Cell::new(""),
            right_cell(&format_number(total_stats.input_tokens, number_format), cyan, true),
            right_cell(&format_number(total_stats.output_tokens, number_format), cyan, true),
        ];
        if show_reasoning {
            row.push(right_cell(&format_number(total_stats.reasoning_tokens, number_format), cyan, true));
        }
        row.push(right_cell(&format_number(total_stats.cache_creation, number_format), cyan, true));
        row.push(right_cell(&format_number(total_stats.cache_read, number_format), cyan, true));
        if show_cost {
            row.push(right_cell(&format!("${:.2}", total_cost), green, true));
        }
        table.add_row(row);
    } else {
        let mut row = vec![
            styled_cell("TOTAL", cyan, true),
            Cell::new(""),
            right_cell(&format_number(total_stats.input_tokens, number_format), cyan, true),
            right_cell(&format_number(total_stats.output_tokens, number_format), cyan, true),
        ];
        if show_reasoning {
            row.push(right_cell(&format_number(total_stats.reasoning_tokens, number_format), cyan, true));
        }
        row.push(right_cell(&format_number(total_stats.cache_creation, number_format), cyan, true));
        row.push(right_cell(&format_number(total_stats.cache_read, number_format), cyan, true));
        row.push(right_cell(&format_number(total_stats.total_tokens(), number_format), cyan, true));
        if show_cost {
            row.push(right_cell(&format!("${:.2}", total_cost), green, true));
        }
        table.add_row(row);
    }

    println!("\n  Monthly Token Usage\n");
    println!("{table}");
    print_summary_line(valid, skipped, number_format, elapsed_ms, use_color);
}

/// Get the Monday of the week for a given date (ISO week)
fn get_week_start(date_str: &str) -> String {
    if let Ok(date) = NaiveDate::parse_from_str(date_str, "%Y-%m-%d") {
        // Get the weekday (Mon=0, Sun=6 in chrono)
        let weekday = date.weekday().num_days_from_monday();
        let monday = date - chrono::Duration::days(weekday as i64);
        monday.format("%Y-%m-%d").to_string()
    } else {
        date_str.to_string()
    }
}

pub fn print_weekly_table(
    day_stats: &HashMap<String, DayStats>,
    breakdown: bool,
    skipped: i64,
    valid: i64,
    pricing_db: &PricingDb,
    order: SortOrder,
    use_color: bool,
    compact: bool,
    show_cost: bool,
    number_format: NumberFormat,
    show_reasoning: bool,
    elapsed_ms: Option<f64>,
) {
    // Aggregate by week (Monday start)
    let mut week_stats: HashMap<String, DayStats> = HashMap::new();

    for (date, stats) in day_stats {
        let week_start = get_week_start(date);
        let week_entry = week_stats.entry(week_start).or_default();

        for (model, model_stats) in &stats.models {
            week_entry.stats.add(model_stats);
            week_entry
                .models
                .entry(model.clone())
                .or_default()
                .add(model_stats);
        }
    }

    let mut weeks: Vec<_> = week_stats.keys().collect();
    sort_keys(&mut weeks, order);

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .apply_modifier(UTF8_SOLID_INNER_BORDERS)
        .set_content_arrangement(ContentArrangement::Dynamic);
    normalize_header_separator(&mut table);


    if compact {
        let mut header = vec![
            header_cell("Week", use_color),
            header_cell("In", use_color),
            header_cell("Out", use_color),
            header_cell("Total", use_color),
        ];
        if show_cost {
            header.push(header_cell("Cost", use_color));
        }
        table.set_header(header);
    } else if breakdown {
        let mut header = vec![
            header_cell("Week", use_color),
            header_cell("Model", use_color),
            header_cell("Input", use_color),
            header_cell("Output", use_color),
        ];
        if show_reasoning {
            header.push(header_cell("Reason", use_color));
        }
        header.push(header_cell("Cache W", use_color));
        header.push(header_cell("Cache R", use_color));
        if show_cost {
            header.push(header_cell("Cost", use_color));
        }
        table.set_header(header);
    } else {
        let mut header = vec![
            header_cell("Week", use_color),
            header_cell("Models", use_color),
            header_cell("Input", use_color),
            header_cell("Output", use_color),
        ];
        if show_reasoning {
            header.push(header_cell("Reason", use_color));
        }
        header.push(header_cell("Cache W", use_color));
        header.push(header_cell("Cache R", use_color));
        header.push(header_cell("Total", use_color));
        if show_cost {
            header.push(header_cell("Cost", use_color));
        }
        table.set_header(header);
    }

    let cost_color = if use_color { Some(Color::Green) } else { None };

    let mut total_stats = Stats::default();
    let mut total_cost = 0.0;

    for week in &weeks {
        let week_data = &week_stats[*week];

        if compact {
            let mut week_cost = 0.0;
            for (model, stats) in &week_data.models {
                week_cost += calculate_cost(stats, model, pricing_db);
            }
            total_cost += week_cost;

            let mut row = vec![
                Cell::new(*week),
                right_cell(&format_compact(week_data.stats.input_tokens, number_format), None, false),
                right_cell(&format_compact(week_data.stats.output_tokens, number_format), None, false),
                right_cell(&format_compact(week_data.stats.total_tokens(), number_format), None, false),
            ];
            if show_cost {
                row.push(right_cell(&format!("${:.2}", week_cost), cost_color, false));
            }
            table.add_row(row);
        } else if breakdown {
            let mut models: Vec<_> = week_data.models.keys().collect();
            models.sort();

            for (i, model) in models.iter().enumerate() {
                let stats = &week_data.models[*model];
                let cost = calculate_cost(stats, model, pricing_db);
                total_cost += cost;

                let mut row = vec![
                    Cell::new(if i == 0 { *week } else { "" }),
                    Cell::new(*model),
                    right_cell(&format_number(stats.input_tokens, number_format), None, false),
                    right_cell(&format_number(stats.output_tokens, number_format), None, false),
                ];
                if show_reasoning {
                    row.push(right_cell(&format_number(stats.reasoning_tokens, number_format), None, false));
                }
                row.push(right_cell(&format_number(stats.cache_creation, number_format), None, false));
                row.push(right_cell(&format_number(stats.cache_read, number_format), None, false));
                if show_cost {
                    row.push(right_cell(&format!("${:.2}", cost), cost_color, false));
                }
                table.add_row(row);
            }
        } else {
            let models: Vec<_> = week_data.models.keys().map(|s| s.as_str()).collect();
            let models_str = models.join(", ");

            let mut week_cost = 0.0;
            for (model, stats) in &week_data.models {
                week_cost += calculate_cost(stats, model, pricing_db);
            }
            total_cost += week_cost;

            let mut row = vec![
                Cell::new(*week),
                Cell::new(&models_str),
                right_cell(&format_number(week_data.stats.input_tokens, number_format), None, false),
                right_cell(&format_number(week_data.stats.output_tokens, number_format), None, false),
            ];
            if show_reasoning {
                row.push(right_cell(&format_number(week_data.stats.reasoning_tokens, number_format), None, false));
            }
            row.push(right_cell(&format_number(week_data.stats.cache_creation, number_format), None, false));
            row.push(right_cell(&format_number(week_data.stats.cache_read, number_format), None, false));
            row.push(right_cell(&format_number(week_data.stats.total_tokens(), number_format), None, false));
            if show_cost {
                row.push(right_cell(&format!("${:.2}", week_cost), cost_color, false));
            }
            table.add_row(row);
        }

        total_stats.add(&week_data.stats);
    }

    let cyan = if use_color { Some(Color::Cyan) } else { None };
    let green = if use_color { Some(Color::Green) } else { None };

    // Add total row
    if compact {
        let mut row = vec![
            styled_cell("TOTAL", cyan, true),
            right_cell(&format_compact(total_stats.input_tokens, number_format), cyan, true),
            right_cell(&format_compact(total_stats.output_tokens, number_format), cyan, true),
            right_cell(&format_compact(total_stats.total_tokens(), number_format), cyan, true),
        ];
        if show_cost {
            row.push(right_cell(&format!("${:.2}", total_cost), green, true));
        }
        table.add_row(row);
    } else if breakdown {
        let mut row = vec![
            styled_cell("TOTAL", cyan, true),
            Cell::new(""),
            right_cell(&format_number(total_stats.input_tokens, number_format), cyan, true),
            right_cell(&format_number(total_stats.output_tokens, number_format), cyan, true),
        ];
        if show_reasoning {
            row.push(right_cell(&format_number(total_stats.reasoning_tokens, number_format), cyan, true));
        }
        row.push(right_cell(&format_number(total_stats.cache_creation, number_format), cyan, true));
        row.push(right_cell(&format_number(total_stats.cache_read, number_format), cyan, true));
        if show_cost {
            row.push(right_cell(&format!("${:.2}", total_cost), green, true));
        }
        table.add_row(row);
    } else {
        let mut row = vec![
            styled_cell("TOTAL", cyan, true),
            Cell::new(""),
            right_cell(&format_number(total_stats.input_tokens, number_format), cyan, true),
            right_cell(&format_number(total_stats.output_tokens, number_format), cyan, true),
        ];
        if show_reasoning {
            row.push(right_cell(&format_number(total_stats.reasoning_tokens, number_format), cyan, true));
        }
        row.push(right_cell(&format_number(total_stats.cache_creation, number_format), cyan, true));
        row.push(right_cell(&format_number(total_stats.cache_read, number_format), cyan, true));
        row.push(right_cell(&format_number(total_stats.total_tokens(), number_format), cyan, true));
        if show_cost {
            row.push(right_cell(&format!("${:.2}", total_cost), green, true));
        }
        table.add_row(row);
    }

    println!("\n  Weekly Token Usage\n");
    println!("{table}");
    print_summary_line(valid, skipped, number_format, elapsed_ms, use_color);
}
