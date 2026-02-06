use comfy_table::{modifiers::UTF8_SOLID_INNER_BORDERS, presets::UTF8_FULL, Cell, Color, ContentArrangement, Table};

use crate::cli::SortOrder;
use crate::core::{ProjectStats, Stats};
use crate::output::format::{
    format_compact, format_number, header_cell, normalize_header_separator, right_cell,
    styled_cell, NumberFormat,
};
use crate::pricing::{calculate_cost, PricingDb};
use std::cmp::Ordering;

fn compare_cost(a: f64, b: f64) -> Ordering {
    if a.is_nan() && b.is_nan() {
        Ordering::Equal
    } else if a.is_nan() {
        Ordering::Greater
    } else if b.is_nan() {
        Ordering::Less
    } else {
        a.partial_cmp(&b).unwrap_or(Ordering::Equal)
    }
}

pub(crate) fn print_project_table(
    projects: &[ProjectStats],
    pricing_db: &PricingDb,
    order: SortOrder,
    use_color: bool,
    compact: bool,
    show_cost: bool,
    source_label: &str,
    number_format: NumberFormat,
) {
    let mut sorted_projects: Vec<_> = projects.iter().collect();

    // Sort by cost (default) or name
    match order {
        SortOrder::Asc => sorted_projects.sort_by(|a, b| {
            let cost_a: f64 = a.models.iter().map(|(m, s)| calculate_cost(s, m, pricing_db)).sum();
            let cost_b: f64 = b.models.iter().map(|(m, s)| calculate_cost(s, m, pricing_db)).sum();
            compare_cost(cost_a, cost_b)
        }),
        SortOrder::Desc => sorted_projects.sort_by(|a, b| {
            let cost_a: f64 = a.models.iter().map(|(m, s)| calculate_cost(s, m, pricing_db)).sum();
            let cost_b: f64 = b.models.iter().map(|(m, s)| calculate_cost(s, m, pricing_db)).sum();
            compare_cost(cost_b, cost_a)
        }),
    }

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .apply_modifier(UTF8_SOLID_INNER_BORDERS)
        .set_content_arrangement(ContentArrangement::Dynamic);
    normalize_header_separator(&mut table);


    if compact {
        let mut header = vec![
            header_cell("Project", use_color),
            header_cell("Sessions", use_color),
            header_cell("Total", use_color),
        ];
        if show_cost {
            header.push(header_cell("Cost", use_color));
        }
        table.set_header(header);
    } else {
        let mut header = vec![
            header_cell("Project", use_color),
            header_cell("Sessions", use_color),
            header_cell("Input", use_color),
            header_cell("Output", use_color),
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
    let mut total_sessions = 0usize;

    for project in &sorted_projects {
        let mut project_cost = 0.0;
        for (model, stats) in &project.models {
            project_cost += calculate_cost(stats, model, pricing_db);
        }
        total_cost += project_cost;
        total_stats.add(&project.stats);
        total_sessions += project.session_count;

        if compact {
            let mut row = vec![
                Cell::new(&project.project_name),
                right_cell(
                    &format_number(project.session_count as i64, number_format),
                    None,
                    false,
                ),
                right_cell(
                    &format_compact(project.stats.total_tokens(), number_format),
                    None,
                    false,
                ),
            ];
            if show_cost {
                row.push(right_cell(&format!("${:.2}", project_cost), cost_color, false));
            }
            table.add_row(row);
        } else {
            let mut row = vec![
                Cell::new(&project.project_name),
                right_cell(
                    &format_number(project.session_count as i64, number_format),
                    None,
                    false,
                ),
                right_cell(&format_number(project.stats.input_tokens, number_format), None, false),
                right_cell(&format_number(project.stats.output_tokens, number_format), None, false),
                right_cell(&format_number(project.stats.total_tokens(), number_format), None, false),
            ];
            if show_cost {
                row.push(right_cell(&format!("${:.2}", project_cost), cost_color, false));
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
            right_cell(&format_number(total_sessions as i64, number_format), cyan, true),
            right_cell(&format_compact(total_stats.total_tokens(), number_format), cyan, true),
        ];
        if show_cost {
            row.push(right_cell(&format!("${:.2}", total_cost), green, true));
        }
        table.add_row(row);
    } else {
        let mut row = vec![
            styled_cell("TOTAL", cyan, true),
            right_cell(&format_number(total_sessions as i64, number_format), cyan, true),
            right_cell(&format_number(total_stats.input_tokens, number_format), cyan, true),
            right_cell(&format_number(total_stats.output_tokens, number_format), cyan, true),
            right_cell(&format_number(total_stats.total_tokens(), number_format), cyan, true),
        ];
        if show_cost {
            row.push(right_cell(&format!("${:.2}", total_cost), green, true));
        }
        table.add_row(row);
    }

    println!("\n  {} Project Usage\n", source_label);
    println!("{table}");
    println!(
        "\n  {} projects, {} sessions\n",
        format_number(sorted_projects.len() as i64, number_format),
        format_number(total_sessions as i64, number_format)
    );
}

pub(crate) fn output_project_json(
    projects: &[ProjectStats],
    pricing_db: &PricingDb,
    order: SortOrder,
    show_cost: bool,
) -> String {
    let mut sorted_projects: Vec<_> = projects.iter().collect();

    match order {
        SortOrder::Asc => sorted_projects.sort_by(|a, b| {
            let cost_a: f64 = a.models.iter().map(|(m, s)| calculate_cost(s, m, pricing_db)).sum();
            let cost_b: f64 = b.models.iter().map(|(m, s)| calculate_cost(s, m, pricing_db)).sum();
            compare_cost(cost_a, cost_b)
        }),
        SortOrder::Desc => sorted_projects.sort_by(|a, b| {
            let cost_a: f64 = a.models.iter().map(|(m, s)| calculate_cost(s, m, pricing_db)).sum();
            let cost_b: f64 = b.models.iter().map(|(m, s)| calculate_cost(s, m, pricing_db)).sum();
            compare_cost(cost_b, cost_a)
        }),
    }

    let output: Vec<serde_json::Value> = sorted_projects
        .iter()
        .map(|project| {
            let mut project_cost = 0.0;
            for (model, stats) in &project.models {
                project_cost += calculate_cost(stats, model, pricing_db);
            }

            let mut models: Vec<_> = project.models.keys().cloned().collect();
            models.sort();
            let mut obj = serde_json::json!({
                "project": project.project_name,
                "project_path": project.project_path,
                "session_count": project.session_count,
                "input_tokens": project.stats.input_tokens,
                "output_tokens": project.stats.output_tokens,
                "cache_creation_tokens": project.stats.cache_creation,
                "cache_read_tokens": project.stats.cache_read,
                "total_tokens": project.stats.total_tokens(),
                "models": models,
            });
            if show_cost {
                obj["cost"] = serde_json::json!(project_cost);
            }
            obj
        })
        .collect();

    serde_json::to_string_pretty(&output).unwrap_or_else(|e| {
        eprintln!("Failed to serialize JSON output: {}", e);
        "[]".to_string()
    })
}
