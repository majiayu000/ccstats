use comfy_table::{presets::UTF8_FULL, Attribute, Cell, Color, ContentArrangement, Table};
use std::collections::HashMap;

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

pub fn print_daily_table(
    day_stats: &HashMap<String, DayStats>,
    breakdown: bool,
    skipped: i64,
    valid: i64,
    pricing_db: &PricingDb,
) {
    let mut dates: Vec<_> = day_stats.keys().collect();
    dates.sort();

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic);

    if breakdown {
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

        if breakdown {
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

    // Add total row
    if breakdown {
        table.add_row(vec![
            Cell::new("TOTAL")
                .fg(Color::Cyan)
                .add_attribute(Attribute::Bold),
            Cell::new(""),
            Cell::new(format_number(total_stats.input_tokens)).fg(Color::Cyan),
            Cell::new(format_number(total_stats.output_tokens)).fg(Color::Cyan),
            Cell::new(format_number(total_stats.cache_creation)).fg(Color::Cyan),
            Cell::new(format_number(total_stats.cache_read)).fg(Color::Cyan),
            Cell::new(format!("${:.2}", total_cost))
                .fg(Color::Green)
                .add_attribute(Attribute::Bold),
        ]);
    } else {
        table.add_row(vec![
            Cell::new("TOTAL")
                .fg(Color::Cyan)
                .add_attribute(Attribute::Bold),
            Cell::new(""),
            Cell::new(format_number(total_stats.input_tokens)).fg(Color::Cyan),
            Cell::new(format_number(total_stats.output_tokens)).fg(Color::Cyan),
            Cell::new(format_number(total_stats.cache_creation)).fg(Color::Cyan),
            Cell::new(format_number(total_stats.cache_read)).fg(Color::Cyan),
            Cell::new(format_number(total_stats.total_tokens())).fg(Color::Cyan),
            Cell::new(format!("${:.2}", total_cost))
                .fg(Color::Green)
                .add_attribute(Attribute::Bold),
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
    months.sort();

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic);

    if breakdown {
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

        if breakdown {
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

    // Add total row
    if breakdown {
        table.add_row(vec![
            Cell::new("TOTAL")
                .fg(Color::Cyan)
                .add_attribute(Attribute::Bold),
            Cell::new(""),
            Cell::new(format_number(total_stats.input_tokens)).fg(Color::Cyan),
            Cell::new(format_number(total_stats.output_tokens)).fg(Color::Cyan),
            Cell::new(format_number(total_stats.cache_creation)).fg(Color::Cyan),
            Cell::new(format_number(total_stats.cache_read)).fg(Color::Cyan),
            Cell::new(format!("${:.2}", total_cost))
                .fg(Color::Green)
                .add_attribute(Attribute::Bold),
        ]);
    } else {
        table.add_row(vec![
            Cell::new("TOTAL")
                .fg(Color::Cyan)
                .add_attribute(Attribute::Bold),
            Cell::new(""),
            Cell::new(format_number(total_stats.input_tokens)).fg(Color::Cyan),
            Cell::new(format_number(total_stats.output_tokens)).fg(Color::Cyan),
            Cell::new(format_number(total_stats.cache_creation)).fg(Color::Cyan),
            Cell::new(format_number(total_stats.cache_read)).fg(Color::Cyan),
            Cell::new(format_number(total_stats.total_tokens())).fg(Color::Cyan),
            Cell::new(format!("${:.2}", total_cost))
                .fg(Color::Green)
                .add_attribute(Attribute::Bold),
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
