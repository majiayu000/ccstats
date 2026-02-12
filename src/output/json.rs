use std::cmp::Ordering;
use std::collections::HashMap;

use crate::cli::SortOrder;
use crate::core::DayStats;
use crate::output::format::cost_json_value;
use crate::output::period::{Period, aggregate_day_stats_by_period};
use crate::pricing::{PricingDb, calculate_cost, sum_model_costs};

fn period_label(period: Period) -> &'static str {
    match period {
        Period::Day => "date",
        Period::Week => "week",
        Period::Month => "month",
    }
}

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

fn sort_models_breakdown(breakdown: &mut [serde_json::Value], show_cost: bool) {
    if show_cost {
        breakdown.sort_by(|a, b| {
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
        breakdown.sort_by(|a, b| {
            a.get("model")
                .and_then(|v| v.as_str())
                .cmp(&b.get("model").and_then(|v| v.as_str()))
        });
    }
}

fn build_period_entry(
    label: &str,
    key: &str,
    stats: &DayStats,
    pricing_db: &PricingDb,
    breakdown: bool,
    show_cost: bool,
) -> serde_json::Value {
    if breakdown {
        let mut models_breakdown: Vec<serde_json::Value> = Vec::new();
        let mut period_cost = 0.0;

        for (model, model_stats) in &stats.models {
            let cost = calculate_cost(model_stats, model, pricing_db);
            period_cost += cost;
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

        sort_models_breakdown(&mut models_breakdown, show_cost);

        let mut obj = serde_json::json!({
            (label): key,
            "input_tokens": stats.stats.input_tokens,
            "output_tokens": stats.stats.output_tokens,
            "reasoning_tokens": stats.stats.reasoning_tokens,
            "cache_creation_tokens": stats.stats.cache_creation,
            "cache_read_tokens": stats.stats.cache_read,
            "total_tokens": stats.stats.total_tokens(),
            "breakdown": models_breakdown,
        });
        if show_cost {
            obj["cost"] = cost_json_value(period_cost);
        }
        obj
    } else {
        let period_cost = sum_model_costs(&stats.models, pricing_db);
        let mut models: Vec<_> = stats.models.keys().cloned().collect();
        models.sort();
        let mut obj = serde_json::json!({
            (label): key,
            "input_tokens": stats.stats.input_tokens,
            "output_tokens": stats.stats.output_tokens,
            "reasoning_tokens": stats.stats.reasoning_tokens,
            "cache_creation_tokens": stats.stats.cache_creation,
            "cache_read_tokens": stats.stats.cache_read,
            "total_tokens": stats.stats.total_tokens(),
            "models": models,
        });
        if show_cost {
            obj["cost"] = cost_json_value(period_cost);
        }
        obj
    }
}

fn output_period_json(
    day_stats: &HashMap<String, DayStats>,
    period: Period,
    pricing_db: &PricingDb,
    order: SortOrder,
    breakdown: bool,
    show_cost: bool,
) -> String {
    let label = period_label(period);
    let aggregated;
    let stats_ref = if period == Period::Day {
        day_stats
    } else {
        aggregated = aggregate_day_stats_by_period(day_stats, period);
        &aggregated
    };

    let mut output: Vec<serde_json::Value> = stats_ref
        .iter()
        .map(|(key, stats)| build_period_entry(label, key, stats, pricing_db, breakdown, show_cost))
        .collect();

    sort_output(&mut output, label, order);
    serde_json::to_string_pretty(&output).unwrap_or_else(|e| {
        eprintln!("Failed to serialize JSON output: {}", e);
        "[]".to_string()
    })
}

pub(crate) fn output_daily_json(
    day_stats: &HashMap<String, DayStats>,
    pricing_db: &PricingDb,
    order: SortOrder,
    breakdown: bool,
    show_cost: bool,
) -> String {
    output_period_json(day_stats, Period::Day, pricing_db, order, breakdown, show_cost)
}

pub(crate) fn output_weekly_json(
    day_stats: &HashMap<String, DayStats>,
    pricing_db: &PricingDb,
    order: SortOrder,
    breakdown: bool,
    show_cost: bool,
) -> String {
    output_period_json(day_stats, Period::Week, pricing_db, order, breakdown, show_cost)
}

pub(crate) fn output_monthly_json(
    day_stats: &HashMap<String, DayStats>,
    pricing_db: &PricingDb,
    order: SortOrder,
    breakdown: bool,
    show_cost: bool,
) -> String {
    output_period_json(day_stats, Period::Month, pricing_db, order, breakdown, show_cost)
}
