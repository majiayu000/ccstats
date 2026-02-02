use comfy_table::{presets::UTF8_FULL, Attribute, Cell, Color, ContentArrangement, Table};

use crate::cli::SortOrder;
use crate::data::{SessionStats, Stats};
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

/// Truncate session ID for display
fn truncate_session_id(id: &str, max_len: usize) -> String {
    if id.len() <= max_len {
        id.to_string()
    } else {
        format!("{}...", &id[..max_len - 3])
    }
}

/// Extract readable project name from path
fn format_project_name(path: &str) -> String {
    // Convert "-Users-apple-Desktop-code-AI-tool-ccstats" to "ccstats"
    path.split('-').last().unwrap_or(path).to_string()
}

/// Extract date from timestamp
fn extract_date(ts: &str) -> String {
    ts.split('T').next().unwrap_or(ts).to_string()
}

pub fn print_session_table(
    sessions: &[SessionStats],
    pricing_db: &PricingDb,
    order: SortOrder,
    use_color: bool,
    compact: bool,
    show_cost: bool,
) {
    let mut sorted_sessions: Vec<_> = sessions.iter().collect();

    // Sort by last timestamp
    match order {
        SortOrder::Asc => sorted_sessions.sort_by(|a, b| a.last_timestamp.cmp(&b.last_timestamp)),
        SortOrder::Desc => sorted_sessions.sort_by(|a, b| b.last_timestamp.cmp(&a.last_timestamp)),
    }

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic);

    if compact {
        let mut header = vec![
            Cell::new("Session").add_attribute(Attribute::Bold),
            Cell::new("Project").add_attribute(Attribute::Bold),
            Cell::new("Date").add_attribute(Attribute::Bold),
            Cell::new("Total").add_attribute(Attribute::Bold),
        ];
        if show_cost {
            header.push(Cell::new("Cost").add_attribute(Attribute::Bold));
        }
        table.set_header(header);
    } else {
        let mut header = vec![
            Cell::new("Session").add_attribute(Attribute::Bold),
            Cell::new("Project").add_attribute(Attribute::Bold),
            Cell::new("Date").add_attribute(Attribute::Bold),
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

    for session in &sorted_sessions {
        let mut session_cost = 0.0;
        for (model, stats) in &session.models {
            session_cost += calculate_cost(stats, model, pricing_db);
        }
        total_cost += session_cost;
        total_stats.add(&session.stats);

        let session_id = truncate_session_id(&session.session_id, 12);
        let project = format_project_name(&session.project_path);
        let date = extract_date(&session.last_timestamp);

        if compact {
            let mut row = vec![
                Cell::new(&session_id),
                Cell::new(&project),
                Cell::new(&date),
                Cell::new(format_compact(session.stats.total_tokens())),
            ];
            if show_cost {
                row.push(Cell::new(format!("${:.2}", session_cost)));
            }
            table.add_row(row);
        } else {
            let mut row = vec![
                Cell::new(&session_id),
                Cell::new(&project),
                Cell::new(&date),
                Cell::new(format_number(session.stats.input_tokens)),
                Cell::new(format_number(session.stats.output_tokens)),
                Cell::new(format_number(session.stats.total_tokens())),
            ];
            if show_cost {
                row.push(Cell::new(format!("${:.2}", session_cost)));
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
            Cell::new(""),
            Cell::new(""),
            styled_cell(&format_compact(total_stats.total_tokens()), cyan, false),
        ];
        if show_cost {
            row.push(styled_cell(&format!("${:.2}", total_cost), green, true));
        }
        table.add_row(row);
    } else {
        let mut row = vec![
            styled_cell("TOTAL", cyan, true),
            Cell::new(""),
            Cell::new(""),
            styled_cell(&format_number(total_stats.input_tokens), cyan, false),
            styled_cell(&format_number(total_stats.output_tokens), cyan, false),
            styled_cell(&format_number(total_stats.total_tokens()), cyan, false),
        ];
        if show_cost {
            row.push(styled_cell(&format!("${:.2}", total_cost), green, true));
        }
        table.add_row(row);
    }

    println!("\n  Claude Code Session Usage\n");
    println!("{table}");
    println!("\n  {} sessions\n", sorted_sessions.len());
}

pub fn output_session_json(
    sessions: &[SessionStats],
    pricing_db: &PricingDb,
    order: SortOrder,
    show_cost: bool,
) -> String {
    let mut sorted_sessions: Vec<_> = sessions.iter().collect();

    match order {
        SortOrder::Asc => sorted_sessions.sort_by(|a, b| a.last_timestamp.cmp(&b.last_timestamp)),
        SortOrder::Desc => sorted_sessions.sort_by(|a, b| b.last_timestamp.cmp(&a.last_timestamp)),
    }

    let output: Vec<serde_json::Value> = sorted_sessions
        .iter()
        .map(|session| {
            let mut session_cost = 0.0;
            for (model, stats) in &session.models {
                session_cost += calculate_cost(stats, model, pricing_db);
            }

            let mut obj = serde_json::json!({
                "session_id": session.session_id,
                "project": format_project_name(&session.project_path),
                "project_path": session.project_path,
                "first_timestamp": session.first_timestamp,
                "last_timestamp": session.last_timestamp,
                "input_tokens": session.stats.input_tokens,
                "output_tokens": session.stats.output_tokens,
                "cache_creation_tokens": session.stats.cache_creation,
                "cache_read_tokens": session.stats.cache_read,
                "total_tokens": session.stats.total_tokens(),
                "models": session.models.keys().collect::<Vec<_>>(),
            });
            if show_cost {
                obj["cost"] = serde_json::json!(session_cost);
            }
            obj
        })
        .collect();

    serde_json::to_string_pretty(&output).unwrap()
}
