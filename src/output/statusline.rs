use std::collections::HashMap;

use crate::data::DayStats;
use crate::output::format::{format_compact, NumberFormat};
use crate::pricing::{calculate_cost, PricingDb};

/// Output a single line suitable for statusline/tmux integration
/// Format: "CC: $X.XX | In: XM Out: XK | Today"
pub fn print_statusline(
    day_stats: &HashMap<String, DayStats>,
    pricing_db: &PricingDb,
    number_format: NumberFormat,
) {
    let mut total_input = 0i64;
    let mut total_output = 0i64;
    let mut total_cost = 0.0;

    for (_date, stats) in day_stats {
        total_input += stats.stats.input_tokens;
        total_output += stats.stats.output_tokens;

        for (model, model_stats) in &stats.models {
            total_cost += calculate_cost(model_stats, model, pricing_db);
        }
    }

    // Simple format for statusline
    println!(
        "CC: ${:.2} | In: {} Out: {}",
        total_cost,
        format_compact(total_input, number_format),
        format_compact(total_output, number_format)
    );
}

/// Output statusline as JSON for programmatic consumption
pub fn print_statusline_json(
    day_stats: &HashMap<String, DayStats>,
    pricing_db: &PricingDb,
    number_format: NumberFormat,
) -> String {
    let mut total_input = 0i64;
    let mut total_output = 0i64;
    let mut total_cache_creation = 0i64;
    let mut total_cache_read = 0i64;
    let mut total_cost = 0.0;

    for (_date, stats) in day_stats {
        total_input += stats.stats.input_tokens;
        total_output += stats.stats.output_tokens;
        total_cache_creation += stats.stats.cache_creation;
        total_cache_read += stats.stats.cache_read;

        for (model, model_stats) in &stats.models {
            total_cost += calculate_cost(model_stats, model, pricing_db);
        }
    }

    let output = serde_json::json!({
        "input_tokens": total_input,
        "output_tokens": total_output,
        "cache_creation_tokens": total_cache_creation,
        "cache_read_tokens": total_cache_read,
        "total_tokens": total_input + total_output + total_cache_creation + total_cache_read,
        "cost": total_cost,
        "formatted": {
            "cost": format!("${:.2}", total_cost),
            "input": format_compact(total_input, number_format),
            "output": format_compact(total_output, number_format),
        }
    });

    serde_json::to_string(&output).unwrap_or_else(|e| {
        eprintln!("Failed to serialize JSON output: {}", e);
        "{}".to_string()
    })
}
