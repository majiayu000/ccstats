use comfy_table::{modifiers::UTF8_SOLID_INNER_BORDERS, presets::UTF8_FULL, Cell, Color, ContentArrangement, Table};
use chrono::{DateTime, Utc};

use crate::cli::SortOrder;
use crate::core::{format_project_name, SessionStats, Stats};
use crate::output::format::{
    format_compact, format_number, header_cell, normalize_header_separator, right_cell,
    styled_cell, NumberFormat,
};
use crate::utils::Timezone;
use crate::pricing::{calculate_cost, PricingDb};

/// Truncate session ID for display
fn truncate_session_id(id: &str, max_len: usize) -> String {
    if id.len() <= max_len {
        id.to_string()
    } else {
        format!("{}...", &id[..max_len - 3])
    }
}

/// Extract date from timestamp
fn extract_date(ts: &str, timezone: &Timezone) -> String {
    if let Ok(utc_dt) = ts.parse::<DateTime<Utc>>() {
        let local = timezone.to_fixed_offset(utc_dt);
        return local.date_naive().format("%Y-%m-%d").to_string();
    }
    ts.split('T').next().unwrap_or(ts).to_string()
}

pub fn print_session_table(
    sessions: &[SessionStats],
    pricing_db: &PricingDb,
    order: SortOrder,
    use_color: bool,
    compact: bool,
    show_cost: bool,
    number_format: NumberFormat,
    timezone: &Timezone,
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
        .apply_modifier(UTF8_SOLID_INNER_BORDERS)
        .set_content_arrangement(ContentArrangement::Dynamic);
    normalize_header_separator(&mut table);


    if compact {
        let mut header = vec![
            header_cell("Session", use_color),
            header_cell("Project", use_color),
            header_cell("Date", use_color),
            header_cell("Total", use_color),
        ];
        if show_cost {
            header.push(header_cell("Cost", use_color));
        }
        table.set_header(header);
    } else {
        let mut header = vec![
            header_cell("Session", use_color),
            header_cell("Project", use_color),
            header_cell("Date", use_color),
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

    for session in &sorted_sessions {
        let mut session_cost = 0.0;
        for (model, stats) in &session.models {
            session_cost += calculate_cost(stats, model, pricing_db);
        }
        total_cost += session_cost;
        total_stats.add(&session.stats);

        let session_id = truncate_session_id(&session.session_id, 12);
        let project = format_project_name(&session.project_path);
        let date = extract_date(&session.last_timestamp, timezone);

        if compact {
            let mut row = vec![
                Cell::new(&session_id),
                Cell::new(&project),
                Cell::new(&date),
                right_cell(&format_compact(session.stats.total_tokens(), number_format), None, false),
            ];
            if show_cost {
                row.push(right_cell(&format!("${:.2}", session_cost), cost_color, false));
            }
            table.add_row(row);
        } else {
            let mut row = vec![
                Cell::new(&session_id),
                Cell::new(&project),
                Cell::new(&date),
                right_cell(&format_number(session.stats.input_tokens, number_format), None, false),
                right_cell(&format_number(session.stats.output_tokens, number_format), None, false),
                right_cell(&format_number(session.stats.total_tokens(), number_format), None, false),
            ];
            if show_cost {
                row.push(right_cell(&format!("${:.2}", session_cost), cost_color, false));
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
            right_cell(&format_compact(total_stats.total_tokens(), number_format), cyan, true),
        ];
        if show_cost {
            row.push(right_cell(&format!("${:.2}", total_cost), green, true));
        }
        table.add_row(row);
    } else {
        let mut row = vec![
            styled_cell("TOTAL", cyan, true),
            Cell::new(""),
            Cell::new(""),
            right_cell(&format_number(total_stats.input_tokens, number_format), cyan, true),
            right_cell(&format_number(total_stats.output_tokens, number_format), cyan, true),
            right_cell(&format_number(total_stats.total_tokens(), number_format), cyan, true),
        ];
        if show_cost {
            row.push(right_cell(&format!("${:.2}", total_cost), green, true));
        }
        table.add_row(row);
    }

    println!("\n  Claude Code Session Usage\n");
    println!("{table}");
    println!(
        "\n  {} sessions\n",
        format_number(sorted_sessions.len() as i64, number_format)
    );
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

            let mut models: Vec<_> = session.models.keys().cloned().collect();
            models.sort();
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
                "models": models,
            });
            if show_cost {
                obj["cost"] = serde_json::json!(session_cost);
            }
            obj
        })
        .collect();

    serde_json::to_string_pretty(&output).unwrap_or_else(|e| {
        eprintln!("Failed to serialize JSON output: {}", e);
        "[]".to_string()
    })
}
