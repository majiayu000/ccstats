use std::collections::HashMap;
use std::fmt::Write;

use crate::cli::SortOrder;
use crate::core::{BlockStats, DayStats, ProjectStats, SessionStats};
use crate::output::period::{Period, aggregate_day_stats_by_period};
use crate::output::format::compare_cost;
use crate::pricing::{PricingDb, calculate_cost, sum_model_costs};

fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') || s.contains('\r') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

pub(crate) fn output_period_csv(
    day_stats: &HashMap<String, DayStats>,
    period: Period,
    pricing_db: &PricingDb,
    order: SortOrder,
    breakdown: bool,
    show_cost: bool,
) -> String {
    let aggregated;
    let stats_ref = if period == Period::Day {
        day_stats
    } else {
        aggregated = aggregate_day_stats_by_period(day_stats, period);
        &aggregated
    };

    let mut rows: Vec<_> = stats_ref.iter().collect();
    match order {
        SortOrder::Asc => rows.sort_by(|a, b| a.0.cmp(b.0)),
        SortOrder::Desc => rows.sort_by(|a, b| b.0.cmp(a.0)),
    }
    let label = period.label();
    let mut out = String::new();

    if breakdown {
        // Breakdown: one row per model per period
        let _ = write!(
            out,
            "{label},model,input_tokens,output_tokens,reasoning_tokens,cache_creation_tokens,cache_read_tokens,total_tokens"
        );
        if show_cost {
            let _ = write!(out, ",cost");
        }
        out.push('\n');

        for (key, stats) in &rows {
            let mut models: Vec<_> = stats.models.iter().collect();
            models.sort_by_key(|(name, _)| name.as_str());
            for (model, model_stats) in &models {
                let _ = write!(
                    out,
                    "{},{},{},{},{},{},{},{}",
                    csv_escape(key),
                    csv_escape(model),
                    model_stats.input_tokens,
                    model_stats.output_tokens,
                    model_stats.reasoning_tokens,
                    model_stats.cache_creation,
                    model_stats.cache_read,
                    model_stats.total_tokens(),
                );
                if show_cost {
                    let cost = calculate_cost(model_stats, model, pricing_db);
                    let _ = write!(out, ",{cost:.6}");
                }
                out.push('\n');
            }
        }
    } else {
        // Standard: one row per period
        let _ = write!(
            out,
            "{label},input_tokens,output_tokens,reasoning_tokens,cache_creation_tokens,cache_read_tokens,total_tokens"
        );
        if show_cost {
            let _ = write!(out, ",cost");
        }
        out.push('\n');

        for (key, stats) in &rows {
            let cost = sum_model_costs(&stats.models, pricing_db);
            let _ = write!(
                out,
                "{},{},{},{},{},{},{}",
                csv_escape(key),
                stats.stats.input_tokens,
                stats.stats.output_tokens,
                stats.stats.reasoning_tokens,
                stats.stats.cache_creation,
                stats.stats.cache_read,
                stats.stats.total_tokens(),
            );
            if show_cost {
                let _ = write!(out, ",{cost:.6}");
            }
            out.push('\n');
        }
    }

    out
}

pub(crate) fn output_session_csv(
    sessions: &[SessionStats],
    pricing_db: &PricingDb,
    order: SortOrder,
    show_cost: bool,
) -> String {
    let mut sorted: Vec<_> = sessions.iter().collect();
    match order {
        SortOrder::Asc => sorted.sort_by(|a, b| a.last_timestamp.cmp(&b.last_timestamp)),
        SortOrder::Desc => sorted.sort_by(|a, b| b.last_timestamp.cmp(&a.last_timestamp)),
    }

    let mut out = String::new();
    let _ = write!(
        out,
        "session_id,project_path,first_timestamp,last_timestamp,input_tokens,output_tokens,total_tokens"
    );
    if show_cost {
        let _ = write!(out, ",cost");
    }
    out.push('\n');

    for s in &sorted {
        let cost = sum_model_costs(&s.models, pricing_db);
        let _ = write!(
            out,
            "{},{},{},{},{},{},{}",
            csv_escape(&s.session_id),
            csv_escape(&s.project_path),
            csv_escape(&s.first_timestamp),
            csv_escape(&s.last_timestamp),
            s.stats.input_tokens,
            s.stats.output_tokens,
            s.stats.total_tokens(),
        );
        if show_cost {
            let _ = write!(out, ",{cost:.6}");
        }
        out.push('\n');
    }

    out
}
pub(crate) fn output_project_csv(
    projects: &[ProjectStats],
    pricing_db: &PricingDb,
    order: SortOrder,
    show_cost: bool,
) -> String {
    let mut sorted: Vec<_> = projects.iter().collect();
    match order {
        SortOrder::Asc => sorted.sort_by(|a, b| {
            compare_cost(
                sum_model_costs(&a.models, pricing_db),
                sum_model_costs(&b.models, pricing_db),
            )
        }),
        SortOrder::Desc => sorted.sort_by(|a, b| {
            compare_cost(
                sum_model_costs(&b.models, pricing_db),
                sum_model_costs(&a.models, pricing_db),
            )
        }),
    }

    let mut out = String::new();
    let _ = write!(
        out,
        "project_name,project_path,sessions,input_tokens,output_tokens,total_tokens"
    );
    if show_cost {
        let _ = write!(out, ",cost");
    }
    out.push('\n');

    for p in &sorted {
        let cost = sum_model_costs(&p.models, pricing_db);
        let _ = write!(
            out,
            "{},{},{},{},{},{}",
            csv_escape(&p.project_name),
            csv_escape(&p.project_path),
            p.session_count,
            p.stats.input_tokens,
            p.stats.output_tokens,
            p.stats.total_tokens(),
        );
        if show_cost {
            let _ = write!(out, ",{cost:.6}");
        }
        out.push('\n');
    }

    out
}

pub(crate) fn output_block_csv(
    blocks: &[BlockStats],
    pricing_db: &PricingDb,
    order: SortOrder,
    show_cost: bool,
) -> String {
    let mut sorted: Vec<_> = blocks.iter().collect();
    match order {
        SortOrder::Asc => sorted.sort_by(|a, b| a.block_start.cmp(&b.block_start)),
        SortOrder::Desc => sorted.sort_by(|a, b| b.block_start.cmp(&a.block_start)),
    }

    let mut out = String::new();
    let _ = write!(
        out,
        "block_start,block_end,input_tokens,output_tokens,cache_creation_tokens,cache_read_tokens,total_tokens"
    );
    if show_cost {
        let _ = write!(out, ",cost");
    }
    out.push('\n');

    for b in &sorted {
        let cost = sum_model_costs(&b.models, pricing_db);
        let _ = write!(
            out,
            "{},{},{},{},{},{},{}",
            csv_escape(&b.block_start),
            csv_escape(&b.block_end),
            b.stats.input_tokens,
            b.stats.output_tokens,
            b.stats.cache_creation,
            b.stats.cache_read,
            b.stats.total_tokens(),
        );
        if show_cost {
            let _ = write!(out, ",{cost:.6}");
        }
        out.push('\n');
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::Stats;

    fn make_day_stats(models: &[(&str, i64)]) -> DayStats {
        let mut ds = DayStats::default();
        for &(model, tokens) in models {
            let stats = Stats {
                input_tokens: tokens,
                output_tokens: tokens / 2,
                count: 1,
                ..Default::default()
            };
            ds.add_stats(model.to_string(), &stats);
        }
        ds
    }

    #[test]
    fn csv_escape_plain() {
        assert_eq!(csv_escape("hello"), "hello");
    }

    #[test]
    fn csv_escape_comma() {
        assert_eq!(csv_escape("a,b"), "\"a,b\"");
    }

    #[test]
    fn csv_escape_quotes() {
        assert_eq!(csv_escape("say \"hi\""), "\"say \"\"hi\"\"\"");
    }

    #[test]
    fn period_csv_daily_no_cost() {
        let mut day_stats = HashMap::new();
        day_stats.insert(
            "2025-01-01".to_string(),
            make_day_stats(&[("sonnet", 1000)]),
        );

        let db = PricingDb::default();
        let csv = output_period_csv(&day_stats, Period::Day, &db, SortOrder::Asc, false, false);

        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(
            lines[0],
            "date,input_tokens,output_tokens,reasoning_tokens,cache_creation_tokens,cache_read_tokens,total_tokens"
        );
        assert!(lines[1].starts_with("2025-01-01,1000,500,"));
        assert!(!lines[0].contains("cost"));
    }

    #[test]
    fn period_csv_daily_with_cost() {
        let mut day_stats = HashMap::new();
        day_stats.insert(
            "2025-01-01".to_string(),
            make_day_stats(&[("sonnet", 1000)]),
        );

        let db = PricingDb::default();
        let csv = output_period_csv(&day_stats, Period::Day, &db, SortOrder::Asc, false, true);

        let lines: Vec<&str> = csv.lines().collect();
        assert!(lines[0].ends_with(",cost"));
        assert_eq!(lines.len(), 2); // header + 1 row
    }

    #[test]
    fn period_csv_sort_desc() {
        let mut day_stats = HashMap::new();
        day_stats.insert("2025-01-01".to_string(), make_day_stats(&[("sonnet", 100)]));
        day_stats.insert("2025-01-02".to_string(), make_day_stats(&[("sonnet", 200)]));

        let db = PricingDb::default();
        let csv = output_period_csv(&day_stats, Period::Day, &db, SortOrder::Desc, false, false);

        let lines: Vec<&str> = csv.lines().collect();
        assert!(lines[1].starts_with("2025-01-02"));
        assert!(lines[2].starts_with("2025-01-01"));
    }

    #[test]
    fn session_csv_structure() {
        let sessions = vec![SessionStats {
            session_id: "abc-123".to_string(),
            project_path: "/home/user/project".to_string(),
            first_timestamp: "2025-01-01T00:00:00Z".to_string(),
            last_timestamp: "2025-01-01T01:00:00Z".to_string(),
            stats: Stats {
                input_tokens: 500,
                output_tokens: 200,
                count: 1,
                ..Default::default()
            },
            models: HashMap::new(),
        }];

        let db = PricingDb::default();
        let csv = output_session_csv(&sessions, &db, SortOrder::Asc, false);

        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(
            lines[0],
            "session_id,project_path,first_timestamp,last_timestamp,input_tokens,output_tokens,total_tokens"
        );
        assert!(lines[1].starts_with("abc-123,/home/user/project,"));
    }

    #[test]
    fn project_csv_structure() {
        let projects = vec![ProjectStats {
            project_path: "/home/user/proj".to_string(),
            project_name: "proj".to_string(),
            session_count: 3,
            stats: Stats {
                input_tokens: 1000,
                output_tokens: 500,
                count: 5,
                ..Default::default()
            },
            models: HashMap::new(),
        }];

        let db = PricingDb::default();
        let csv = output_project_csv(&projects, &db, SortOrder::Asc, true);

        let lines: Vec<&str> = csv.lines().collect();
        assert!(lines[0].ends_with(",cost"));
        assert!(lines[1].starts_with("proj,"));
    }

    #[test]
    fn block_csv_structure() {
        let blocks = vec![BlockStats {
            block_start: "2025-01-01 00:00".to_string(),
            block_end: "2025-01-01 05:00".to_string(),
            stats: Stats {
                input_tokens: 800,
                output_tokens: 300,
                cache_creation: 50,
                cache_read: 100,
                ..Default::default()
            },
            models: HashMap::new(),
        }];

        let db = PricingDb::default();
        let csv = output_block_csv(&blocks, &db, SortOrder::Asc, false);

        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(
            lines[0],
            "block_start,block_end,input_tokens,output_tokens,cache_creation_tokens,cache_read_tokens,total_tokens"
        );
        // block_start contains comma-like chars? No, but spaces. Should be fine.
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn empty_data_returns_header_only() {
        let db = PricingDb::default();
        let csv = output_period_csv(
            &HashMap::new(),
            Period::Day,
            &db,
            SortOrder::Asc,
            false,
            false,
        );
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines.len(), 1); // header only
    }

    #[test]
    fn breakdown_csv_header_includes_model() {
        let mut day_stats = HashMap::new();
        day_stats.insert(
            "2025-01-01".to_string(),
            make_day_stats(&[("sonnet", 1000)]),
        );

        let db = PricingDb::default();
        let csv = output_period_csv(&day_stats, Period::Day, &db, SortOrder::Asc, true, false);

        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(
            lines[0],
            "date,model,input_tokens,output_tokens,reasoning_tokens,cache_creation_tokens,cache_read_tokens,total_tokens"
        );
    }

    #[test]
    fn breakdown_csv_one_row_per_model() {
        let mut day_stats = HashMap::new();
        day_stats.insert(
            "2025-01-01".to_string(),
            make_day_stats(&[("sonnet", 1000), ("opus", 500)]),
        );

        let db = PricingDb::default();
        let csv = output_period_csv(&day_stats, Period::Day, &db, SortOrder::Asc, true, false);

        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines.len(), 3); // header + 2 model rows
        // Models sorted alphabetically
        assert!(lines[1].starts_with("2025-01-01,opus,"));
        assert!(lines[2].starts_with("2025-01-01,sonnet,"));
    }

    #[test]
    fn breakdown_csv_with_cost() {
        let mut day_stats = HashMap::new();
        day_stats.insert(
            "2025-01-01".to_string(),
            make_day_stats(&[("sonnet", 1000)]),
        );

        let db = PricingDb::default();
        let csv = output_period_csv(&day_stats, Period::Day, &db, SortOrder::Asc, true, true);

        let lines: Vec<&str> = csv.lines().collect();
        assert!(lines[0].ends_with(",cost"));
        // Data row should also have cost column
        let fields: Vec<&str> = lines[1].split(',').collect();
        assert_eq!(fields.len(), 9); // date,model,5 token fields,total,cost
    }

    #[test]
    fn breakdown_csv_empty_data() {
        let db = PricingDb::default();
        let csv = output_period_csv(
            &HashMap::new(),
            Period::Day,
            &db,
            SortOrder::Asc,
            true,
            false,
        );
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines.len(), 1); // header only
        assert!(lines[0].contains(",model,"));
    }

    #[test]
    fn breakdown_csv_weekly_aggregation() {
        let mut day_stats = HashMap::new();
        // Same week (Mon 2025-01-06 and Wed 2025-01-08)
        day_stats.insert("2025-01-06".to_string(), make_day_stats(&[("sonnet", 100)]));
        day_stats.insert("2025-01-08".to_string(), make_day_stats(&[("sonnet", 200)]));

        let db = PricingDb::default();
        let csv = output_period_csv(&day_stats, Period::Week, &db, SortOrder::Asc, true, false);

        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines.len(), 2); // header + 1 aggregated model row
        assert!(lines[0].starts_with("week,model,"));
        assert!(lines[1].starts_with("2025-01-06,sonnet,"));
        // Verify aggregated input tokens: 100 + 200 = 300
        assert!(lines[1].contains(",300,"));
    }
}
