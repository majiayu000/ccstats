use std::collections::HashMap;

use crate::core::{DataQuality, DayStats, Stats};
use crate::output::format::{NumberFormat, cost_json_value, format_compact, format_cost};
use crate::output::pricing_meta;
use crate::pricing::{
    CostDisplayMode, CurrencyConverter, PricingDb, sum_display_model_costs,
    sum_estimated_proxy_model_costs,
};

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
pub(crate) fn print_statusline(
    day_stats: &HashMap<String, DayStats>,
    pricing_db: &PricingDb,
    source_label: &str,
    number_format: NumberFormat,
    currency: Option<&CurrencyConverter>,
    cost_mode: CostDisplayMode,
) {
    let t = aggregate_totals(day_stats, pricing_db, cost_mode);

    let mut parts = vec![
        format!("{}: {}", source_label, format_cost(t.cost, currency)),
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
        None,
        CostDisplayMode::Total,
    )
}

pub(crate) fn print_statusline_json_with_quality(
    day_stats: &HashMap<String, DayStats>,
    pricing_db: &PricingDb,
    source_label: &str,
    number_format: NumberFormat,
    currency: Option<&CurrencyConverter>,
    data_quality: Option<DataQuality>,
    cost_mode: CostDisplayMode,
) -> String {
    let t = aggregate_totals(day_stats, pricing_db, cost_mode);

    let mut output = serde_json::json!({
        "source": source_label,
        "input_tokens": t.stats.input_tokens,
        "output_tokens": t.stats.output_tokens,
        "reasoning_tokens": t.stats.reasoning_tokens,
        "cache_creation_tokens": t.stats.cache_creation,
        "cache_read_tokens": t.stats.cache_read,
        "total_tokens": t.stats.total_tokens(),
        "cost": cost_json_value(t.cost, currency),
        "formatted": {
            "cost": format_cost(t.cost, currency),
            "input": format_compact(t.stats.input_tokens, number_format),
            "output": format_compact(t.stats.output_tokens, number_format),
            "reasoning": format_compact(t.stats.reasoning_tokens, number_format),
        }
    });
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
        output["cost_kind"] = serde_json::json!(t.stats.cost_kind().as_str());
        output["estimated_cost"] = cost_json_value(t.estimated_proxy_cost, currency);
    }

    serde_json::to_string(&output).unwrap_or_else(|e| {
        eprintln!("Failed to serialize JSON output: {e}");
        "{}".to_string()
    })
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
            cache_read: cache_r,
            count: 1,
            skipped_chunks: 0,
            estimated_proxy: crate::core::CostTokens::default(),
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
                cache_read: 20,
                count: 1,
                skipped_chunks: 0,
                estimated_proxy: crate::core::CostTokens::default(),
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
        assert_eq!(v["cache_read_tokens"].as_i64(), Some(700));
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
