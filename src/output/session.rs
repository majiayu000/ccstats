use chrono::{DateTime, Utc};
use comfy_table::{
    Cell, Color, ContentArrangement, Table, modifiers::UTF8_SOLID_INNER_BORDERS, presets::UTF8_FULL,
};
use std::cmp::Ordering;

use crate::cli::SortOrder;
use crate::consts::DATE_FORMAT;
use crate::core::{SessionStats, Stats, format_project_name};
use crate::output::format::{
    NumberFormat, cost_json_value, format_compact, format_cost, format_number, header_cell,
    normalize_header_separator, right_cell, styled_cell,
};
use crate::pricing::{PricingDb, sum_model_costs};
use crate::utils::Timezone;

/// Truncate session ID for display
fn truncate_session_id(id: &str, max_len: usize) -> String {
    if id.chars().count() <= max_len {
        id.to_string()
    } else if max_len <= 3 {
        ".".repeat(max_len)
    } else {
        let prefix: String = id.chars().take(max_len - 3).collect();
        format!("{}...", prefix)
    }
}

/// Extract date from timestamp
fn extract_date(ts: &str, timezone: &Timezone) -> String {
    if let Ok(utc_dt) = ts.parse::<DateTime<Utc>>() {
        let local = timezone.to_fixed_offset(utc_dt);
        return local.date_naive().format(DATE_FORMAT).to_string();
    }
    ts.split('T').next().unwrap_or(ts).to_string()
}

fn parse_timestamp_millis(ts: &str) -> Option<i64> {
    DateTime::parse_from_rfc3339(ts)
        .ok()
        .map(|dt| dt.timestamp_millis())
}

fn compare_session_last_timestamp(a: &SessionStats, b: &SessionStats) -> Ordering {
    match (
        parse_timestamp_millis(&a.last_timestamp),
        parse_timestamp_millis(&b.last_timestamp),
    ) {
        (Some(a_ms), Some(b_ms)) => a_ms
            .cmp(&b_ms)
            .then_with(|| a.last_timestamp.cmp(&b.last_timestamp)),
        (Some(_), None) => Ordering::Greater,
        (None, Some(_)) => Ordering::Less,
        (None, None) => a.last_timestamp.cmp(&b.last_timestamp),
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct SessionTableOptions<'a> {
    pub(crate) order: SortOrder,
    pub(crate) use_color: bool,
    pub(crate) compact: bool,
    pub(crate) show_cost: bool,
    pub(crate) number_format: NumberFormat,
    pub(crate) source_label: &'a str,
    pub(crate) timezone: Timezone,
}

pub(crate) fn print_session_table(
    sessions: &[SessionStats],
    pricing_db: &PricingDb,
    options: SessionTableOptions<'_>,
) {
    let order = options.order;
    let use_color = options.use_color;
    let compact = options.compact;
    let show_cost = options.show_cost;
    let number_format = options.number_format;
    let source_label = options.source_label;
    let timezone = options.timezone;

    let mut sorted_sessions: Vec<_> = sessions.iter().collect();

    // Sort by last timestamp
    match order {
        SortOrder::Asc => sorted_sessions.sort_by(|a, b| compare_session_last_timestamp(a, b)),
        SortOrder::Desc => sorted_sessions.sort_by(|a, b| compare_session_last_timestamp(b, a)),
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
        let session_cost = sum_model_costs(&session.models, pricing_db);
        total_cost += session_cost;
        total_stats.add(&session.stats);

        let session_id = truncate_session_id(&session.session_id, 12);
        let project = format_project_name(&session.project_path);
        let date = extract_date(&session.last_timestamp, &timezone);

        if compact {
            let mut row = vec![
                Cell::new(&session_id),
                Cell::new(&project),
                Cell::new(&date),
                right_cell(
                    &format_compact(session.stats.total_tokens(), number_format),
                    None,
                    false,
                ),
            ];
            if show_cost {
                row.push(right_cell(&format_cost(session_cost), cost_color, false));
            }
            table.add_row(row);
        } else {
            let mut row = vec![
                Cell::new(&session_id),
                Cell::new(&project),
                Cell::new(&date),
                right_cell(
                    &format_number(session.stats.input_tokens, number_format),
                    None,
                    false,
                ),
                right_cell(
                    &format_number(session.stats.output_tokens, number_format),
                    None,
                    false,
                ),
                right_cell(
                    &format_number(session.stats.total_tokens(), number_format),
                    None,
                    false,
                ),
            ];
            if show_cost {
                row.push(right_cell(&format_cost(session_cost), cost_color, false));
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
            Cell::new(""),
            Cell::new(""),
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

    println!("\n  {} Session Usage\n", source_label);
    println!("{table}");
    println!(
        "\n  {} sessions\n",
        format_number(sorted_sessions.len() as i64, number_format)
    );
}

pub(crate) fn output_session_json(
    sessions: &[SessionStats],
    pricing_db: &PricingDb,
    order: SortOrder,
    show_cost: bool,
) -> String {
    let mut sorted_sessions: Vec<_> = sessions.iter().collect();

    match order {
        SortOrder::Asc => sorted_sessions.sort_by(|a, b| compare_session_last_timestamp(a, b)),
        SortOrder::Desc => sorted_sessions.sort_by(|a, b| compare_session_last_timestamp(b, a)),
    }

    let output: Vec<serde_json::Value> = sorted_sessions
        .iter()
        .map(|session| {
            let session_cost = sum_model_costs(&session.models, pricing_db);

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
                "reasoning_tokens": session.stats.reasoning_tokens,
                "cache_creation_tokens": session.stats.cache_creation,
                "cache_read_tokens": session.stats.cache_read,
                "total_tokens": session.stats.total_tokens(),
                "models": models,
            });
            if show_cost {
                obj["cost"] = cost_json_value(session_cost);
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
    use super::{compare_session_last_timestamp, truncate_session_id};
    use crate::core::SessionStats;
    use std::cmp::Ordering;

    #[test]
    fn truncate_session_id_ascii() {
        assert_eq!(truncate_session_id("abcdefghijk", 12), "abcdefghijk");
        assert_eq!(truncate_session_id("abcdefghijkl", 12), "abcdefghijkl");
        assert_eq!(truncate_session_id("abcdefghijklmnop", 12), "abcdefghi...");
    }

    #[test]
    fn truncate_session_id_utf8_boundary_safe() {
        assert_eq!(truncate_session_id("ééééééé", 12), "ééééééé");
        assert_eq!(truncate_session_id("ééééééé", 6), "ééé...");
    }

    #[test]
    fn compare_session_last_timestamp_uses_absolute_time() {
        let a = SessionStats {
            last_timestamp: "2026-02-06T23:00:00+08:00".to_string(), // 15:00Z
            ..Default::default()
        };
        let b = SessionStats {
            last_timestamp: "2026-02-06T16:00:00Z".to_string(), // 16:00Z
            ..Default::default()
        };

        assert_eq!(compare_session_last_timestamp(&a, &b), Ordering::Less);
        assert_eq!(compare_session_last_timestamp(&b, &a), Ordering::Greater);
    }
}
