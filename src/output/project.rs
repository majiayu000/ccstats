use comfy_table::{Cell, Color};

use crate::cli::SortOrder;
use crate::core::{ProjectStats, Stats};
use crate::output::format::{
    NumberFormat, cost_json_value, create_styled_table, format_compact, format_cost, format_number,
    header_cell, right_cell, styled_cell,
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

    let mut table = create_styled_table();

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

    println!("\n  {source_label} Project Usage\n");
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
        eprintln!("Failed to serialize JSON output: {e}");
        "[]".to_string()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

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

    fn make_project(name: &str, path: &str, sessions: usize, input: i64, output: i64) -> ProjectStats {
        ProjectStats {
            project_name: name.to_string(),
            project_path: path.to_string(),
            session_count: sessions,
            stats: Stats { input_tokens: input, output_tokens: output, count: 1, ..Default::default() },
            models: HashMap::from([("claude".to_string(), Stats { input_tokens: input, output_tokens: output, count: 1, ..Default::default() })]),
        }
    }

    #[test]
    fn output_project_json_empty() {
        let db = PricingDb::default();
        let result = output_project_json(&[], &db, SortOrder::Desc, false);
        assert_eq!(result.trim(), "[]");
    }

    #[test]
    fn output_project_json_single_project() {
        let db = PricingDb::default();
        let projects = vec![make_project("myapp", "/path/myapp", 3, 1000, 500)];
        let result = output_project_json(&projects, &db, SortOrder::Desc, false);
        let parsed: Vec<serde_json::Value> = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0]["project"], "myapp");
        assert_eq!(parsed[0]["project_path"], "/path/myapp");
        assert_eq!(parsed[0]["session_count"], 3);
        assert_eq!(parsed[0]["input_tokens"], 1000);
        assert_eq!(parsed[0]["output_tokens"], 500);
        assert_eq!(parsed[0]["total_tokens"], 1500);
        assert!(parsed[0].get("cost").is_none());
    }

    #[test]
    fn output_project_json_with_cost() {
        let db = PricingDb::default();
        let projects = vec![make_project("app", "/app", 1, 100, 50)];
        let result = output_project_json(&projects, &db, SortOrder::Desc, true);
        let parsed: Vec<serde_json::Value> = serde_json::from_str(&result).unwrap();
        assert!(parsed[0].get("cost").is_some());
    }

    #[test]
    fn output_project_json_models_sorted() {
        let db = PricingDb::default();
        let projects = vec![ProjectStats {
            project_name: "app".to_string(),
            project_path: "/app".to_string(),
            session_count: 1,
            stats: Stats { input_tokens: 300, count: 2, ..Default::default() },
            models: HashMap::from([
                ("gpt-4".to_string(), Stats { input_tokens: 100, count: 1, ..Default::default() }),
                ("claude".to_string(), Stats { input_tokens: 200, count: 1, ..Default::default() }),
            ]),
        }];
        let result = output_project_json(&projects, &db, SortOrder::Desc, false);
        let parsed: Vec<serde_json::Value> = serde_json::from_str(&result).unwrap();
        let models = parsed[0]["models"].as_array().unwrap();
        assert_eq!(models[0], "claude");
        assert_eq!(models[1], "gpt-4");
    }

    #[test]
    fn output_project_json_sort_order() {
        let db = PricingDb::default();
        let projects = vec![
            make_project("small", "/small", 1, 10, 5),
            make_project("big", "/big", 1, 1000, 500),
        ];
        // Desc: big first (higher cost)
        let desc = output_project_json(&projects, &db, SortOrder::Desc, false);
        let parsed: Vec<serde_json::Value> = serde_json::from_str(&desc).unwrap();
        // With empty pricing DB, all costs are 0.0, so order is stable
        assert_eq!(parsed.len(), 2);

        // Asc: same with empty pricing
        let asc = output_project_json(&projects, &db, SortOrder::Asc, false);
        let parsed_asc: Vec<serde_json::Value> = serde_json::from_str(&asc).unwrap();
        assert_eq!(parsed_asc.len(), 2);
    }
}
