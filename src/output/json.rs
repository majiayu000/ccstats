use std::cmp::Ordering;
use std::collections::HashMap;

use crate::cli::SortOrder;
use crate::core::{DataQuality, DayStats};
use crate::output::format::{cache_hit_rate_json_value, cost_json_value};
use crate::output::period::{Period, aggregate_day_stats_by_period};
use crate::output::pricing_meta;
use crate::pricing::{
    CostDisplayMode, CurrencyConverter, PricingDb, calculate_display_cost,
    calculate_estimated_proxy_cost, model_cost_kind, sum_display_model_costs,
    sum_estimated_proxy_model_costs,
};

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
            let cost_a = a.get("cost").and_then(serde_json::Value::as_f64);
            let cost_b = b.get("cost").and_then(serde_json::Value::as_f64);
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
    options: &PeriodJsonOptions<'_>,
) -> serde_json::Value {
    if options.breakdown {
        let mut models_breakdown: Vec<serde_json::Value> = Vec::new();
        let mut period_cost = 0.0;

        for (model, model_stats) in &stats.models {
            let mut model_obj = serde_json::json!({
                "model": model,
                "input_tokens": model_stats.input_tokens,
                "output_tokens": model_stats.output_tokens,
                "reasoning_tokens": model_stats.reasoning_tokens,
                "cache_creation_tokens": model_stats.cache_creation,
                "cache_read_tokens": model_stats.cache_read,
                "cache_hit_rate": cache_hit_rate_json_value(
                    model_stats.cache_hit_rate(options.supports_cache_read)
                ),
                "total_tokens": model_stats.total_tokens(),
            });
            if options.show_cost {
                let cost = calculate_display_cost(
                    model_stats,
                    model,
                    options.pricing_db,
                    options.cost_mode,
                );
                period_cost += cost;
                model_obj["cost"] = cost_json_value(cost, options.currency);
                pricing_meta::add_model_json(&mut model_obj, model, options.pricing_db);
                let estimated_cost =
                    calculate_estimated_proxy_cost(model_stats, model, options.pricing_db);
                if estimated_cost > 0.0 {
                    model_obj["cost_kind"] = serde_json::json!(model_stats.cost_kind().as_str());
                    model_obj["estimated_cost"] = cost_json_value(estimated_cost, options.currency);
                }
            }
            models_breakdown.push(model_obj);
        }

        sort_models_breakdown(&mut models_breakdown, options.show_cost);

        let mut obj = serde_json::json!({
            (label): key,
            "input_tokens": stats.stats.input_tokens,
            "output_tokens": stats.stats.output_tokens,
            "reasoning_tokens": stats.stats.reasoning_tokens,
            "cache_creation_tokens": stats.stats.cache_creation,
            "cache_read_tokens": stats.stats.cache_read,
            "cache_hit_rate": cache_hit_rate_json_value(
                stats.stats.cache_hit_rate(options.supports_cache_read)
            ),
            "total_tokens": stats.stats.total_tokens(),
            "breakdown": models_breakdown,
        });
        if options.show_cost {
            obj["cost"] = cost_json_value(period_cost, options.currency);
            pricing_meta::add_json(&mut obj, &stats.models, options.pricing_db);
            let estimated_cost = sum_estimated_proxy_model_costs(&stats.models, options.pricing_db);
            if estimated_cost > 0.0 {
                obj["cost_kind"] = serde_json::json!(model_cost_kind(&stats.models).as_str());
                obj["estimated_cost"] = cost_json_value(estimated_cost, options.currency);
            }
        }
        obj
    } else {
        let period_cost = if options.show_cost {
            Some(sum_display_model_costs(
                &stats.models,
                options.pricing_db,
                options.cost_mode,
            ))
        } else {
            None
        };
        let mut models: Vec<_> = stats.models.keys().cloned().collect();
        models.sort();
        let mut obj = serde_json::json!({
            (label): key,
            "input_tokens": stats.stats.input_tokens,
            "output_tokens": stats.stats.output_tokens,
            "reasoning_tokens": stats.stats.reasoning_tokens,
            "cache_creation_tokens": stats.stats.cache_creation,
            "cache_read_tokens": stats.stats.cache_read,
            "cache_hit_rate": cache_hit_rate_json_value(
                stats.stats.cache_hit_rate(options.supports_cache_read)
            ),
            "total_tokens": stats.stats.total_tokens(),
            "models": models,
        });
        if options.show_cost {
            obj["cost"] = cost_json_value(period_cost.unwrap_or(0.0), options.currency);
            pricing_meta::add_json(&mut obj, &stats.models, options.pricing_db);
            let estimated_cost = sum_estimated_proxy_model_costs(&stats.models, options.pricing_db);
            if estimated_cost > 0.0 {
                obj["cost_kind"] = serde_json::json!(model_cost_kind(&stats.models).as_str());
                obj["estimated_cost"] = cost_json_value(estimated_cost, options.currency);
            }
        }
        obj
    }
}

struct PeriodJsonOptions<'a> {
    pricing_db: &'a PricingDb,
    breakdown: bool,
    show_cost: bool,
    supports_cache_read: bool,
    currency: Option<&'a CurrencyConverter>,
    cost_mode: CostDisplayMode,
}

fn data_quality_json(data_quality: DataQuality) -> serde_json::Value {
    serde_json::json!({
        "valid_entries": data_quality.valid_entries,
        "dedup_skipped_entries": data_quality.dedup_skipped_entries,
        "parse_errors": data_quality.parse_errors,
    })
}

#[cfg(test)]
pub(crate) fn output_period_json(
    day_stats: &HashMap<String, DayStats>,
    period: Period,
    pricing_db: &PricingDb,
    order: SortOrder,
    breakdown: bool,
    show_cost: bool,
    currency: Option<&CurrencyConverter>,
) -> String {
    output_period_json_with_quality(
        day_stats,
        period,
        pricing_db,
        order,
        breakdown,
        show_cost,
        true,
        currency,
        None,
        CostDisplayMode::Total,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn output_period_json_with_quality(
    day_stats: &HashMap<String, DayStats>,
    period: Period,
    pricing_db: &PricingDb,
    order: SortOrder,
    breakdown: bool,
    show_cost: bool,
    supports_cache_read: bool,
    currency: Option<&CurrencyConverter>,
    data_quality: Option<DataQuality>,
    cost_mode: CostDisplayMode,
) -> String {
    let label = period.label();
    let aggregated;
    let stats_ref = if period == Period::Day {
        day_stats
    } else {
        aggregated = aggregate_day_stats_by_period(day_stats, period);
        &aggregated
    };

    let data_quality = data_quality.map(data_quality_json);
    let options = PeriodJsonOptions {
        pricing_db,
        breakdown,
        show_cost,
        supports_cache_read,
        currency,
        cost_mode,
    };
    let mut output: Vec<serde_json::Value> = stats_ref
        .iter()
        .map(|(key, stats)| {
            let mut entry = build_period_entry(label, key, stats, &options);
            if let Some(data_quality) = &data_quality {
                entry["data_quality"] = data_quality.clone();
            }
            entry
        })
        .collect();

    if output.is_empty()
        && let Some(data_quality) = data_quality
    {
        output.push(serde_json::json!({ "data_quality": data_quality }));
    }

    sort_output(&mut output, label, order);
    serde_json::to_string(&output).unwrap_or_else(|e| {
        eprintln!("Failed to serialize JSON output: {e}");
        "[]".to_string()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::Stats;

    fn make_day_stats(models: &[(&str, i64)]) -> DayStats {
        let mut ds = DayStats::default();
        for &(model, tokens) in models {
            let stats = Stats {
                input_tokens: tokens,
                output_tokens: tokens / 2,
                count: 1,
                ..Default::default()
            };
            ds.add_stats(model.to_string(), &stats);
        }
        ds
    }

    #[test]
    fn period_label_mapping() {
        assert_eq!(Period::Day.label(), "date");
        assert_eq!(Period::Week.label(), "week");
        assert_eq!(Period::Month.label(), "month");
    }

    #[test]
    fn sort_output_asc() {
        let mut vals = vec![
            serde_json::json!({"date": "2025-01-15"}),
            serde_json::json!({"date": "2025-01-01"}),
            serde_json::json!({"date": "2025-01-10"}),
        ];
        sort_output(&mut vals, "date", SortOrder::Asc);
        let dates: Vec<&str> = vals.iter().map(|v| v["date"].as_str().unwrap()).collect();
        assert_eq!(dates, vec!["2025-01-01", "2025-01-10", "2025-01-15"]);
    }

    #[test]
    fn sort_output_desc() {
        let mut vals = vec![
            serde_json::json!({"date": "2025-01-01"}),
            serde_json::json!({"date": "2025-01-15"}),
        ];
        sort_output(&mut vals, "date", SortOrder::Desc);
        assert_eq!(vals[0]["date"].as_str().unwrap(), "2025-01-15");
    }

    #[test]
    fn sort_models_breakdown_by_cost_desc() {
        let mut models = vec![
            serde_json::json!({"model": "opus", "cost": 1.0}),
            serde_json::json!({"model": "sonnet", "cost": 5.0}),
            serde_json::json!({"model": "haiku", "cost": 0.5}),
        ];
        sort_models_breakdown(&mut models, true);
        let names: Vec<&str> = models
            .iter()
            .map(|v| v["model"].as_str().unwrap())
            .collect();
        assert_eq!(names, vec!["sonnet", "opus", "haiku"]);
    }

    #[test]
    fn sort_models_breakdown_by_name_when_no_cost() {
        let mut models = vec![
            serde_json::json!({"model": "sonnet"}),
            serde_json::json!({"model": "haiku"}),
            serde_json::json!({"model": "opus"}),
        ];
        sort_models_breakdown(&mut models, false);
        let names: Vec<&str> = models
            .iter()
            .map(|v| v["model"].as_str().unwrap())
            .collect();
        assert_eq!(names, vec!["haiku", "opus", "sonnet"]);
    }

    #[test]
    fn output_daily_json_structure() {
        let mut day_stats = HashMap::new();
        day_stats.insert(
            "2025-01-01".to_string(),
            make_day_stats(&[("sonnet", 1000)]),
        );

        let db = PricingDb::default();
        let json_str = output_period_json(
            &day_stats,
            Period::Day,
            &db,
            SortOrder::Asc,
            false,
            false,
            None,
        );
        let parsed: Vec<serde_json::Value> = serde_json::from_str(&json_str).unwrap();

        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0]["date"], "2025-01-01");
        assert_eq!(parsed[0]["input_tokens"], 1000);
        assert_eq!(parsed[0]["output_tokens"], 500);
        assert!(parsed[0].get("cost").is_none());
    }

    #[test]
    fn output_daily_json_with_cost() {
        let mut day_stats = HashMap::new();
        day_stats.insert(
            "2025-01-01".to_string(),
            make_day_stats(&[("sonnet", 1000)]),
        );

        let db = PricingDb::default();
        let json_str = output_period_json(
            &day_stats,
            Period::Day,
            &db,
            SortOrder::Asc,
            false,
            true,
            None,
        );
        let parsed: Vec<serde_json::Value> = serde_json::from_str(&json_str).unwrap();

        assert!(parsed[0].get("cost").is_some());
    }

    #[test]
    fn output_daily_json_with_breakdown() {
        let mut day_stats = HashMap::new();
        day_stats.insert(
            "2025-01-01".to_string(),
            make_day_stats(&[("sonnet", 1000), ("opus", 500)]),
        );

        let db = PricingDb::default();
        let json_str = output_period_json(
            &day_stats,
            Period::Day,
            &db,
            SortOrder::Asc,
            true,
            false,
            None,
        );
        let parsed: Vec<serde_json::Value> = serde_json::from_str(&json_str).unwrap();

        let breakdown = parsed[0]["breakdown"].as_array().unwrap();
        assert_eq!(breakdown.len(), 2);
    }

    #[test]
    fn output_weekly_json_aggregates() {
        let mut day_stats = HashMap::new();
        // Same week (Mon 2025-01-06 and Wed 2025-01-08)
        day_stats.insert("2025-01-06".to_string(), make_day_stats(&[("sonnet", 100)]));
        day_stats.insert("2025-01-08".to_string(), make_day_stats(&[("sonnet", 200)]));

        let db = PricingDb::default();
        let json_str = output_period_json(
            &day_stats,
            Period::Week,
            &db,
            SortOrder::Asc,
            false,
            false,
            None,
        );
        let parsed: Vec<serde_json::Value> = serde_json::from_str(&json_str).unwrap();

        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0]["week"], "2025-01-06");
        assert_eq!(parsed[0]["input_tokens"], 300);
    }

    #[test]
    fn output_monthly_json_uses_month_key() {
        let mut day_stats = HashMap::new();
        day_stats.insert("2025-03-15".to_string(), make_day_stats(&[("sonnet", 100)]));

        let db = PricingDb::default();
        let json_str = output_period_json(
            &day_stats,
            Period::Month,
            &db,
            SortOrder::Asc,
            false,
            false,
            None,
        );
        let parsed: Vec<serde_json::Value> = serde_json::from_str(&json_str).unwrap();

        assert_eq!(parsed[0]["month"], "2025-03");
    }
}
