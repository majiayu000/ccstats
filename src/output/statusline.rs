use std::collections::HashMap;

use crate::core::DayStats;
use crate::output::format::{format_compact, NumberFormat};
use crate::pricing::{calculate_cost, PricingDb};

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

    for (_date, stats) in day_stats {
        total_input += stats.stats.input_tokens;
        total_output += stats.stats.output_tokens;
        total_reasoning += stats.stats.reasoning_tokens;

        for (model, model_stats) in &stats.models {
            total_cost += calculate_cost(model_stats, model, pricing_db);
        }
    }

    let mut parts = vec![
        format!("{}: ${:.2}", source_label, total_cost),
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

    for (_date, stats) in day_stats {
        total_input += stats.stats.input_tokens;
        total_output += stats.stats.output_tokens;
        total_reasoning += stats.stats.reasoning_tokens;
        total_cache_creation += stats.stats.cache_creation;
        total_cache_read += stats.stats.cache_read;

        for (model, model_stats) in &stats.models {
            total_cost += calculate_cost(model_stats, model, pricing_db);
        }
    }

    let output = serde_json::json!({
        "source": source_label,
        "input_tokens": total_input,
        "output_tokens": total_output,
        "reasoning_tokens": total_reasoning,
        "cache_creation_tokens": total_cache_creation,
        "cache_read_tokens": total_cache_read,
        "total_tokens": total_input + total_output + total_reasoning + total_cache_creation + total_cache_read,
        "cost": total_cost,
        "formatted": {
            "cost": format!("${:.2}", total_cost),
            "input": format_compact(total_input, number_format),
            "output": format_compact(total_output, number_format),
            "reasoning": format_compact(total_reasoning, number_format),
        }
    });

    serde_json::to_string(&output).unwrap_or_else(|e| {
        eprintln!("Failed to serialize JSON output: {}", e);
        "{}".to_string()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{DayStats, Stats};

    #[test]
    fn statusline_json_total_includes_reasoning_tokens() {
        let mut day_stats = HashMap::new();
        let mut day = DayStats::default();
        day.stats = Stats {
            input_tokens: 100,
            output_tokens: 200,
            reasoning_tokens: 50,
            cache_creation: 10,
            cache_read: 20,
            count: 1,
            skipped_chunks: 0,
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
}
