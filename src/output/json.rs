use std::cmp::Ordering;
use std::collections::HashMap;

use crate::cli::SortOrder;
use crate::core::DayStats;
use crate::output::format::cost_json_value;
use crate::output::period::{Period, aggregate_day_stats_by_period};
use crate::pricing::{PricingDb, calculate_cost, sum_model_costs};

fn sort_output(output: &mut [serde_json::Value], key: &str, order: SortOrder) {
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

pub(crate) fn output_daily_json(
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
                    "reasoning_tokens": model_stats.reasoning_tokens,
                    "cache_creation_tokens": model_stats.cache_creation,
                    "cache_read_tokens": model_stats.cache_read,
                    "total_tokens": model_stats.total_tokens(),
                });
                if show_cost {
                    model_obj["cost"] = cost_json_value(cost);
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
                "reasoning_tokens": stats.stats.reasoning_tokens,
                "cache_creation_tokens": stats.stats.cache_creation,
                "cache_read_tokens": stats.stats.cache_read,
                "total_tokens": stats.stats.total_tokens(),
                "breakdown": models_breakdown,
            });
            if show_cost {
                day_obj["cost"] = cost_json_value(day_cost);
            }
            output.push(day_obj);
        } else {
            let day_cost = sum_model_costs(&stats.models, pricing_db);
            let mut models: Vec<_> = stats.models.keys().cloned().collect();
            models.sort();
            let mut day_obj = serde_json::json!({
                "date": date,
                "input_tokens": stats.stats.input_tokens,
                "output_tokens": stats.stats.output_tokens,
                "reasoning_tokens": stats.stats.reasoning_tokens,
                "cache_creation_tokens": stats.stats.cache_creation,
                "cache_read_tokens": stats.stats.cache_read,
                "total_tokens": stats.stats.total_tokens(),
                "models": models,
            });
            if show_cost {
                day_obj["cost"] = cost_json_value(day_cost);
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

pub(crate) fn output_monthly_json(
    day_stats: &HashMap<String, DayStats>,
    pricing_db: &PricingDb,
    order: SortOrder,
    breakdown: bool,
    show_cost: bool,
) -> String {
    let month_stats = aggregate_day_stats_by_period(day_stats, Period::Month);

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
                    "reasoning_tokens": model_stats.reasoning_tokens,
                    "cache_creation_tokens": model_stats.cache_creation,
                    "cache_read_tokens": model_stats.cache_read,
                    "total_tokens": model_stats.total_tokens(),
                });
                if show_cost {
                    model_obj["cost"] = cost_json_value(cost);
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
                "reasoning_tokens": stats.stats.reasoning_tokens,
                "cache_creation_tokens": stats.stats.cache_creation,
                "cache_read_tokens": stats.stats.cache_read,
                "total_tokens": stats.stats.total_tokens(),
                "breakdown": models_breakdown,
            });
            if show_cost {
                month_obj["cost"] = cost_json_value(month_cost);
            }
            output.push(month_obj);
        } else {
            let month_cost = sum_model_costs(&stats.models, pricing_db);
            let mut models: Vec<_> = stats.models.keys().cloned().collect();
            models.sort();
            let mut month_obj = serde_json::json!({
                "month": month,
                "input_tokens": stats.stats.input_tokens,
                "output_tokens": stats.stats.output_tokens,
                "reasoning_tokens": stats.stats.reasoning_tokens,
                "cache_creation_tokens": stats.stats.cache_creation,
                "cache_read_tokens": stats.stats.cache_read,
                "total_tokens": stats.stats.total_tokens(),
                "models": models,
            });
            if show_cost {
                month_obj["cost"] = cost_json_value(month_cost);
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

pub(crate) fn output_weekly_json(
    day_stats: &HashMap<String, DayStats>,
    pricing_db: &PricingDb,
    order: SortOrder,
    breakdown: bool,
    show_cost: bool,
) -> String {
    let week_stats = aggregate_day_stats_by_period(day_stats, Period::Week);

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
                    "reasoning_tokens": model_stats.reasoning_tokens,
                    "cache_creation_tokens": model_stats.cache_creation,
                    "cache_read_tokens": model_stats.cache_read,
                    "total_tokens": model_stats.total_tokens(),
                });
                if show_cost {
                    model_obj["cost"] = cost_json_value(cost);
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
                "reasoning_tokens": stats.stats.reasoning_tokens,
                "cache_creation_tokens": stats.stats.cache_creation,
                "cache_read_tokens": stats.stats.cache_read,
                "total_tokens": stats.stats.total_tokens(),
                "breakdown": models_breakdown,
            });
            if show_cost {
                week_obj["cost"] = cost_json_value(week_cost);
            }
            output.push(week_obj);
        } else {
            let week_cost = sum_model_costs(&stats.models, pricing_db);
            let mut models: Vec<_> = stats.models.keys().cloned().collect();
            models.sort();
            let mut week_obj = serde_json::json!({
                "week": week,
                "input_tokens": stats.stats.input_tokens,
                "output_tokens": stats.stats.output_tokens,
                "reasoning_tokens": stats.stats.reasoning_tokens,
                "cache_creation_tokens": stats.stats.cache_creation,
                "cache_read_tokens": stats.stats.cache_read,
                "total_tokens": stats.stats.total_tokens(),
                "models": models,
            });
            if show_cost {
                week_obj["cost"] = cost_json_value(week_cost);
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
