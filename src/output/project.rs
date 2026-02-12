use comfy_table::{
    Cell, Color, ContentArrangement, Table, modifiers::UTF8_SOLID_INNER_BORDERS, presets::UTF8_FULL,
};

use crate::cli::SortOrder;
use crate::core::{ProjectStats, Stats};
use crate::output::format::{
    NumberFormat, cost_json_value, format_compact, format_cost, format_number, header_cell,
    normalize_header_separator, right_cell, styled_cell,
};
use crate::pricing::{PricingDb, attach_costs};
use std::cmp::Ordering;

#[derive(Debug, Clone, Copy)]
pub(crate) struct ProjectTableOptions<'a> {
    pub(crate) order: SortOrder,
    pub(crate) use_color: bool,
    pub(crate) compact: bool,
    pub(crate) show_cost: bool,
    pub(crate) source_label: &'a str,
    pub(crate) number_format: NumberFormat,
}

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
    options: ProjectTableOptions<'_>,
) {
    let order = options.order;
    let use_color = options.use_color;
    let compact = options.compact;
    let show_cost = options.show_cost;
    let source_label = options.source_label;
    let number_format = options.number_format;

    let mut sorted_projects = attach_costs(projects, |p| &p.models, pricing_db);

    // Sort by cost (default) or name
    match order {
        SortOrder::Asc => sorted_projects.sort_by(|a, b| compare_cost(a.cost, b.cost)),
        SortOrder::Desc => sorted_projects.sort_by(|a, b| compare_cost(b.cost, a.cost)),
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

    for costed in &sorted_projects {
        let project = costed.item;
        let project_cost = costed.cost;
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
                row.push(right_cell(&format_cost(project_cost), cost_color, false));
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
                right_cell(
                    &format_number(project.stats.input_tokens, number_format),
                    None,
                    false,
                ),
                right_cell(
                    &format_number(project.stats.output_tokens, number_format),
                    None,
                    false,
                ),
                right_cell(
                    &format_number(project.stats.total_tokens(), number_format),
                    None,
                    false,
                ),
            ];
            if show_cost {
                row.push(right_cell(&format_cost(project_cost), cost_color, false));
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
                &format_number(total_sessions as i64, number_format),
                cyan,
                true,
            ),
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
                &format_number(total_sessions as i64, number_format),
                cyan,
                true,
            ),
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
    let mut sorted_projects = attach_costs(projects, |p| &p.models, pricing_db);

    match order {
        SortOrder::Asc => sorted_projects.sort_by(|a, b| compare_cost(a.cost, b.cost)),
        SortOrder::Desc => sorted_projects.sort_by(|a, b| compare_cost(b.cost, a.cost)),
    }

    let output: Vec<serde_json::Value> = sorted_projects
        .iter()
        .map(|costed| {
            let project = costed.item;
            let project_cost = costed.cost;

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
                obj["cost"] = cost_json_value(project_cost);
            }
            obj
        })
        .collect();

    serde_json::to_string_pretty(&output).unwrap_or_else(|e| {
        eprintln!("Failed to serialize JSON output: {}", e);
        "[]".to_string()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compare_cost_normal_values() {
        assert_eq!(compare_cost(1.0, 2.0), Ordering::Less);
        assert_eq!(compare_cost(2.0, 1.0), Ordering::Greater);
        assert_eq!(compare_cost(1.0, 1.0), Ordering::Equal);
    }

    #[test]
    fn compare_cost_nan_handling() {
        assert_eq!(compare_cost(f64::NAN, f64::NAN), Ordering::Equal);
        assert_eq!(compare_cost(f64::NAN, 1.0), Ordering::Greater);
        assert_eq!(compare_cost(1.0, f64::NAN), Ordering::Less);
    }

    #[test]
    fn compare_cost_zero_and_negative() {
        assert_eq!(compare_cost(0.0, 0.0), Ordering::Equal);
        assert_eq!(compare_cost(-1.0, 1.0), Ordering::Less);
    }
}
