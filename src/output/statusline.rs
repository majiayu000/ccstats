use std::collections::HashMap;

use crate::core::DayStats;
use crate::output::format::{NumberFormat, cost_json_value, format_compact, format_cost};
use crate::pricing::{PricingDb, sum_model_costs};

/// Output a single line suitable for statusline/tmux integration
/// Format: "CC: $X.XX | In: XM Out: XK | Today"
pub(crate) fn print_statusline(
    day_stats: &HashMap<String, DayStats>,
    pricing_db: &PricingDb,
    source_label: &str,
    number_format: NumberFormat,
) {
    let mut total_input = 0i64;
    let mut total_output = 0i64;
    let mut total_reasoning = 0i64;
    let mut total_cost = 0.0;

    for stats in day_stats.values() {
        total_input += stats.stats.input_tokens;
        total_output += stats.stats.output_tokens;
        total_reasoning += stats.stats.reasoning_tokens;
        total_cost += sum_model_costs(&stats.models, pricing_db);
    }

    let mut parts = vec![
        format!("{}: {}", source_label, format_cost(total_cost)),
        format!(
            "In: {} Out: {}",
            format_compact(total_input, number_format),
            format_compact(total_output, number_format)
        ),
    ];
    if total_reasoning > 0 {
        parts.push(format!(
            "Reason: {}",
            format_compact(total_reasoning, number_format)
        ));
    }
    println!("{}", parts.join(" | "));
}

/// Output statusline as JSON for programmatic consumption
pub(crate) fn print_statusline_json(
    day_stats: &HashMap<String, DayStats>,
    pricing_db: &PricingDb,
    source_label: &str,
    number_format: NumberFormat,
) -> String {
    let mut total_input = 0i64;
    let mut total_output = 0i64;
    let mut total_reasoning = 0i64;
    let mut total_cache_creation = 0i64;
    let mut total_cache_read = 0i64;
    let mut total_cost = 0.0;

    for stats in day_stats.values() {
        total_input += stats.stats.input_tokens;
        total_output += stats.stats.output_tokens;
        total_reasoning += stats.stats.reasoning_tokens;
        total_cache_creation += stats.stats.cache_creation;
        total_cache_read += stats.stats.cache_read;
        total_cost += sum_model_costs(&stats.models, pricing_db);
    }

    let output = serde_json::json!({
        "source": source_label,
        "input_tokens": total_input,
        "output_tokens": total_output,
        "reasoning_tokens": total_reasoning,
        "cache_creation_tokens": total_cache_creation,
        "cache_read_tokens": total_cache_read,
        "total_tokens": total_input + total_output + total_reasoning + total_cache_creation + total_cache_read,
        "cost": cost_json_value(total_cost),
        "formatted": {
            "cost": format_cost(total_cost),
            "input": format_compact(total_input, number_format),
            "output": format_compact(total_output, number_format),
            "reasoning": format_compact(total_reasoning, number_format),
        }
    });

    serde_json::to_string(&output).unwrap_or_else(|e| {
        eprintln!("Failed to serialize JSON output: {e}");
        "{}".to_string()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{DayStats, Stats};

    fn make_day(input: i64, output: i64, reasoning: i64, cache_c: i64, cache_r: i64) -> DayStats {
        let stats = Stats {
            input_tokens: input,
            output_tokens: output,
            reasoning_tokens: reasoning,
            cache_creation: cache_c,
            cache_read: cache_r,
            count: 1,
            skipped_chunks: 0,
        };
        let mut day = DayStats {
            stats: stats.clone(),
            ..Default::default()
        };
        day.models.insert("test-model".to_string(), stats);
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
        );
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["reasoning_tokens"].as_i64(), Some(0));
        assert_eq!(v["formatted"]["reasoning"].as_str(), Some("0"));
    }

    #[test]
    fn statusline_json_formatted_uses_compact() {
        let mut day_stats = HashMap::new();
        day_stats.insert("2026-02-12".to_string(), make_day(1_500_000, 250_000, 0, 0, 0));

        let json = print_statusline_json(
            &day_stats,
            &PricingDb::default(),
            "CC",
            NumberFormat::default(),
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
        );
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        // Cost should be a number (0.0), not null
        assert!(v["cost"].is_number());
    }
}
