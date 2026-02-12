use comfy_table::{Cell, Color};

use crate::cli::SortOrder;
use crate::core::{BlockStats, Stats};
use crate::output::format::{
    NumberFormat, cost_json_value, create_styled_table, format_compact, format_cost, format_number,
    header_cell, right_cell, styled_cell,
};
use crate::pricing::{PricingDb, sum_model_costs};

#[derive(Debug, Clone, Copy)]
pub(crate) struct BlockTableOptions<'a> {
    pub(crate) order: SortOrder,
    pub(crate) use_color: bool,
    pub(crate) compact: bool,
    pub(crate) show_cost: bool,
    pub(crate) source_label: &'a str,
    pub(crate) number_format: NumberFormat,
}

pub(crate) fn print_block_table(
    blocks: &[BlockStats],
    pricing_db: &PricingDb,
    options: BlockTableOptions<'_>,
) {
    let order = options.order;
    let use_color = options.use_color;
    let compact = options.compact;
    let show_cost = options.show_cost;
    let source_label = options.source_label;
    let number_format = options.number_format;

    let mut sorted_blocks: Vec<_> = blocks.iter().collect();

    match order {
        SortOrder::Asc => sorted_blocks.sort_by(|a, b| a.block_start.cmp(&b.block_start)),
        SortOrder::Desc => sorted_blocks.sort_by(|a, b| b.block_start.cmp(&a.block_start)),
    }

    let mut table = create_styled_table();

    if compact {
        let mut header = vec![
            header_cell("Block", use_color),
            header_cell("Total", use_color),
        ];
        if show_cost {
            header.push(header_cell("Cost", use_color));
        }
        table.set_header(header);
    } else {
        let mut header = vec![
            header_cell("Block", use_color),
            header_cell("Input", use_color),
            header_cell("Output", use_color),
            header_cell("Cache Create", use_color),
            header_cell("Cache Read", use_color),
            header_cell("Total", use_color),
        ];
        if show_cost {
            header.push(header_cell("Cost", use_color));
        }
        table.set_header(header);
    }

    let cost_color = if use_color { Some(Color::Green) } else { None };

    let mut total_stats = Stats::default();
    let mut total_cost = 0.0;

    for block in &sorted_blocks {
        let block_cost = sum_model_costs(&block.models, pricing_db);
        total_cost += block_cost;
        total_stats.add(&block.stats);

        let block_label = format!("{} - {}", block.block_start, block.block_end);

        if compact {
            let mut row = vec![
                Cell::new(&block_label),
                right_cell(
                    &format_compact(block.stats.total_tokens(), number_format),
                    None,
                    false,
                ),
            ];
            if show_cost {
                row.push(right_cell(&format_cost(block_cost), cost_color, false));
            }
            table.add_row(row);
        } else {
            let mut row = vec![
                Cell::new(&block_label),
                right_cell(
                    &format_number(block.stats.input_tokens, number_format),
                    None,
                    false,
                ),
                right_cell(
                    &format_number(block.stats.output_tokens, number_format),
                    None,
                    false,
                ),
                right_cell(
                    &format_number(block.stats.cache_creation, number_format),
                    None,
                    false,
                ),
                right_cell(
                    &format_number(block.stats.cache_read, number_format),
                    None,
                    false,
                ),
                right_cell(
                    &format_number(block.stats.total_tokens(), number_format),
                    None,
                    false,
                ),
            ];
            if show_cost {
                row.push(right_cell(&format_cost(block_cost), cost_color, false));
            }
            table.add_row(row);
        }
    }

    let cyan = if use_color { Some(Color::Cyan) } else { None };
    let green = if use_color { Some(Color::Green) } else { None };

    // Add total row
    if compact {
        let mut row = vec![
            styled_cell("TOTAL", cyan, true),
            right_cell(
                &format_compact(total_stats.total_tokens(), number_format),
                cyan,
                true,
            ),
        ];
        if show_cost {
            row.push(right_cell(&format_cost(total_cost), green, true));
        }
        table.add_row(row);
    } else {
        let mut row = vec![
            styled_cell("TOTAL", cyan, true),
            right_cell(
                &format_number(total_stats.input_tokens, number_format),
                cyan,
                true,
            ),
            right_cell(
                &format_number(total_stats.output_tokens, number_format),
                cyan,
                true,
            ),
            right_cell(
                &format_number(total_stats.cache_creation, number_format),
                cyan,
                true,
            ),
            right_cell(
                &format_number(total_stats.cache_read, number_format),
                cyan,
                true,
            ),
            right_cell(
                &format_number(total_stats.total_tokens(), number_format),
                cyan,
                true,
            ),
        ];
        if show_cost {
            row.push(right_cell(&format_cost(total_cost), green, true));
        }
        table.add_row(row);
    }

    println!("\n  {source_label} 5-Hour Billing Blocks\n");
    println!("{table}");
    println!(
        "\n  {} blocks\n",
        format_number(sorted_blocks.len() as i64, number_format)
    );
}

pub(crate) fn output_block_json(
    blocks: &[BlockStats],
    pricing_db: &PricingDb,
    order: SortOrder,
    show_cost: bool,
) -> String {
    let mut sorted_blocks: Vec<_> = blocks.iter().collect();

    match order {
        SortOrder::Asc => sorted_blocks.sort_by(|a, b| a.block_start.cmp(&b.block_start)),
        SortOrder::Desc => sorted_blocks.sort_by(|a, b| b.block_start.cmp(&a.block_start)),
    }

    let output: Vec<serde_json::Value> = sorted_blocks
        .iter()
        .map(|block| {
            let block_cost = sum_model_costs(&block.models, pricing_db);

            let mut models: Vec<_> = block.models.keys().cloned().collect();
            models.sort();
            let mut obj = serde_json::json!({
                "block_start": block.block_start,
                "block_end": block.block_end,
                "input_tokens": block.stats.input_tokens,
                "output_tokens": block.stats.output_tokens,
                "cache_creation_tokens": block.stats.cache_creation,
                "cache_read_tokens": block.stats.cache_read,
                "total_tokens": block.stats.total_tokens(),
                "models": models,
            });
            if show_cost {
                obj["cost"] = cost_json_value(block_cost);
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
    use std::collections::HashMap;

    fn make_block(start: &str, end: &str, input: i64, output: i64) -> BlockStats {
        BlockStats {
            block_start: start.to_string(),
            block_end: end.to_string(),
            stats: Stats {
                input_tokens: input,
                output_tokens: output,
                ..Default::default()
            },
            models: HashMap::new(),
        }
    }

    fn make_block_with_cache(
        start: &str,
        end: &str,
        input: i64,
        output: i64,
        cache_creation: i64,
        cache_read: i64,
    ) -> BlockStats {
        BlockStats {
            block_start: start.to_string(),
            block_end: end.to_string(),
            stats: Stats {
                input_tokens: input,
                output_tokens: output,
                cache_creation,
                cache_read,
                ..Default::default()
            },
            models: HashMap::new(),
        }
    }

    // --- JSON output tests ---

    #[test]
    fn output_block_json_empty_input() {
        let db = PricingDb::default();
        let json_str = output_block_json(&[], &db, SortOrder::Asc, false);
        let parsed: Vec<serde_json::Value> = serde_json::from_str(&json_str).unwrap();
        assert!(parsed.is_empty());
    }

    #[test]
    fn output_block_json_single_block() {
        let db = PricingDb::default();
        let blocks = vec![make_block("2026-02-12 10:00", "2026-02-12 15:00", 1000, 500)];
        let json_str = output_block_json(&blocks, &db, SortOrder::Asc, false);
        let parsed: Vec<serde_json::Value> = serde_json::from_str(&json_str).unwrap();

        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0]["block_start"], "2026-02-12 10:00");
        assert_eq!(parsed[0]["block_end"], "2026-02-12 15:00");
        assert_eq!(parsed[0]["input_tokens"], 1000);
        assert_eq!(parsed[0]["output_tokens"], 500);
        assert_eq!(parsed[0]["total_tokens"], 1500);
        assert!(parsed[0].get("cost").is_none());
    }

    #[test]
    fn output_block_json_includes_cost_when_requested() {
        let db = PricingDb::default();
        let blocks = vec![make_block("2026-02-12 10:00", "2026-02-12 15:00", 100, 50)];
        let json_str = output_block_json(&blocks, &db, SortOrder::Asc, true);
        let parsed: Vec<serde_json::Value> = serde_json::from_str(&json_str).unwrap();

        assert!(parsed[0].get("cost").is_some());
    }

    #[test]
    fn output_block_json_sorts_asc() {
        let db = PricingDb::default();
        let blocks = vec![
            make_block("2026-02-12 15:00", "2026-02-12 20:00", 100, 50),
            make_block("2026-02-12 05:00", "2026-02-12 10:00", 200, 100),
            make_block("2026-02-12 10:00", "2026-02-12 15:00", 300, 150),
        ];
        let json_str = output_block_json(&blocks, &db, SortOrder::Asc, false);
        let parsed: Vec<serde_json::Value> = serde_json::from_str(&json_str).unwrap();

        assert_eq!(parsed[0]["block_start"], "2026-02-12 05:00");
        assert_eq!(parsed[1]["block_start"], "2026-02-12 10:00");
        assert_eq!(parsed[2]["block_start"], "2026-02-12 15:00");
    }

    #[test]
    fn output_block_json_sorts_desc() {
        let db = PricingDb::default();
        let blocks = vec![
            make_block("2026-02-12 05:00", "2026-02-12 10:00", 100, 50),
            make_block("2026-02-12 15:00", "2026-02-12 20:00", 200, 100),
        ];
        let json_str = output_block_json(&blocks, &db, SortOrder::Desc, false);
        let parsed: Vec<serde_json::Value> = serde_json::from_str(&json_str).unwrap();

        assert_eq!(parsed[0]["block_start"], "2026-02-12 15:00");
        assert_eq!(parsed[1]["block_start"], "2026-02-12 05:00");
    }

    #[test]
    fn output_block_json_includes_cache_tokens() {
        let db = PricingDb::default();
        let blocks = vec![make_block_with_cache(
            "2026-02-12 10:00",
            "2026-02-12 15:00",
            1000,
            500,
            200,
            300,
        )];
        let json_str = output_block_json(&blocks, &db, SortOrder::Asc, false);
        let parsed: Vec<serde_json::Value> = serde_json::from_str(&json_str).unwrap();

        assert_eq!(parsed[0]["cache_creation_tokens"], 200);
        assert_eq!(parsed[0]["cache_read_tokens"], 300);
        assert_eq!(parsed[0]["total_tokens"], 2000); // 1000+500+200+300
    }

    #[test]
    fn output_block_json_models_sorted_alphabetically() {
        let db = PricingDb::default();
        let mut models = HashMap::new();
        models.insert("sonnet".to_string(), Stats::default());
        models.insert("opus".to_string(), Stats::default());
        models.insert("haiku".to_string(), Stats::default());

        let blocks = vec![BlockStats {
            block_start: "2026-02-12 10:00".to_string(),
            block_end: "2026-02-12 15:00".to_string(),
            stats: Stats::default(),
            models,
        }];
        let json_str = output_block_json(&blocks, &db, SortOrder::Asc, false);
        let parsed: Vec<serde_json::Value> = serde_json::from_str(&json_str).unwrap();

        let model_list: Vec<&str> = parsed[0]["models"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(model_list, vec!["haiku", "opus", "sonnet"]);
    }
}
