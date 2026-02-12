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
    use super::*;
    use crate::core::SessionStats;
    use std::cmp::Ordering;
    use std::collections::HashMap;

    // --- truncate_session_id ---

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
    fn truncate_session_id_small_max_len() {
        assert_eq!(truncate_session_id("abcdef", 3), "...");
        assert_eq!(truncate_session_id("abcdef", 2), "..");
        assert_eq!(truncate_session_id("abcdef", 1), ".");
        assert_eq!(truncate_session_id("abcdef", 0), "");
    }

    #[test]
    fn truncate_session_id_exact_boundary() {
        assert_eq!(truncate_session_id("abcde", 5), "abcde");
        assert_eq!(truncate_session_id("abcdef", 5), "ab...");
    }

    // --- parse_timestamp_millis ---

    #[test]
    fn parse_timestamp_millis_valid_rfc3339() {
        let ms = parse_timestamp_millis("2026-02-12T10:00:00Z");
        assert!(ms.is_some());
        assert!(ms.unwrap() > 0);
    }

    #[test]
    fn parse_timestamp_millis_invalid_input() {
        assert!(parse_timestamp_millis("not-a-timestamp").is_none());
        assert!(parse_timestamp_millis("").is_none());
        assert!(parse_timestamp_millis("2026-02-12").is_none());
    }

    // --- extract_date ---

    #[test]
    fn extract_date_valid_utc_timestamp() {
        let tz = Timezone::Named(chrono_tz::UTC);
        assert_eq!(extract_date("2026-02-12T10:30:00Z", &tz), "2026-02-12");
    }

    #[test]
    fn extract_date_fallback_on_invalid_timestamp() {
        let tz = Timezone::Named(chrono_tz::UTC);
        // Falls back to splitting on 'T'
        assert_eq!(extract_date("2026-02-12T_garbage", &tz), "2026-02-12");
    }

    #[test]
    fn extract_date_no_t_separator() {
        let tz = Timezone::Named(chrono_tz::UTC);
        assert_eq!(extract_date("just-a-string", &tz), "just-a-string");
    }

    // --- compare_session_last_timestamp ---

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

    #[test]
    fn compare_session_last_timestamp_equal() {
        let a = SessionStats {
            last_timestamp: "2026-02-06T10:00:00Z".to_string(),
            ..Default::default()
        };
        let b = SessionStats {
            last_timestamp: "2026-02-06T10:00:00Z".to_string(),
            ..Default::default()
        };
        assert_eq!(compare_session_last_timestamp(&a, &b), Ordering::Equal);
    }

    #[test]
    fn compare_session_last_timestamp_invalid_falls_back_to_string() {
        let a = SessionStats {
            last_timestamp: "aaa".to_string(),
            ..Default::default()
        };
        let b = SessionStats {
            last_timestamp: "bbb".to_string(),
            ..Default::default()
        };
        assert_eq!(compare_session_last_timestamp(&a, &b), Ordering::Less);
    }

    #[test]
    fn compare_session_valid_before_invalid() {
        let valid = SessionStats {
            last_timestamp: "2026-02-06T10:00:00Z".to_string(),
            ..Default::default()
        };
        let invalid = SessionStats {
            last_timestamp: "not-valid".to_string(),
            ..Default::default()
        };
        // Valid parses → Some, invalid → None → valid is Greater
        assert_eq!(
            compare_session_last_timestamp(&valid, &invalid),
            Ordering::Greater
        );
        assert_eq!(
            compare_session_last_timestamp(&invalid, &valid),
            Ordering::Less
        );
    }

    // --- output_session_json ---

    fn make_session(id: &str, last_ts: &str, input: i64, output: i64) -> SessionStats {
        SessionStats {
            session_id: id.to_string(),
            project_path: "/home/user/project".to_string(),
            first_timestamp: "2026-02-12T08:00:00Z".to_string(),
            last_timestamp: last_ts.to_string(),
            stats: Stats {
                input_tokens: input,
                output_tokens: output,
                ..Default::default()
            },
            models: HashMap::new(),
        }
    }

    #[test]
    fn output_session_json_empty() {
        let db = PricingDb::default();
        let json_str = output_session_json(&[], &db, SortOrder::Asc, false);
        let parsed: Vec<serde_json::Value> = serde_json::from_str(&json_str).unwrap();
        assert!(parsed.is_empty());
    }

    #[test]
    fn output_session_json_fields_present() {
        let db = PricingDb::default();
        let sessions = vec![make_session("sess-1", "2026-02-12T10:00:00Z", 1000, 500)];
        let json_str = output_session_json(&sessions, &db, SortOrder::Asc, false);
        let parsed: Vec<serde_json::Value> = serde_json::from_str(&json_str).unwrap();

        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0]["session_id"], "sess-1");
        assert_eq!(parsed[0]["project"], "project");
        assert_eq!(parsed[0]["project_path"], "/home/user/project");
        assert_eq!(parsed[0]["input_tokens"], 1000);
        assert_eq!(parsed[0]["output_tokens"], 500);
        assert_eq!(parsed[0]["total_tokens"], 1500);
        assert!(parsed[0].get("cost").is_none());
    }

    #[test]
    fn output_session_json_includes_cost_when_requested() {
        let db = PricingDb::default();
        let sessions = vec![make_session("sess-1", "2026-02-12T10:00:00Z", 100, 50)];
        let json_str = output_session_json(&sessions, &db, SortOrder::Asc, true);
        let parsed: Vec<serde_json::Value> = serde_json::from_str(&json_str).unwrap();

        assert!(parsed[0].get("cost").is_some());
    }

    #[test]
    fn output_session_json_sorts_by_timestamp() {
        let db = PricingDb::default();
        let sessions = vec![
            make_session("late", "2026-02-12T20:00:00Z", 100, 50),
            make_session("early", "2026-02-12T08:00:00Z", 200, 100),
        ];

        let asc = output_session_json(&sessions, &db, SortOrder::Asc, false);
        let parsed: Vec<serde_json::Value> = serde_json::from_str(&asc).unwrap();
        assert_eq!(parsed[0]["session_id"], "early");
        assert_eq!(parsed[1]["session_id"], "late");

        let desc = output_session_json(&sessions, &db, SortOrder::Desc, false);
        let parsed: Vec<serde_json::Value> = serde_json::from_str(&desc).unwrap();
        assert_eq!(parsed[0]["session_id"], "late");
        assert_eq!(parsed[1]["session_id"], "early");
    }

    #[test]
    fn output_session_json_models_sorted() {
        let db = PricingDb::default();
        let mut models = HashMap::new();
        models.insert("sonnet".to_string(), Stats::default());
        models.insert("haiku".to_string(), Stats::default());

        let sessions = vec![SessionStats {
            session_id: "s1".to_string(),
            models,
            ..Default::default()
        }];
        let json_str = output_session_json(&sessions, &db, SortOrder::Asc, false);
        let parsed: Vec<serde_json::Value> = serde_json::from_str(&json_str).unwrap();

        let model_list: Vec<&str> = parsed[0]["models"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(model_list, vec!["haiku", "sonnet"]);
    }
}
