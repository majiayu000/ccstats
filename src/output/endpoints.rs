//! Output for per-endpoint (native vs proxy) usage breakdown.

use comfy_table::{Cell, Color};

use crate::core::{EndpointStats, Stats};
use crate::output::format::{
    NumberFormat, cost_json_value, create_styled_table, format_cost, format_number, header_cell,
    right_cell,
};
use crate::output::pricing_meta;
use crate::pricing::{CurrencyConverter, PricingDb, sum_model_costs};

#[derive(Debug, Clone, Copy)]
pub(crate) struct EndpointTableOptions<'a> {
    pub(crate) use_color: bool,
    pub(crate) show_cost: bool,
    pub(crate) source_label: &'a str,
    pub(crate) number_format: NumberFormat,
    pub(crate) currency: Option<&'a CurrencyConverter>,
}

/// Average input tokens per call — the key signal that distinguishes native
/// (well-cached, small raw input) from proxy (full context billed as input).
fn avg_input_per_call(stats: &Stats) -> i64 {
    if stats.count <= 0 {
        0
    } else {
        stats.input_tokens / stats.count
    }
}

#[allow(clippy::too_many_lines)]
pub(crate) fn print_endpoint_table(
    endpoints: &[EndpointStats],
    pricing_db: &PricingDb,
    options: EndpointTableOptions<'_>,
) {
    let use_color = options.use_color;
    let show_cost = options.show_cost;
    let number_format = options.number_format;

    let mut table = create_styled_table();
    let mut header = vec![
        header_cell("Endpoint", use_color),
        header_cell("Calls", use_color),
        header_cell("Input", use_color),
        header_cell("Output", use_color),
        header_cell("Cache Create", use_color),
        header_cell("Cache Read", use_color),
        header_cell("Total", use_color),
        header_cell("Avg In/Call", use_color),
    ];
    if show_cost {
        header.push(header_cell("Cost", use_color));
    }
    table.set_header(header);

    let cost_color = if use_color { Some(Color::Green) } else { None };
    let mut total_stats = Stats::default();
    let mut total_cost = 0.0;

    for ep in endpoints {
        let ep_cost = sum_model_costs(&ep.models, pricing_db);
        total_cost += ep_cost;
        total_stats.add(&ep.stats);

        let mut row = vec![
            Cell::new(ep.endpoint.as_str()),
            right_cell(&format_number(ep.stats.count, number_format), None, false),
            right_cell(
                &format_number(ep.stats.input_tokens, number_format),
                None,
                false,
            ),
            right_cell(
                &format_number(ep.stats.output_tokens, number_format),
                None,
                false,
            ),
            right_cell(
                &format_number(ep.stats.cache_creation, number_format),
                None,
                false,
            ),
            right_cell(
                &format_number(ep.stats.cache_read, number_format),
                None,
                false,
            ),
            right_cell(
                &format_number(ep.stats.total_tokens(), number_format),
                None,
                false,
            ),
            right_cell(
                &format_number(avg_input_per_call(&ep.stats), number_format),
                None,
                false,
            ),
        ];
        if show_cost {
            row.push(right_cell(
                &format_cost(ep_cost, options.currency),
                cost_color,
                false,
            ));
        }
        table.add_row(row);
    }

    // Total row
    let mut total_row = vec![
        Cell::new("TOTAL"),
        right_cell(&format_number(total_stats.count, number_format), None, true),
        right_cell(
            &format_number(total_stats.input_tokens, number_format),
            None,
            true,
        ),
        right_cell(
            &format_number(total_stats.output_tokens, number_format),
            None,
            true,
        ),
        right_cell(
            &format_number(total_stats.cache_creation, number_format),
            None,
            true,
        ),
        right_cell(
            &format_number(total_stats.cache_read, number_format),
            None,
            true,
        ),
        right_cell(
            &format_number(total_stats.total_tokens(), number_format),
            None,
            true,
        ),
        right_cell(
            &format_number(avg_input_per_call(&total_stats), number_format),
            None,
            true,
        ),
    ];
    if show_cost {
        total_row.push(right_cell(
            &format_cost(total_cost, options.currency),
            cost_color,
            true,
        ));
    }
    table.add_row(total_row);

    println!("\n  {} Usage by Endpoint\n", options.source_label);
    println!("{table}");
    if show_cost
        && let Some(note) =
            pricing_meta::note_for_maps(endpoints.iter().map(|ep| &ep.models), pricing_db)
    {
        println!("\n  {note}");
    }
    println!(
        "\n  proxy = third-party gateway (does not report cache creation; bills full context as input)\n"
    );
}

pub(crate) fn output_endpoint_json(
    endpoints: &[EndpointStats],
    pricing_db: &PricingDb,
    show_cost: bool,
    currency: Option<&CurrencyConverter>,
) -> String {
    let output: Vec<serde_json::Value> = endpoints
        .iter()
        .map(|ep| {
            let ep_cost = sum_model_costs(&ep.models, pricing_db);
            let mut models: Vec<_> = ep.models.keys().cloned().collect();
            models.sort();
            let mut obj = serde_json::json!({
                "endpoint": ep.endpoint.as_str(),
                "calls": ep.stats.count,
                "input_tokens": ep.stats.input_tokens,
                "output_tokens": ep.stats.output_tokens,
                "cache_creation_tokens": ep.stats.cache_creation,
                "cache_read_tokens": ep.stats.cache_read,
                "total_tokens": ep.stats.total_tokens(),
                "avg_input_per_call": avg_input_per_call(&ep.stats),
                "models": models,
            });
            if show_cost {
                obj["cost"] = cost_json_value(ep_cost, currency);
                pricing_meta::add_json(&mut obj, &ep.models, pricing_db);
            }
            obj
        })
        .collect();

    serde_json::to_string_pretty(&output).unwrap_or_else(|e| {
        eprintln!("Failed to serialize JSON output: {e}");
        "[]".to_string()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::Endpoint;
    use std::collections::HashMap;

    fn make_endpoint(endpoint: Endpoint, input: i64, count: i64) -> EndpointStats {
        EndpointStats {
            endpoint,
            stats: Stats {
                input_tokens: input,
                count,
                ..Default::default()
            },
            models: HashMap::new(),
        }
    }

    #[test]
    fn avg_input_handles_zero_count() {
        let stats = Stats {
            input_tokens: 100,
            count: 0,
            ..Default::default()
        };
        assert_eq!(avg_input_per_call(&stats), 0);
    }

    #[test]
    fn avg_input_divides_by_call_count() {
        let stats = Stats {
            input_tokens: 1000,
            count: 4,
            ..Default::default()
        };
        assert_eq!(avg_input_per_call(&stats), 250);
    }

    #[test]
    fn json_contains_endpoint_and_avg_fields() {
        let db = PricingDb::default();
        let endpoints = vec![
            make_endpoint(Endpoint::Native, 1000, 10),
            make_endpoint(Endpoint::Proxy, 5000, 5),
        ];
        let json = output_endpoint_json(&endpoints, &db, false, None);
        assert!(json.contains("\"endpoint\": \"native\""));
        assert!(json.contains("\"endpoint\": \"proxy\""));
        assert!(json.contains("\"avg_input_per_call\": 100"));
        assert!(json.contains("\"avg_input_per_call\": 1000"));
    }
}
