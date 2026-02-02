use chrono::{Datelike, NaiveDate};
use comfy_table::{presets::UTF8_FULL, Attribute, Cell, Color, ContentArrangement, Table};
use std::collections::HashMap;

use crate::cli::SortOrder;
use crate::data::{DayStats, Stats};
use crate::pricing::{calculate_cost, PricingDb};

pub fn format_number(n: i64) -> String {
    let s = n.to_string();
    let mut result = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }
    result.chars().rev().collect()
}

/// Format number in compact form (K, M, B suffixes)
fn format_compact(n: i64) -> String {
    if n >= 1_000_000_000 {
        format!("{:.1}B", n as f64 / 1_000_000_000.0)
    } else if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

fn sort_keys<'a>(keys: &mut Vec<&'a String>, order: SortOrder) {
    match order {
        SortOrder::Asc => keys.sort(),
        SortOrder::Desc => keys.sort_by(|a, b| b.cmp(a)),
    }
}

fn styled_cell(text: &str, color: Option<Color>, bold: bool) -> Cell {
    let mut cell = Cell::new(text);
    if let Some(c) = color {
        cell = cell.fg(c);
    }
    if bold {
        cell = cell.add_attribute(Attribute::Bold);
    }
    cell
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
) {
    let mut dates: Vec<_> = day_stats.keys().collect();
    sort_keys(&mut dates, order);

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic);

    if compact {
        // Compact mode: Date, In, Out, Total, Cost
        table.set_header(vec![
            Cell::new("Date").add_attribute(Attribute::Bold),
            Cell::new("In").add_attribute(Attribute::Bold),
            Cell::new("Out").add_attribute(Attribute::Bold),
            Cell::new("Total").add_attribute(Attribute::Bold),
            Cell::new("Cost").add_attribute(Attribute::Bold),
        ]);
    } else if breakdown {
        table.set_header(vec![
            Cell::new("Date").add_attribute(Attribute::Bold),
            Cell::new("Model").add_attribute(Attribute::Bold),
            Cell::new("Input").add_attribute(Attribute::Bold),
            Cell::new("Output").add_attribute(Attribute::Bold),
            Cell::new("Cache W").add_attribute(Attribute::Bold),
            Cell::new("Cache R").add_attribute(Attribute::Bold),
            Cell::new("Cost").add_attribute(Attribute::Bold),
        ]);
    } else {
        table.set_header(vec![
            Cell::new("Date").add_attribute(Attribute::Bold),
            Cell::new("Models").add_attribute(Attribute::Bold),
            Cell::new("Input").add_attribute(Attribute::Bold),
            Cell::new("Output").add_attribute(Attribute::Bold),
            Cell::new("Cache W").add_attribute(Attribute::Bold),
            Cell::new("Cache R").add_attribute(Attribute::Bold),
            Cell::new("Total").add_attribute(Attribute::Bold),
            Cell::new("Cost").add_attribute(Attribute::Bold),
        ]);
    }

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

            table.add_row(vec![
                Cell::new(*date),
                Cell::new(format_compact(day.stats.input_tokens)),
                Cell::new(format_compact(day.stats.output_tokens)),
                Cell::new(format_compact(day.stats.total_tokens())),
                Cell::new(format!("${:.2}", day_cost)),
            ]);
        } else if breakdown {
            let mut models: Vec<_> = day.models.keys().collect();
            models.sort();

            for (i, model) in models.iter().enumerate() {
                let stats = &day.models[*model];
                let cost = calculate_cost(stats, model, pricing_db);
                total_cost += cost;

                table.add_row(vec![
                    Cell::new(if i == 0 { *date } else { "" }),
                    Cell::new(*model),
                    Cell::new(format_number(stats.input_tokens)),
                    Cell::new(format_number(stats.output_tokens)),
                    Cell::new(format_number(stats.cache_creation)),
                    Cell::new(format_number(stats.cache_read)),
                    Cell::new(format!("${:.2}", cost)),
                ]);
            }
        } else {
            let models: Vec<_> = day.models.keys().map(|s| s.as_str()).collect();
            let models_str = models.join(", ");

            let mut day_cost = 0.0;
            for (model, stats) in &day.models {
                day_cost += calculate_cost(stats, model, pricing_db);
            }
            total_cost += day_cost;

            table.add_row(vec![
                Cell::new(*date),
                Cell::new(&models_str),
                Cell::new(format_number(day.stats.input_tokens)),
                Cell::new(format_number(day.stats.output_tokens)),
                Cell::new(format_number(day.stats.cache_creation)),
                Cell::new(format_number(day.stats.cache_read)),
                Cell::new(format_number(day.stats.total_tokens())),
                Cell::new(format!("${:.2}", day_cost)),
            ]);
        }

        total_stats.add(&day.stats);
    }

    let cyan = if use_color { Some(Color::Cyan) } else { None };
    let green = if use_color { Some(Color::Green) } else { None };

    // Add total row
    if compact {
        table.add_row(vec![
            styled_cell("TOTAL", cyan, true),
            styled_cell(&format_compact(total_stats.input_tokens), cyan, false),
            styled_cell(&format_compact(total_stats.output_tokens), cyan, false),
            styled_cell(&format_compact(total_stats.total_tokens()), cyan, false),
            styled_cell(&format!("${:.2}", total_cost), green, true),
        ]);
    } else if breakdown {
        table.add_row(vec![
            styled_cell("TOTAL", cyan, true),
            Cell::new(""),
            styled_cell(&format_number(total_stats.input_tokens), cyan, false),
            styled_cell(&format_number(total_stats.output_tokens), cyan, false),
            styled_cell(&format_number(total_stats.cache_creation), cyan, false),
            styled_cell(&format_number(total_stats.cache_read), cyan, false),
            styled_cell(&format!("${:.2}", total_cost), green, true),
        ]);
    } else {
        table.add_row(vec![
            styled_cell("TOTAL", cyan, true),
            Cell::new(""),
            styled_cell(&format_number(total_stats.input_tokens), cyan, false),
            styled_cell(&format_number(total_stats.output_tokens), cyan, false),
            styled_cell(&format_number(total_stats.cache_creation), cyan, false),
            styled_cell(&format_number(total_stats.cache_read), cyan, false),
            styled_cell(&format_number(total_stats.total_tokens()), cyan, false),
            styled_cell(&format!("${:.2}", total_cost), green, true),
        ]);
    }

    println!("\n  Claude Code Token Usage\n");
    println!("{table}");
    println!(
        "\n  {} unique API calls ({} streaming entries deduplicated)\n",
        format_number(valid),
        format_number(skipped)
    );
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
        .set_content_arrangement(ContentArrangement::Dynamic);

    if compact {
        table.set_header(vec![
            Cell::new("Month").add_attribute(Attribute::Bold),
            Cell::new("In").add_attribute(Attribute::Bold),
            Cell::new("Out").add_attribute(Attribute::Bold),
            Cell::new("Total").add_attribute(Attribute::Bold),
            Cell::new("Cost").add_attribute(Attribute::Bold),
        ]);
    } else if breakdown {
        table.set_header(vec![
            Cell::new("Month").add_attribute(Attribute::Bold),
            Cell::new("Model").add_attribute(Attribute::Bold),
            Cell::new("Input").add_attribute(Attribute::Bold),
            Cell::new("Output").add_attribute(Attribute::Bold),
            Cell::new("Cache W").add_attribute(Attribute::Bold),
            Cell::new("Cache R").add_attribute(Attribute::Bold),
            Cell::new("Cost").add_attribute(Attribute::Bold),
        ]);
    } else {
        table.set_header(vec![
            Cell::new("Month").add_attribute(Attribute::Bold),
            Cell::new("Models").add_attribute(Attribute::Bold),
            Cell::new("Input").add_attribute(Attribute::Bold),
            Cell::new("Output").add_attribute(Attribute::Bold),
            Cell::new("Cache W").add_attribute(Attribute::Bold),
            Cell::new("Cache R").add_attribute(Attribute::Bold),
            Cell::new("Total").add_attribute(Attribute::Bold),
            Cell::new("Cost").add_attribute(Attribute::Bold),
        ]);
    }

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

            table.add_row(vec![
                Cell::new(*month),
                Cell::new(format_compact(month_data.stats.input_tokens)),
                Cell::new(format_compact(month_data.stats.output_tokens)),
                Cell::new(format_compact(month_data.stats.total_tokens())),
                Cell::new(format!("${:.2}", month_cost)),
            ]);
        } else if breakdown {
            let mut models: Vec<_> = month_data.models.keys().collect();
            models.sort();

            for (i, model) in models.iter().enumerate() {
                let stats = &month_data.models[*model];
                let cost = calculate_cost(stats, model, pricing_db);
                total_cost += cost;

                table.add_row(vec![
                    Cell::new(if i == 0 { *month } else { "" }),
                    Cell::new(*model),
                    Cell::new(format_number(stats.input_tokens)),
                    Cell::new(format_number(stats.output_tokens)),
                    Cell::new(format_number(stats.cache_creation)),
                    Cell::new(format_number(stats.cache_read)),
                    Cell::new(format!("${:.2}", cost)),
                ]);
            }
        } else {
            let models: Vec<_> = month_data.models.keys().map(|s| s.as_str()).collect();
            let models_str = models.join(", ");

            let mut month_cost = 0.0;
            for (model, stats) in &month_data.models {
                month_cost += calculate_cost(stats, model, pricing_db);
            }
            total_cost += month_cost;

            table.add_row(vec![
                Cell::new(*month),
                Cell::new(&models_str),
                Cell::new(format_number(month_data.stats.input_tokens)),
                Cell::new(format_number(month_data.stats.output_tokens)),
                Cell::new(format_number(month_data.stats.cache_creation)),
                Cell::new(format_number(month_data.stats.cache_read)),
                Cell::new(format_number(month_data.stats.total_tokens())),
                Cell::new(format!("${:.2}", month_cost)),
            ]);
        }

        total_stats.add(&month_data.stats);
    }

    let cyan = if use_color { Some(Color::Cyan) } else { None };
    let green = if use_color { Some(Color::Green) } else { None };

    // Add total row
    if compact {
        table.add_row(vec![
            styled_cell("TOTAL", cyan, true),
            styled_cell(&format_compact(total_stats.input_tokens), cyan, false),
            styled_cell(&format_compact(total_stats.output_tokens), cyan, false),
            styled_cell(&format_compact(total_stats.total_tokens()), cyan, false),
            styled_cell(&format!("${:.2}", total_cost), green, true),
        ]);
    } else if breakdown {
        table.add_row(vec![
            styled_cell("TOTAL", cyan, true),
            Cell::new(""),
            styled_cell(&format_number(total_stats.input_tokens), cyan, false),
            styled_cell(&format_number(total_stats.output_tokens), cyan, false),
            styled_cell(&format_number(total_stats.cache_creation), cyan, false),
            styled_cell(&format_number(total_stats.cache_read), cyan, false),
            styled_cell(&format!("${:.2}", total_cost), green, true),
        ]);
    } else {
        table.add_row(vec![
            styled_cell("TOTAL", cyan, true),
            Cell::new(""),
            styled_cell(&format_number(total_stats.input_tokens), cyan, false),
            styled_cell(&format_number(total_stats.output_tokens), cyan, false),
            styled_cell(&format_number(total_stats.cache_creation), cyan, false),
            styled_cell(&format_number(total_stats.cache_read), cyan, false),
            styled_cell(&format_number(total_stats.total_tokens()), cyan, false),
            styled_cell(&format!("${:.2}", total_cost), green, true),
        ]);
    }

    println!("\n  Claude Code Monthly Token Usage\n");
    println!("{table}");
    println!(
        "\n  {} unique API calls ({} streaming entries deduplicated)\n",
        format_number(valid),
        format_number(skipped)
    );
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
        .set_content_arrangement(ContentArrangement::Dynamic);

    if compact {
        table.set_header(vec![
            Cell::new("Week").add_attribute(Attribute::Bold),
            Cell::new("In").add_attribute(Attribute::Bold),
            Cell::new("Out").add_attribute(Attribute::Bold),
            Cell::new("Total").add_attribute(Attribute::Bold),
            Cell::new("Cost").add_attribute(Attribute::Bold),
        ]);
    } else if breakdown {
        table.set_header(vec![
            Cell::new("Week").add_attribute(Attribute::Bold),
            Cell::new("Model").add_attribute(Attribute::Bold),
            Cell::new("Input").add_attribute(Attribute::Bold),
            Cell::new("Output").add_attribute(Attribute::Bold),
            Cell::new("Cache W").add_attribute(Attribute::Bold),
            Cell::new("Cache R").add_attribute(Attribute::Bold),
            Cell::new("Cost").add_attribute(Attribute::Bold),
        ]);
    } else {
        table.set_header(vec![
            Cell::new("Week").add_attribute(Attribute::Bold),
            Cell::new("Models").add_attribute(Attribute::Bold),
            Cell::new("Input").add_attribute(Attribute::Bold),
            Cell::new("Output").add_attribute(Attribute::Bold),
            Cell::new("Cache W").add_attribute(Attribute::Bold),
            Cell::new("Cache R").add_attribute(Attribute::Bold),
            Cell::new("Total").add_attribute(Attribute::Bold),
            Cell::new("Cost").add_attribute(Attribute::Bold),
        ]);
    }

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

            table.add_row(vec![
                Cell::new(*week),
                Cell::new(format_compact(week_data.stats.input_tokens)),
                Cell::new(format_compact(week_data.stats.output_tokens)),
                Cell::new(format_compact(week_data.stats.total_tokens())),
                Cell::new(format!("${:.2}", week_cost)),
            ]);
        } else if breakdown {
            let mut models: Vec<_> = week_data.models.keys().collect();
            models.sort();

            for (i, model) in models.iter().enumerate() {
                let stats = &week_data.models[*model];
                let cost = calculate_cost(stats, model, pricing_db);
                total_cost += cost;

                table.add_row(vec![
                    Cell::new(if i == 0 { *week } else { "" }),
                    Cell::new(*model),
                    Cell::new(format_number(stats.input_tokens)),
                    Cell::new(format_number(stats.output_tokens)),
                    Cell::new(format_number(stats.cache_creation)),
                    Cell::new(format_number(stats.cache_read)),
                    Cell::new(format!("${:.2}", cost)),
                ]);
            }
        } else {
            let models: Vec<_> = week_data.models.keys().map(|s| s.as_str()).collect();
            let models_str = models.join(", ");

            let mut week_cost = 0.0;
            for (model, stats) in &week_data.models {
                week_cost += calculate_cost(stats, model, pricing_db);
            }
            total_cost += week_cost;

            table.add_row(vec![
                Cell::new(*week),
                Cell::new(&models_str),
                Cell::new(format_number(week_data.stats.input_tokens)),
                Cell::new(format_number(week_data.stats.output_tokens)),
                Cell::new(format_number(week_data.stats.cache_creation)),
                Cell::new(format_number(week_data.stats.cache_read)),
                Cell::new(format_number(week_data.stats.total_tokens())),
                Cell::new(format!("${:.2}", week_cost)),
            ]);
        }

        total_stats.add(&week_data.stats);
    }

    let cyan = if use_color { Some(Color::Cyan) } else { None };
    let green = if use_color { Some(Color::Green) } else { None };

    // Add total row
    if compact {
        table.add_row(vec![
            styled_cell("TOTAL", cyan, true),
            styled_cell(&format_compact(total_stats.input_tokens), cyan, false),
            styled_cell(&format_compact(total_stats.output_tokens), cyan, false),
            styled_cell(&format_compact(total_stats.total_tokens()), cyan, false),
            styled_cell(&format!("${:.2}", total_cost), green, true),
        ]);
    } else if breakdown {
        table.add_row(vec![
            styled_cell("TOTAL", cyan, true),
            Cell::new(""),
            styled_cell(&format_number(total_stats.input_tokens), cyan, false),
            styled_cell(&format_number(total_stats.output_tokens), cyan, false),
            styled_cell(&format_number(total_stats.cache_creation), cyan, false),
            styled_cell(&format_number(total_stats.cache_read), cyan, false),
            styled_cell(&format!("${:.2}", total_cost), green, true),
        ]);
    } else {
        table.add_row(vec![
            styled_cell("TOTAL", cyan, true),
            Cell::new(""),
            styled_cell(&format_number(total_stats.input_tokens), cyan, false),
            styled_cell(&format_number(total_stats.output_tokens), cyan, false),
            styled_cell(&format_number(total_stats.cache_creation), cyan, false),
            styled_cell(&format_number(total_stats.cache_read), cyan, false),
            styled_cell(&format_number(total_stats.total_tokens()), cyan, false),
            styled_cell(&format!("${:.2}", total_cost), green, true),
        ]);
    }

    println!("\n  Claude Code Weekly Token Usage\n");
    println!("{table}");
    println!(
        "\n  {} unique API calls ({} streaming entries deduplicated)\n",
        format_number(valid),
        format_number(skipped)
    );
}
