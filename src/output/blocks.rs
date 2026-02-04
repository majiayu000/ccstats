use comfy_table::{presets::UTF8_FULL, Attribute, Cell, Color, ContentArrangement, Table};

use crate::cli::SortOrder;
use crate::data::{BlockStats, Stats};
use crate::output::table::{format_compact, format_number, styled_cell};
use crate::pricing::{calculate_cost, PricingDb};

pub fn print_block_table(
    blocks: &[BlockStats],
    pricing_db: &PricingDb,
    order: SortOrder,
    use_color: bool,
    compact: bool,
    show_cost: bool,
) {
    let mut sorted_blocks: Vec<_> = blocks.iter().collect();

    match order {
        SortOrder::Asc => sorted_blocks.sort_by(|a, b| a.block_start.cmp(&b.block_start)),
        SortOrder::Desc => sorted_blocks.sort_by(|a, b| b.block_start.cmp(&a.block_start)),
    }

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic);

    if compact {
        let mut header = vec![
            Cell::new("Block").add_attribute(Attribute::Bold),
            Cell::new("Total").add_attribute(Attribute::Bold),
        ];
        if show_cost {
            header.push(Cell::new("Cost").add_attribute(Attribute::Bold));
        }
        table.set_header(header);
    } else {
        let mut header = vec![
            Cell::new("Block").add_attribute(Attribute::Bold),
            Cell::new("Input").add_attribute(Attribute::Bold),
            Cell::new("Output").add_attribute(Attribute::Bold),
            Cell::new("Cache Create").add_attribute(Attribute::Bold),
            Cell::new("Cache Read").add_attribute(Attribute::Bold),
            Cell::new("Total").add_attribute(Attribute::Bold),
        ];
        if show_cost {
            header.push(Cell::new("Cost").add_attribute(Attribute::Bold));
        }
        table.set_header(header);
    }

    let mut total_stats = Stats::default();
    let mut total_cost = 0.0;

    for block in &sorted_blocks {
        let mut block_cost = 0.0;
        for (model, stats) in &block.models {
            block_cost += calculate_cost(stats, model, pricing_db);
        }
        total_cost += block_cost;
        total_stats.add(&block.stats);

        let block_label = format!("{} - {}", block.block_start, block.block_end);

        if compact {
            let mut row = vec![
                Cell::new(&block_label),
                Cell::new(format_compact(block.stats.total_tokens())),
            ];
            if show_cost {
                row.push(Cell::new(format!("${:.2}", block_cost)));
            }
            table.add_row(row);
        } else {
            let mut row = vec![
                Cell::new(&block_label),
                Cell::new(format_number(block.stats.input_tokens)),
                Cell::new(format_number(block.stats.output_tokens)),
                Cell::new(format_number(block.stats.cache_creation)),
                Cell::new(format_number(block.stats.cache_read)),
                Cell::new(format_number(block.stats.total_tokens())),
            ];
            if show_cost {
                row.push(Cell::new(format!("${:.2}", block_cost)));
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
            styled_cell(&format_compact(total_stats.total_tokens()), cyan, false),
        ];
        if show_cost {
            row.push(styled_cell(&format!("${:.2}", total_cost), green, true));
        }
        table.add_row(row);
    } else {
        let mut row = vec![
            styled_cell("TOTAL", cyan, true),
            styled_cell(&format_number(total_stats.input_tokens), cyan, false),
            styled_cell(&format_number(total_stats.output_tokens), cyan, false),
            styled_cell(&format_number(total_stats.cache_creation), cyan, false),
            styled_cell(&format_number(total_stats.cache_read), cyan, false),
            styled_cell(&format_number(total_stats.total_tokens()), cyan, false),
        ];
        if show_cost {
            row.push(styled_cell(&format!("${:.2}", total_cost), green, true));
        }
        table.add_row(row);
    }

    println!("\n  Claude Code 5-Hour Billing Blocks\n");
    println!("{table}");
    println!("\n  {} blocks\n", sorted_blocks.len());
}

pub fn output_block_json(
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
            let mut block_cost = 0.0;
            for (model, stats) in &block.models {
                block_cost += calculate_cost(stats, model, pricing_db);
            }

            let mut obj = serde_json::json!({
                "block_start": block.block_start,
                "block_end": block.block_end,
                "input_tokens": block.stats.input_tokens,
                "output_tokens": block.stats.output_tokens,
                "cache_creation_tokens": block.stats.cache_creation,
                "cache_read_tokens": block.stats.cache_read,
                "total_tokens": block.stats.total_tokens(),
                "models": block.models.keys().collect::<Vec<_>>(),
            });
            if show_cost {
                obj["cost"] = serde_json::json!(block_cost);
            }
            obj
        })
        .collect();

    serde_json::to_string_pretty(&output).unwrap()
}
