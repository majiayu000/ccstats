use std::collections::HashMap;

use chrono::{DateTime, Utc};

use crate::core::{DataQuality, DayStats, Stats};
use crate::output::format::{
    NumberFormat, cache_hit_rate_json_value, cost_json_value, format_cache_hit_rate,
    format_compact, format_cost,
};
use crate::output::pricing_meta;
use crate::pricing::{
    CostDisplayMode, CurrencyConverter, PricingDb, model_cost_kind, sum_display_model_costs,
    sum_estimated_proxy_model_costs,
};
use crate::quota::{ClaudeHook, Confidence, CostSource, format_reset_countdown};

pub(crate) struct StatuslineHook {
    pub hook: ClaudeHook,
    pub cost_source: CostSource,
    pub captured_at: DateTime<Utc>,
}

struct Totals {
    stats: Stats,
    cost: f64,
    estimated_proxy_cost: f64,
}

fn aggregate_totals(
    day_stats: &HashMap<String, DayStats>,
    pricing_db: &PricingDb,
    cost_mode: CostDisplayMode,
) -> Totals {
    let mut stats = Stats::default();
    let mut cost = 0.0;
    let mut estimated_proxy_cost = 0.0;
    for day in day_stats.values() {
        stats.add(&day.stats);
        cost += sum_display_model_costs(&day.models, pricing_db, cost_mode);
        estimated_proxy_cost += sum_estimated_proxy_model_costs(&day.models, pricing_db);
    }
    Totals {
        stats,
        cost,
        estimated_proxy_cost,
    }
}

/// Output a single line suitable for statusline/tmux integration
/// Format: "CC: $X.XX | In: XM Out: XK | Today"
#[allow(clippy::too_many_arguments)]
pub(crate) fn print_statusline(
    day_stats: &HashMap<String, DayStats>,
    pricing_db: &PricingDb,
    source_label: &str,
    number_format: NumberFormat,
    currency: Option<&CurrencyConverter>,
    supports_cache_read: bool,
    cost_mode: CostDisplayMode,
    hook: Option<&StatuslineHook>,
) {
    let t = aggregate_totals(day_stats, pricing_db, cost_mode);

    let mut parts = vec![
        format!(
            "{}: {}",
            source_label,
            format_statusline_cost(t.cost, currency, hook)
        ),
        format!(
            "In: {} Out: {}",
            format_compact(t.stats.input_tokens, number_format),
            format_compact(t.stats.output_tokens, number_format)
        ),
    ];
    if t.stats.reasoning_tokens > 0 {
        parts.push(format!(
            "Reason: {}",
            format_compact(t.stats.reasoning_tokens, number_format)
        ));
    }
    parts.push(format!(
        "Cache Hit: {}",
        format_cache_hit_rate(t.stats.cache_hit_rate(supports_cache_read))
    ));
    parts.extend(quota_text_parts(hook));
    println!("{}", parts.join(" | "));
}

/// Output statusline as JSON for programmatic consumption
#[cfg(test)]
pub(crate) fn print_statusline_json(
    day_stats: &HashMap<String, DayStats>,
    pricing_db: &PricingDb,
    source_label: &str,
    number_format: NumberFormat,
    currency: Option<&CurrencyConverter>,
) -> String {
    print_statusline_json_with_quality(
        day_stats,
        pricing_db,
        source_label,
        number_format,
        currency,
        true,
        None,
        CostDisplayMode::Total,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn print_statusline_json_with_quality(
    day_stats: &HashMap<String, DayStats>,
    pricing_db: &PricingDb,
    source_label: &str,
    number_format: NumberFormat,
    currency: Option<&CurrencyConverter>,
    supports_cache_read: bool,
    data_quality: Option<DataQuality>,
    cost_mode: CostDisplayMode,
    hook: Option<&StatuslineHook>,
) -> String {
    let t = aggregate_totals(day_stats, pricing_db, cost_mode);
    let (display_cost, cc_cost) = resolve_costs(t.cost, hook);
    let confidence = if hook.is_some_and(|h| h.hook.has_official_windows()) {
        Confidence::Official
    } else {
        Confidence::Estimated
    };
    let captured_at = hook.map_or_else(Utc::now, |h| h.captured_at);

    let mut output = serde_json::json!({
        "source": source_label,
        "input_tokens": t.stats.input_tokens,
        "output_tokens": t.stats.output_tokens,
        "reasoning_tokens": t.stats.reasoning_tokens,
        "cache_creation_tokens": t.stats.cache_creation,
        "cache_creation_1h_tokens": t.stats.cache_creation_1h,
        "cache_read_tokens": t.stats.cache_read,
        "cache_hit_rate": cache_hit_rate_json_value(
            t.stats.cache_hit_rate(supports_cache_read)
        ),
        "total_tokens": t.stats.total_tokens(),
        "cost": cost_json_value(display_cost, currency),
        "confidence": confidence.as_str(),
        "captured_at": captured_at.to_rfc3339(),
        "formatted": {
            "cost": format_statusline_cost(t.cost, currency, hook),
            "input": format_compact(t.stats.input_tokens, number_format),
            "output": format_compact(t.stats.output_tokens, number_format),
            "reasoning": format_compact(t.stats.reasoning_tokens, number_format),
        }
    });
    if let Some(hook) = hook {
        output["cost_source"] = serde_json::json!(hook.cost_source.as_str());
        if let Some(cc_cost) = cc_cost {
            output["cc_cost"] = cost_json_value(cc_cost, currency);
        }
        if let Some(pct) = hook.hook.context_used_pct() {
            output["context_used_pct"] = serde_json::json!(pct);
        }
        if let Some(five) = hook.hook.five_hour() {
            output["rate_limits"]["five_hour"] = serde_json::json!({
                "used_pct": five.used_percentage,
                "resets_at": five.resets_at,
            });
        }
        if let Some(seven) = hook.hook.seven_day() {
            output["rate_limits"]["seven_day"] = serde_json::json!({
                "used_pct": seven.used_percentage,
                "resets_at": seven.resets_at,
            });
        }
    }
    if let Some(data_quality) = data_quality {
        output["data_quality"] = serde_json::json!({
            "valid_entries": data_quality.valid_entries,
            "dedup_skipped_entries": data_quality.dedup_skipped_entries,
            "parse_errors": data_quality.parse_errors,
        });
    }
    pricing_meta::add_json_for_maps(
        &mut output,
        day_stats.values().map(|day| &day.models),
        pricing_db,
    );
    if t.estimated_proxy_cost > 0.0 {
        let mut models: HashMap<String, Stats> = HashMap::new();
        for day in day_stats.values() {
            for (model, stats) in &day.models {
                models.entry(model.clone()).or_default().add(stats);
            }
        }
        output["cost_kind"] = serde_json::json!(model_cost_kind(&models).as_str());
        output["estimated_cost"] = cost_json_value(t.estimated_proxy_cost, currency);
    }

    serde_json::to_string(&output).unwrap_or_else(|e| {
        eprintln!("Failed to serialize JSON output: {e}");
        "{}".to_string()
    })
}

fn resolve_costs(ccstats: f64, hook: Option<&StatuslineHook>) -> (f64, Option<f64>) {
    let cc = hook.and_then(|h| h.hook.cc_cost_usd());
    let source = hook.map_or(CostSource::Ccstats, |h| h.cost_source);
    match source {
        CostSource::Cc => (cc.unwrap_or(f64::NAN), cc),
        CostSource::Auto => (cc.unwrap_or(ccstats), cc),
        CostSource::Ccstats | CostSource::Both => (ccstats, cc),
    }
}

fn format_statusline_cost(
    ccstats: f64,
    currency: Option<&CurrencyConverter>,
    hook: Option<&StatuslineHook>,
) -> String {
    let (display, cc) = resolve_costs(ccstats, hook);
    let source = hook.map_or(CostSource::Ccstats, |h| h.cost_source);
    match source {
        CostSource::Both => {
            let left = format_cost(display, currency);
            match cc {
                Some(cc) => format!("{left} ccstats / {} cc", format_cost(cc, currency)),
                None => format!("{left} ccstats / unknown cc"),
            }
        }
        CostSource::Cc if display.is_nan() => "unknown".to_string(),
        _ => format_cost(display, currency),
    }
}

fn quota_text_parts(hook: Option<&StatuslineHook>) -> Vec<String> {
    let Some(hook) = hook else {
        return Vec::new();
    };
    let mut parts = Vec::new();
    if let Some(pct) = hook.hook.context_used_pct() {
        parts.push(format!("ctx {pct:.0}%"));
    }
    if let Some(five) = hook.hook.five_hour()
        && let Some(pct) = five.used_percentage
    {
        let mut text = format!("5h {pct:.0}%");
        if let Some(reset) = five.resets_at {
            text.push_str(" ↻");
            text.push_str(&format_reset_countdown(reset, hook.captured_at));
        }
        parts.push(text);
    }
    if let Some(seven) = hook.hook.seven_day()
        && let Some(pct) = seven.used_percentage
    {
        parts.push(format!("7d {pct:.0}%"));
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_day(input: i64, output: i64, reasoning: i64, cache_c: i64, cache_r: i64) -> DayStats {
        let stats = Stats {
            input_tokens: input,
            output_tokens: output,
            reasoning_tokens: reasoning,
            cache_creation: cache_c,
            cache_creation_1h: 0,
            cache_read: cache_r,
            count: 1,
            skipped_chunks: 0,
            estimated_proxy: crate::core::CostTokens::default(),
            ..Default::default()
        };
        let mut day = DayStats {
            stats: stats.clone(),
            ..Default::default()
        };
        // Use a model name that resolves via fallback (sonnet) even with an
        // empty PricingDb, so cost-bearing tests get a real value instead of
        // the None/NaN that unknown models now produce.
        day.models.insert("sonnet-4".to_string(), stats);
        day
    }

    #[test]
    fn statusline_json_total_includes_reasoning_tokens() {
        let mut day_stats = HashMap::new();
        let mut day = DayStats {
            stats: Stats {
                input_tokens: 100,
                output_tokens: 200,
                reasoning_tokens: 50,
                cache_creation: 10,
                cache_creation_1h: 0,
                cache_read: 20,
                count: 1,
                skipped_chunks: 0,
                estimated_proxy: crate::core::CostTokens::default(),
                ..Default::default()
            },
            ..Default::default()
        };
        day.models.insert("gpt-5".to_string(), day.stats.clone());
        day_stats.insert("2026-02-06".to_string(), day);

        let json = print_statusline_json(
            &day_stats,
            &PricingDb::default(),
            "OpenAI Codex",
            NumberFormat::default(),
            None,
        );
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["reasoning_tokens"].as_i64(), Some(50));
        assert_eq!(value["total_tokens"].as_i64(), Some(380));
        assert_eq!(value["source"].as_str(), Some("OpenAI Codex"));
    }

    #[test]
    fn statusline_json_empty_stats() {
        let day_stats = HashMap::new();
        let json = print_statusline_json(
            &day_stats,
            &PricingDb::default(),
            "Claude Code",
            NumberFormat::default(),
            None,
        );
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["input_tokens"].as_i64(), Some(0));
        assert_eq!(v["output_tokens"].as_i64(), Some(0));
        assert_eq!(v["total_tokens"].as_i64(), Some(0));
        assert_eq!(v["source"].as_str(), Some("Claude Code"));
        assert_eq!(v["formatted"]["cost"].as_str(), Some("$0.00"));
        assert_eq!(v["formatted"]["input"].as_str(), Some("0"));
        assert_eq!(v["formatted"]["output"].as_str(), Some("0"));
        assert_eq!(v["confidence"].as_str(), Some("estimated"));
    }

    #[test]
    fn statusline_json_hook_marks_official_windows_and_cc_cost() {
        let hook: crate::quota::ClaudeHook = serde_json::from_str(
            r#"{
                "cost": {"total_cost_usd": 1.5},
                "context_window": {"used_percentage": 42.0},
                "rate_limits": {
                    "five_hour": {"used_percentage": 23.0, "resets_at": 4102444800}
                }
            }"#,
        )
        .unwrap();
        let captured_at = DateTime::from_timestamp(1_700_000_000, 0).unwrap();
        let json = print_statusline_json_with_quality(
            &HashMap::new(),
            &PricingDb::default(),
            "Claude Code",
            NumberFormat::default(),
            None,
            true,
            None,
            CostDisplayMode::Total,
            Some(&StatuslineHook {
                hook,
                cost_source: CostSource::Auto,
                captured_at,
            }),
        );
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        let captured = captured_at.to_rfc3339();
        assert_eq!(v["confidence"].as_str(), Some("official"));
        assert_eq!(v["context_used_pct"].as_f64(), Some(42.0));
        assert_eq!(v["cc_cost"].as_f64(), Some(1.5));
        assert_eq!(v["captured_at"].as_str(), Some(captured.as_str()));
        assert_eq!(
            v["rate_limits"]["five_hour"]["used_pct"].as_f64(),
            Some(23.0)
        );
    }

    #[test]
    fn statusline_json_aggregates_multiple_days() {
        let mut day_stats = HashMap::new();
        day_stats.insert("2026-02-10".to_string(), make_day(1000, 2000, 0, 0, 500));
        day_stats.insert("2026-02-11".to_string(), make_day(3000, 4000, 100, 50, 200));

        let json = print_statusline_json(
            &day_stats,
            &PricingDb::default(),
            "CC",
            NumberFormat::default(),
            None,
        );
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["input_tokens"].as_i64(), Some(4000));
        assert_eq!(v["output_tokens"].as_i64(), Some(6000));
        assert_eq!(v["reasoning_tokens"].as_i64(), Some(100));
        assert_eq!(v["cache_creation_tokens"].as_i64(), Some(50));
        assert_eq!(v["cache_creation_1h_tokens"].as_i64(), Some(0));
        assert_eq!(v["cache_read_tokens"].as_i64(), Some(700));
        assert_eq!(v["cache_hit_rate"].as_f64(), Some(14.74));
        assert_eq!(v["total_tokens"].as_i64(), Some(10850));
    }

    #[test]
    fn statusline_json_zero_reasoning_still_present() {
        let mut day_stats = HashMap::new();
        day_stats.insert("2026-02-12".to_string(), make_day(500, 300, 0, 0, 0));

        let json = print_statusline_json(
            &day_stats,
            &PricingDb::default(),
            "CC",
            NumberFormat::default(),
            None,
        );
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["reasoning_tokens"].as_i64(), Some(0));
        assert_eq!(v["formatted"]["reasoning"].as_str(), Some("0"));
    }

    #[test]
    fn statusline_json_formatted_uses_compact() {
        let mut day_stats = HashMap::new();
        day_stats.insert(
            "2026-02-12".to_string(),
            make_day(1_500_000, 250_000, 0, 0, 0),
        );

        let json = print_statusline_json(
            &day_stats,
            &PricingDb::default(),
            "CC",
            NumberFormat::default(),
            None,
        );
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["formatted"]["input"].as_str(), Some("1.5M"));
        assert_eq!(v["formatted"]["output"].as_str(), Some("250.0K"));
    }

    #[test]
    fn statusline_json_cost_is_valid_json_number() {
        let day_stats = HashMap::new();
        let json = print_statusline_json(
            &day_stats,
            &PricingDb::default(),
            "CC",
            NumberFormat::default(),
            None,
        );
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        // Cost should be a number (0.0), not null
        assert!(v["cost"].is_number());
    }

    #[test]
    fn statusline_json_converts_cost_when_currency_is_set() {
        let mut day_stats = HashMap::new();
        day_stats.insert(
            "2026-02-12".to_string(),
            make_day(1_000_000, 100_000, 0, 0, 0),
        );
        let converter = CurrencyConverter::from_rate_for_test("CNY", 7.0, "CNY ");

        let json = print_statusline_json(
            &day_stats,
            &PricingDb::default(),
            "CC",
            NumberFormat::default(),
            Some(&converter),
        );

        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["cost"].as_f64(), Some(31.5));
        assert_eq!(v["formatted"]["cost"].as_str(), Some("CNY 31.50"));
    }
}
