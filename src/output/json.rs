use chrono::{Datelike, NaiveDate};
use std::cmp::Ordering;
use std::collections::HashMap;

use crate::cli::SortOrder;
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

fn sort_output(output: &mut Vec<serde_json::Value>, key: &str, order: SortOrder) {
    match order {
        SortOrder::Asc => output.sort_by(|a, b| {
            a.get(key)
                .and_then(|v| v.as_str())
                .cmp(&b.get(key).and_then(|v| v.as_str()))
        }),
        SortOrder::Desc => output.sort_by(|a, b| {
            b.get(key)
                .and_then(|v| v.as_str())
                .cmp(&a.get(key).and_then(|v| v.as_str()))
        }),
    }
}

pub fn output_daily_json(
    day_stats: &HashMap<String, DayStats>,
    pricing_db: &PricingDb,
    order: SortOrder,
    breakdown: bool,
    show_cost: bool,
) -> String {
    let mut output: Vec<serde_json::Value> = Vec::new();

    for (date, stats) in day_stats {
        if breakdown {
            // Output per-model breakdown
            let mut models_breakdown: Vec<serde_json::Value> = Vec::new();
            let mut day_cost = 0.0;

            for (model, model_stats) in &stats.models {
                let cost = calculate_cost(model_stats, model, pricing_db);
                day_cost += cost;
                let mut model_obj = serde_json::json!({
                    "model": model,
                    "input_tokens": model_stats.input_tokens,
                    "output_tokens": model_stats.output_tokens,
                    "cache_creation_tokens": model_stats.cache_creation,
                    "cache_read_tokens": model_stats.cache_read,
                    "total_tokens": model_stats.total_tokens(),
                });
                if show_cost {
                    model_obj["cost"] = serde_json::json!(cost);
                }
                models_breakdown.push(model_obj);
            }

            // Sort models for stable output
            if show_cost {
                models_breakdown.sort_by(|a, b| {
                    let cost_a = a.get("cost").and_then(|v| v.as_f64());
                    let cost_b = b.get("cost").and_then(|v| v.as_f64());
                    match cost_b.partial_cmp(&cost_a).unwrap_or(Ordering::Equal) {
                        Ordering::Equal => a
                            .get("model")
                            .and_then(|v| v.as_str())
                            .cmp(&b.get("model").and_then(|v| v.as_str())),
                        other => other,
                    }
                });
            } else {
                models_breakdown.sort_by(|a, b| {
                    a.get("model")
                        .and_then(|v| v.as_str())
                        .cmp(&b.get("model").and_then(|v| v.as_str()))
                });
            }

            let mut day_obj = serde_json::json!({
                "date": date,
                "input_tokens": stats.stats.input_tokens,
                "output_tokens": stats.stats.output_tokens,
                "cache_creation_tokens": stats.stats.cache_creation,
                "cache_read_tokens": stats.stats.cache_read,
                "total_tokens": stats.stats.total_tokens(),
                "breakdown": models_breakdown,
            });
            if show_cost {
                day_obj["cost"] = serde_json::json!(day_cost);
            }
            output.push(day_obj);
        } else {
            let mut day_cost = 0.0;
            for (model, model_stats) in &stats.models {
                day_cost += calculate_cost(model_stats, model, pricing_db);
            }
            let mut models: Vec<_> = stats.models.keys().cloned().collect();
            models.sort();
            let mut day_obj = serde_json::json!({
                "date": date,
                "input_tokens": stats.stats.input_tokens,
                "output_tokens": stats.stats.output_tokens,
                "cache_creation_tokens": stats.stats.cache_creation,
                "cache_read_tokens": stats.stats.cache_read,
                "total_tokens": stats.stats.total_tokens(),
                "models": models,
            });
            if show_cost {
                day_obj["cost"] = serde_json::json!(day_cost);
            }
            output.push(day_obj);
        }
    }

    sort_output(&mut output, "date", order);
    serde_json::to_string_pretty(&output).unwrap_or_else(|e| {
        eprintln!("Failed to serialize JSON output: {}", e);
        "[]".to_string()
    })
}

pub fn output_monthly_json(
    day_stats: &HashMap<String, DayStats>,
    pricing_db: &PricingDb,
    order: SortOrder,
    breakdown: bool,
    show_cost: bool,
) -> String {
    // Aggregate by month
    let mut month_stats: HashMap<String, DayStats> = HashMap::new();

    for (date, stats) in day_stats {
        let month = &date[..7];
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

    let mut output: Vec<serde_json::Value> = Vec::new();

    for (month, stats) in &month_stats {
        if breakdown {
            let mut models_breakdown: Vec<serde_json::Value> = Vec::new();
            let mut month_cost = 0.0;

            for (model, model_stats) in &stats.models {
                let cost = calculate_cost(model_stats, model, pricing_db);
                month_cost += cost;
                let mut model_obj = serde_json::json!({
                    "model": model,
                    "input_tokens": model_stats.input_tokens,
                    "output_tokens": model_stats.output_tokens,
                    "cache_creation_tokens": model_stats.cache_creation,
                    "cache_read_tokens": model_stats.cache_read,
                    "total_tokens": model_stats.total_tokens(),
                });
                if show_cost {
                    model_obj["cost"] = serde_json::json!(cost);
                }
                models_breakdown.push(model_obj);
            }

            if show_cost {
                models_breakdown.sort_by(|a, b| {
                    let cost_a = a.get("cost").and_then(|v| v.as_f64());
                    let cost_b = b.get("cost").and_then(|v| v.as_f64());
                    match cost_b.partial_cmp(&cost_a).unwrap_or(Ordering::Equal) {
                        Ordering::Equal => a
                            .get("model")
                            .and_then(|v| v.as_str())
                            .cmp(&b.get("model").and_then(|v| v.as_str())),
                        other => other,
                    }
                });
            } else {
                models_breakdown.sort_by(|a, b| {
                    a.get("model")
                        .and_then(|v| v.as_str())
                        .cmp(&b.get("model").and_then(|v| v.as_str()))
                });
            }

            let mut month_obj = serde_json::json!({
                "month": month,
                "input_tokens": stats.stats.input_tokens,
                "output_tokens": stats.stats.output_tokens,
                "cache_creation_tokens": stats.stats.cache_creation,
                "cache_read_tokens": stats.stats.cache_read,
                "total_tokens": stats.stats.total_tokens(),
                "breakdown": models_breakdown,
            });
            if show_cost {
                month_obj["cost"] = serde_json::json!(month_cost);
            }
            output.push(month_obj);
        } else {
            let mut month_cost = 0.0;
            for (model, model_stats) in &stats.models {
                month_cost += calculate_cost(model_stats, model, pricing_db);
            }
            let mut models: Vec<_> = stats.models.keys().cloned().collect();
            models.sort();
            let mut month_obj = serde_json::json!({
                "month": month,
                "input_tokens": stats.stats.input_tokens,
                "output_tokens": stats.stats.output_tokens,
                "cache_creation_tokens": stats.stats.cache_creation,
                "cache_read_tokens": stats.stats.cache_read,
                "total_tokens": stats.stats.total_tokens(),
                "models": models,
            });
            if show_cost {
                month_obj["cost"] = serde_json::json!(month_cost);
            }
            output.push(month_obj);
        }
    }

    sort_output(&mut output, "month", order);
    serde_json::to_string_pretty(&output).unwrap_or_else(|e| {
        eprintln!("Failed to serialize JSON output: {}", e);
        "[]".to_string()
    })
}

pub fn output_weekly_json(
    day_stats: &HashMap<String, DayStats>,
    pricing_db: &PricingDb,
    order: SortOrder,
    breakdown: bool,
    show_cost: bool,
) -> String {
    // Aggregate by week
    let mut week_stats: HashMap<String, DayStats> = HashMap::new();

    for (date, stats) in day_stats {
        let week = get_week_start(date);
        let week_entry = week_stats.entry(week).or_default();

        for (model, model_stats) in &stats.models {
            week_entry.stats.add(model_stats);
            week_entry
                .models
                .entry(model.clone())
                .or_default()
                .add(model_stats);
        }
    }

    let mut output: Vec<serde_json::Value> = Vec::new();

    for (week, stats) in &week_stats {
        if breakdown {
            let mut models_breakdown: Vec<serde_json::Value> = Vec::new();
            let mut week_cost = 0.0;

            for (model, model_stats) in &stats.models {
                let cost = calculate_cost(model_stats, model, pricing_db);
                week_cost += cost;
                let mut model_obj = serde_json::json!({
                    "model": model,
                    "input_tokens": model_stats.input_tokens,
                    "output_tokens": model_stats.output_tokens,
                    "cache_creation_tokens": model_stats.cache_creation,
                    "cache_read_tokens": model_stats.cache_read,
                    "total_tokens": model_stats.total_tokens(),
                });
                if show_cost {
                    model_obj["cost"] = serde_json::json!(cost);
                }
                models_breakdown.push(model_obj);
            }

            if show_cost {
                models_breakdown.sort_by(|a, b| {
                    let cost_a = a.get("cost").and_then(|v| v.as_f64());
                    let cost_b = b.get("cost").and_then(|v| v.as_f64());
                    match cost_b.partial_cmp(&cost_a).unwrap_or(Ordering::Equal) {
                        Ordering::Equal => a
                            .get("model")
                            .and_then(|v| v.as_str())
                            .cmp(&b.get("model").and_then(|v| v.as_str())),
                        other => other,
                    }
                });
            } else {
                models_breakdown.sort_by(|a, b| {
                    a.get("model")
                        .and_then(|v| v.as_str())
                        .cmp(&b.get("model").and_then(|v| v.as_str()))
                });
            }

            let mut week_obj = serde_json::json!({
                "week": week,
                "input_tokens": stats.stats.input_tokens,
                "output_tokens": stats.stats.output_tokens,
                "cache_creation_tokens": stats.stats.cache_creation,
                "cache_read_tokens": stats.stats.cache_read,
                "total_tokens": stats.stats.total_tokens(),
                "breakdown": models_breakdown,
            });
            if show_cost {
                week_obj["cost"] = serde_json::json!(week_cost);
            }
            output.push(week_obj);
        } else {
            let mut week_cost = 0.0;
            for (model, model_stats) in &stats.models {
                week_cost += calculate_cost(model_stats, model, pricing_db);
            }
            let mut models: Vec<_> = stats.models.keys().cloned().collect();
            models.sort();
            let mut week_obj = serde_json::json!({
                "week": week,
                "input_tokens": stats.stats.input_tokens,
                "output_tokens": stats.stats.output_tokens,
                "cache_creation_tokens": stats.stats.cache_creation,
                "cache_read_tokens": stats.stats.cache_read,
                "total_tokens": stats.stats.total_tokens(),
                "models": models,
            });
            if show_cost {
                week_obj["cost"] = serde_json::json!(week_cost);
            }
            output.push(week_obj);
        }
    }

    sort_output(&mut output, "week", order);
    serde_json::to_string_pretty(&output).unwrap_or_else(|e| {
        eprintln!("Failed to serialize JSON output: {}", e);
        "[]".to_string()
    })
}
