use std::collections::HashMap;

use crate::data::DayStats;
use crate::output::table::format_compact;
use crate::pricing::{calculate_cost, PricingDb};

/// Output a single line suitable for statusline/tmux integration
/// Format: "CC: $X.XX | In: XM Out: XK | Today"
pub fn print_statusline(day_stats: &HashMap<String, DayStats>, pricing_db: &PricingDb) {
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
        format_compact(total_input),
        format_compact(total_output)
    );
}

/// Output statusline as JSON for programmatic consumption
pub fn print_statusline_json(day_stats: &HashMap<String, DayStats>, pricing_db: &PricingDb) -> String {
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
            "input": format_compact(total_input),
            "output": format_compact(total_output),
        }
    });

    serde_json::to_string(&output).unwrap()
}
