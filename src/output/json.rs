use chrono::{Datelike, NaiveDate};
use std::collections::HashMap;

use crate::data::DayStats;
use crate::pricing::{calculate_cost, PricingDb};

/// Get the Monday of the week for a given date (ISO week)
fn get_week_start(date_str: &str) -> String {
    if let Ok(date) = NaiveDate::parse_from_str(date_str, "%Y-%m-%d") {
        let weekday = date.weekday().num_days_from_monday();
        let monday = date - chrono::Duration::days(weekday as i64);
        monday.format("%Y-%m-%d").to_string()
    } else {
        date_str.to_string()
    }
}

pub fn output_daily_json(day_stats: &HashMap<String, DayStats>, pricing_db: &PricingDb) {
    let mut output: Vec<serde_json::Value> = Vec::new();
    for (date, stats) in day_stats {
        let mut day_cost = 0.0;
        for (model, model_stats) in &stats.models {
            day_cost += calculate_cost(model_stats, model, pricing_db);
        }
        output.push(serde_json::json!({
            "date": date,
            "input_tokens": stats.stats.input_tokens,
            "output_tokens": stats.stats.output_tokens,
            "cache_creation_tokens": stats.stats.cache_creation,
            "cache_read_tokens": stats.stats.cache_read,
            "total_tokens": stats.stats.total_tokens(),
            "cost": day_cost,
            "models": stats.models.keys().collect::<Vec<_>>(),
        }));
    }
    // Sort by date
    output.sort_by(|a, b| {
        a.get("date")
            .and_then(|v| v.as_str())
            .cmp(&b.get("date").and_then(|v| v.as_str()))
    });
    println!("{}", serde_json::to_string_pretty(&output).unwrap());
}

pub fn output_monthly_json(day_stats: &HashMap<String, DayStats>, pricing_db: &PricingDb) {
    // Aggregate by month
    let mut month_data: HashMap<String, (i64, i64, i64, i64, f64, Vec<String>)> = HashMap::new();

    for (date, stats) in day_stats {
        let month = &date[..7];
        let entry = month_data.entry(month.to_string()).or_default();

        entry.0 += stats.stats.input_tokens;
        entry.1 += stats.stats.output_tokens;
        entry.2 += stats.stats.cache_creation;
        entry.3 += stats.stats.cache_read;

        for (model, model_stats) in &stats.models {
            entry.4 += calculate_cost(model_stats, model, pricing_db);
            if !entry.5.contains(model) {
                entry.5.push(model.clone());
            }
        }
    }

    let mut output: Vec<serde_json::Value> = Vec::new();
    for (month, (input, output_tokens, cache_creation, cache_read, cost, models)) in month_data {
        output.push(serde_json::json!({
            "month": month,
            "input_tokens": input,
            "output_tokens": output_tokens,
            "cache_creation_tokens": cache_creation,
            "cache_read_tokens": cache_read,
            "total_tokens": input + output_tokens + cache_creation + cache_read,
            "cost": cost,
            "models": models,
        }));
    }
    // Sort by month
    output.sort_by(|a, b| {
        a.get("month")
            .and_then(|v| v.as_str())
            .cmp(&b.get("month").and_then(|v| v.as_str()))
    });
    println!("{}", serde_json::to_string_pretty(&output).unwrap());
}

pub fn output_weekly_json(day_stats: &HashMap<String, DayStats>, pricing_db: &PricingDb) {
    // Aggregate by week
    let mut week_data: HashMap<String, (i64, i64, i64, i64, f64, Vec<String>)> = HashMap::new();

    for (date, stats) in day_stats {
        let week = get_week_start(date);
        let entry = week_data.entry(week).or_default();

        entry.0 += stats.stats.input_tokens;
        entry.1 += stats.stats.output_tokens;
        entry.2 += stats.stats.cache_creation;
        entry.3 += stats.stats.cache_read;

        for (model, model_stats) in &stats.models {
            entry.4 += calculate_cost(model_stats, model, pricing_db);
            if !entry.5.contains(model) {
                entry.5.push(model.clone());
            }
        }
    }

    let mut output: Vec<serde_json::Value> = Vec::new();
    for (week, (input, output_tokens, cache_creation, cache_read, cost, models)) in week_data {
        output.push(serde_json::json!({
            "week": week,
            "input_tokens": input,
            "output_tokens": output_tokens,
            "cache_creation_tokens": cache_creation,
            "cache_read_tokens": cache_read,
            "total_tokens": input + output_tokens + cache_creation + cache_read,
            "cost": cost,
            "models": models,
        }));
    }
    // Sort by week
    output.sort_by(|a, b| {
        a.get("week")
            .and_then(|v| v.as_str())
            .cmp(&b.get("week").and_then(|v| v.as_str()))
    });
    println!("{}", serde_json::to_string_pretty(&output).unwrap());
}
