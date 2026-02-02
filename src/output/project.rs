use comfy_table::{presets::UTF8_FULL, Attribute, Cell, Color, ContentArrangement, Table};

use crate::cli::SortOrder;
use crate::data::{ProjectStats, Stats};
use crate::output::table::format_number;
use crate::pricing::{calculate_cost, PricingDb};

/// Format number in compact form (K, M, B suffixes)
fn format_compact(n: i64) -> String {
    if n >= 1_000_000_000 {
        format!("{:.1}B", n as f64 / 1_000_000_000.0)
    } else if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

fn styled_cell(text: &str, color: Option<Color>, bold: bool) -> Cell {
    let mut cell = Cell::new(text);
    if let Some(c) = color {
        cell = cell.fg(c);
    }
    if bold {
        cell = cell.add_attribute(Attribute::Bold);
    }
    cell
}

pub fn print_project_table(
    projects: &[ProjectStats],
    pricing_db: &PricingDb,
    order: SortOrder,
    use_color: bool,
    compact: bool,
    show_cost: bool,
) {
    let mut sorted_projects: Vec<_> = projects.iter().collect();

    // Sort by cost (default) or name
    match order {
        SortOrder::Asc => sorted_projects.sort_by(|a, b| {
            let cost_a: f64 = a.models.iter().map(|(m, s)| calculate_cost(s, m, pricing_db)).sum();
            let cost_b: f64 = b.models.iter().map(|(m, s)| calculate_cost(s, m, pricing_db)).sum();
            cost_a.partial_cmp(&cost_b).unwrap()
        }),
        SortOrder::Desc => sorted_projects.sort_by(|a, b| {
            let cost_a: f64 = a.models.iter().map(|(m, s)| calculate_cost(s, m, pricing_db)).sum();
            let cost_b: f64 = b.models.iter().map(|(m, s)| calculate_cost(s, m, pricing_db)).sum();
            cost_b.partial_cmp(&cost_a).unwrap()
        }),
    }

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic);

    if compact {
        let mut header = vec![
            Cell::new("Project").add_attribute(Attribute::Bold),
            Cell::new("Sessions").add_attribute(Attribute::Bold),
            Cell::new("Total").add_attribute(Attribute::Bold),
        ];
        if show_cost {
            header.push(Cell::new("Cost").add_attribute(Attribute::Bold));
        }
        table.set_header(header);
    } else {
        let mut header = vec![
            Cell::new("Project").add_attribute(Attribute::Bold),
            Cell::new("Sessions").add_attribute(Attribute::Bold),
            Cell::new("Input").add_attribute(Attribute::Bold),
            Cell::new("Output").add_attribute(Attribute::Bold),
            Cell::new("Total").add_attribute(Attribute::Bold),
        ];
        if show_cost {
            header.push(Cell::new("Cost").add_attribute(Attribute::Bold));
        }
        table.set_header(header);
    }

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
                Cell::new(project.session_count.to_string()),
                Cell::new(format_compact(project.stats.total_tokens())),
            ];
            if show_cost {
                row.push(Cell::new(format!("${:.2}", project_cost)));
            }
            table.add_row(row);
        } else {
            let mut row = vec![
                Cell::new(&project.project_name),
                Cell::new(project.session_count.to_string()),
                Cell::new(format_number(project.stats.input_tokens)),
                Cell::new(format_number(project.stats.output_tokens)),
                Cell::new(format_number(project.stats.total_tokens())),
            ];
            if show_cost {
                row.push(Cell::new(format!("${:.2}", project_cost)));
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
            styled_cell(&total_sessions.to_string(), cyan, false),
            styled_cell(&format_compact(total_stats.total_tokens()), cyan, false),
        ];
        if show_cost {
            row.push(styled_cell(&format!("${:.2}", total_cost), green, true));
        }
        table.add_row(row);
    } else {
        let mut row = vec![
            styled_cell("TOTAL", cyan, true),
            styled_cell(&total_sessions.to_string(), cyan, false),
            styled_cell(&format_number(total_stats.input_tokens), cyan, false),
            styled_cell(&format_number(total_stats.output_tokens), cyan, false),
            styled_cell(&format_number(total_stats.total_tokens()), cyan, false),
        ];
        if show_cost {
            row.push(styled_cell(&format!("${:.2}", total_cost), green, true));
        }
        table.add_row(row);
    }

    println!("\n  Claude Code Project Usage\n");
    println!("{table}");
    println!("\n  {} projects, {} sessions\n", sorted_projects.len(), total_sessions);
}

pub fn output_project_json(
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
            cost_a.partial_cmp(&cost_b).unwrap()
        }),
        SortOrder::Desc => sorted_projects.sort_by(|a, b| {
            let cost_a: f64 = a.models.iter().map(|(m, s)| calculate_cost(s, m, pricing_db)).sum();
            let cost_b: f64 = b.models.iter().map(|(m, s)| calculate_cost(s, m, pricing_db)).sum();
            cost_b.partial_cmp(&cost_a).unwrap()
        }),
    }

    let output: Vec<serde_json::Value> = sorted_projects
        .iter()
        .map(|project| {
            let mut project_cost = 0.0;
            for (model, stats) in &project.models {
                project_cost += calculate_cost(stats, model, pricing_db);
            }

            let mut obj = serde_json::json!({
                "project": project.project_name,
                "project_path": project.project_path,
                "session_count": project.session_count,
                "input_tokens": project.stats.input_tokens,
                "output_tokens": project.stats.output_tokens,
                "cache_creation_tokens": project.stats.cache_creation,
                "cache_read_tokens": project.stats.cache_read,
                "total_tokens": project.stats.total_tokens(),
                "models": project.models.keys().collect::<Vec<_>>(),
            });
            if show_cost {
                obj["cost"] = serde_json::json!(project_cost);
            }
            obj
        })
        .collect();

    serde_json::to_string_pretty(&output).unwrap()
}
