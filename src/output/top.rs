//! Output formatters for the `top` command (ranked leaderboard).
//!
//! Aggregates per-model or per-project usage and emits the top-N rows
//! sorted by cost (or token total when costs are unavailable). Each row
//! reports the share of the overall total so callers can see which
//! consumer is dominating spend or token volume.

use std::collections::HashMap;
use std::fmt::Write;

use comfy_table::{Attribute, Cell, CellAlignment, Color};
use serde_json::json;

use crate::cli::TopDimension;
use crate::core::{CostKind, DayStats, ProjectStats, Stats};
use crate::output::format::{
    NumberFormat, create_styled_table, csv_escape, format_compact, format_cost, format_number,
    header_cell, right_cell, styled_cell,
};
use crate::pricing::{
    CostDisplayMode, CurrencyConverter, PricingDb, calculate_display_cost, model_cost_kind,
    sum_display_model_costs, sum_estimated_proxy_model_costs,
};

/// One row in the leaderboard.
#[derive(Debug, Clone)]
pub(crate) struct TopRow {
    pub(crate) name: String,
    pub(crate) count: i64,
    pub(crate) stats: Stats,
    pub(crate) cost: f64,
    pub(crate) estimated_cost: f64,
    pub(crate) cost_kind: CostKind,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct TopTableOptions<'a> {
    pub(crate) use_color: bool,
    pub(crate) compact: bool,
    pub(crate) show_cost: bool,
    pub(crate) source_label: &'a str,
    pub(crate) number_format: NumberFormat,
    pub(crate) currency: Option<&'a CurrencyConverter>,
    pub(crate) dim: TopDimension,
    pub(crate) limit: usize,
    pub(crate) cost_mode: CostDisplayMode,
}

/// Aggregate per-model rows from a daily-stats map.
pub(crate) fn rank_by_model(
    day_stats: &HashMap<String, DayStats>,
    pricing_db: &PricingDb,
) -> Vec<TopRow> {
    rank_by_model_with_cost_mode(day_stats, pricing_db, CostDisplayMode::Total)
}

pub(crate) fn rank_by_model_with_cost_mode(
    day_stats: &HashMap<String, DayStats>,
    pricing_db: &PricingDb,
    cost_mode: CostDisplayMode,
) -> Vec<TopRow> {
    let mut totals: HashMap<String, Stats> = HashMap::new();
    for day in day_stats.values() {
        for (model, stats) in &day.models {
            totals.entry(model.clone()).or_default().add(stats);
        }
    }

    let mut rows: Vec<TopRow> = totals
        .into_iter()
        .map(|(model, stats)| {
            let cost = calculate_display_cost(&stats, &model, pricing_db, cost_mode);
            let estimated_cost =
                calculate_display_cost(&stats, &model, pricing_db, CostDisplayMode::Total)
                    - calculate_display_cost(&stats, &model, pricing_db, CostDisplayMode::RealOnly);
            let cost_kind = stats.cost_kind();
            TopRow {
                name: model,
                count: stats.count,
                stats,
                cost,
                estimated_cost,
                cost_kind,
            }
        })
        .collect();

    sort_rows(&mut rows);
    rows
}

/// Aggregate per-project rows from session-derived project stats.
pub(crate) fn rank_by_project(projects: &[ProjectStats], pricing_db: &PricingDb) -> Vec<TopRow> {
    let mut rows: Vec<TopRow> = projects
        .iter()
        .map(|project| {
            let cost = sum_display_model_costs(&project.models, pricing_db, CostDisplayMode::Total);
            let estimated_cost = sum_estimated_proxy_model_costs(&project.models, pricing_db);
            let cost_kind = model_cost_kind(&project.models);
            TopRow {
                name: if project.project_name.is_empty() {
                    project.project_path.clone()
                } else {
                    project.project_name.clone()
                },
                count: project.session_count as i64,
                stats: project.stats.clone(),
                cost,
                estimated_cost,
                cost_kind,
            }
        })
        .collect();

    sort_rows(&mut rows);
    rows
}

/// Sort rows by cost desc, falling back to token total when cost is unknown
/// or when both rows tie on cost. NaN costs sink to the bottom so usable
/// data dominates the leaderboard. Name is the final tie-breaker so output
/// is deterministic regardless of `HashMap` iteration order.
fn sort_rows(rows: &mut [TopRow]) {
    rows.sort_by(|a, b| match (a.cost.is_nan(), b.cost.is_nan()) {
        (true, true) => b
            .stats
            .total_tokens()
            .cmp(&a.stats.total_tokens())
            .then_with(|| a.name.cmp(&b.name)),
        (true, false) => std::cmp::Ordering::Greater,
        (false, true) => std::cmp::Ordering::Less,
        (false, false) => b
            .cost
            .partial_cmp(&a.cost)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.stats.total_tokens().cmp(&a.stats.total_tokens()))
            .then_with(|| a.name.cmp(&b.name)),
    });
}

/// Decide how to compute share-of-total. Use cost when every row has a
/// numeric cost so percentages stay consistent with the sort order; fall
/// back to token totals otherwise.
fn share_basis(rows: &[TopRow]) -> ShareBasis {
    if !rows.is_empty() && rows.iter().all(|r| !r.cost.is_nan() && r.cost > 0.0) {
        ShareBasis::Cost
    } else {
        ShareBasis::Tokens
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShareBasis {
    Cost,
    Tokens,
}

fn share_of(row: &TopRow, total_cost: f64, total_tokens: i64, basis: ShareBasis) -> f64 {
    match basis {
        ShareBasis::Cost if total_cost > 0.0 => (row.cost / total_cost) * 100.0,
        ShareBasis::Tokens if total_tokens > 0 => {
            (row.stats.total_tokens() as f64 / total_tokens as f64) * 100.0
        }
        _ => 0.0,
    }
}

fn dim_label(dim: TopDimension) -> &'static str {
    match dim {
        TopDimension::Model => "Model",
        TopDimension::Project => "Project",
    }
}

fn count_label(dim: TopDimension) -> &'static str {
    match dim {
        TopDimension::Model => "Calls",
        TopDimension::Project => "Sessions",
    }
}

/// Print the leaderboard as a styled table.
#[allow(clippy::too_many_lines)]
pub(crate) fn print_top_table(rows: &[TopRow], options: TopTableOptions<'_>) {
    let limited = take_top(rows, options.limit);

    if limited.is_empty() {
        println!(
            "No {} usage to rank for {}.",
            dim_label(options.dim).to_lowercase(),
            options.source_label
        );
        return;
    }

    let total_cost = sum_cost(&limited);
    let total_tokens = sum_tokens(&limited);
    let basis = share_basis(&limited);
    let cost_color = if options.use_color {
        Some(Color::Green)
    } else {
        None
    };
    let bold_cyan = if options.use_color {
        Some(Color::Cyan)
    } else {
        None
    };

    let mut table = create_styled_table();
    let mut header = vec![
        header_cell("#", options.use_color),
        header_cell(dim_label(options.dim), options.use_color),
        header_cell(count_label(options.dim), options.use_color),
    ];
    if !options.compact {
        header.push(header_cell("Input", options.use_color));
        header.push(header_cell("Output", options.use_color));
    }
    header.push(header_cell("Total", options.use_color));
    header.push(header_cell("Share", options.use_color));
    if options.show_cost {
        header.push(header_cell("Cost", options.use_color));
    }
    table.set_header(header);

    for (idx, row) in limited.iter().enumerate() {
        let share = share_of(row, total_cost, total_tokens, basis);
        let mut cells = vec![
            right_cell(&format!("{}", idx + 1), None, false),
            Cell::new(&row.name),
            right_cell(
                &format_number(row.count, options.number_format),
                None,
                false,
            ),
        ];
        if !options.compact {
            cells.push(right_cell(
                &format_number(row.stats.input_tokens, options.number_format),
                None,
                false,
            ));
            cells.push(right_cell(
                &format_number(row.stats.output_tokens, options.number_format),
                None,
                false,
            ));
        }
        cells.push(right_cell(
            &format_compact(row.stats.total_tokens(), options.number_format),
            None,
            false,
        ));
        cells.push(right_cell(&format!("{share:.1}%"), None, false));
        if options.show_cost {
            cells.push(right_cell(
                &format_cost(row.cost, options.currency),
                cost_color,
                false,
            ));
        }
        table.add_row(cells);
    }

    // TOTAL row reflects the displayed slice, not the full dataset, so the
    // share column always sums to 100% within the leaderboard.
    let displayed_total_tokens: i64 = limited.iter().map(|r| r.stats.total_tokens()).sum();
    let displayed_total_count: i64 = limited.iter().map(|r| r.count).sum();
    let displayed_total_input: i64 = limited.iter().map(|r| r.stats.input_tokens).sum();
    let displayed_total_output: i64 = limited.iter().map(|r| r.stats.output_tokens).sum();
    let mut total_row = vec![
        styled_cell("", bold_cyan, true),
        styled_cell("TOTAL", bold_cyan, true),
        right_cell(
            &format_number(displayed_total_count, options.number_format),
            bold_cyan,
            true,
        ),
    ];
    if !options.compact {
        total_row.push(right_cell(
            &format_number(displayed_total_input, options.number_format),
            bold_cyan,
            true,
        ));
        total_row.push(right_cell(
            &format_number(displayed_total_output, options.number_format),
            bold_cyan,
            true,
        ));
    }
    total_row.push(right_cell(
        &format_compact(displayed_total_tokens, options.number_format),
        bold_cyan,
        true,
    ));
    total_row.push(
        Cell::new("100.0%")
            .add_attribute(Attribute::Bold)
            .set_alignment(CellAlignment::Right),
    );
    if options.show_cost {
        total_row.push(right_cell(
            &format_cost(total_cost, options.currency),
            cost_color,
            true,
        ));
    }
    table.add_row(total_row);
    let estimated_proxy_cost: f64 = limited
        .iter()
        .filter(|row| row.estimated_cost.is_finite())
        .map(|row| row.estimated_cost)
        .sum();

    if rows.len() > limited.len() {
        println!(
            "{} top {} of {} {}(s) — by {}",
            options.source_label,
            limited.len(),
            rows.len(),
            dim_label(options.dim).to_lowercase(),
            if basis == ShareBasis::Cost {
                "cost"
            } else {
                "tokens"
            }
        );
    } else {
        println!(
            "{} top {} {}(s) — by {}",
            options.source_label,
            limited.len(),
            dim_label(options.dim).to_lowercase(),
            if basis == ShareBasis::Cost {
                "cost"
            } else {
                "tokens"
            }
        );
    }
    println!("{table}");
    if options.show_cost && estimated_proxy_cost > 0.0 {
        match options.cost_mode {
            CostDisplayMode::RealOnly => println!(
                "\nEstimated proxy cost excluded from Cost ranking: {}",
                format_cost(estimated_proxy_cost, options.currency)
            ),
            CostDisplayMode::Total => println!(
                "\nCost includes estimated proxy values: {}",
                format_cost(estimated_proxy_cost, options.currency)
            ),
        }
    }
}

/// JSON output. Always includes share, basis, and full stats so downstream
/// tooling does not have to recompute them.
pub(crate) fn output_top_json(
    rows: &[TopRow],
    dim: TopDimension,
    limit: usize,
    show_cost: bool,
    currency: Option<&CurrencyConverter>,
) -> String {
    let limited = take_top(rows, limit);
    let total_cost = sum_cost(&limited);
    let total_tokens = sum_tokens(&limited);
    let basis = share_basis(&limited);
    let include_estimated = show_cost && limited.iter().any(|row| row.estimated_cost > 0.0);

    let entries: Vec<serde_json::Value> = limited
        .iter()
        .enumerate()
        .map(|(idx, row)| {
            let share = share_of(row, total_cost, total_tokens, basis);
            let mut obj = json!({
                "rank": idx + 1,
                "name": row.name,
                "count": row.count,
                "input_tokens": row.stats.input_tokens,
                "output_tokens": row.stats.output_tokens,
                "cache_creation": row.stats.cache_creation,
                "cache_read": row.stats.cache_read,
                "reasoning_tokens": row.stats.reasoning_tokens,
                "total_tokens": row.stats.total_tokens(),
                "share_percent": (share * 100.0).round() / 100.0,
            });
            if show_cost {
                obj["cost_usd"] = if row.cost.is_nan() {
                    serde_json::Value::Null
                } else {
                    json!((row.cost * 100_000.0).round() / 100_000.0)
                };
                if let Some(conv) = currency
                    && !row.cost.is_nan()
                {
                    obj["cost_local"] = json!(conv.format(row.cost));
                }
                if include_estimated {
                    obj["cost_kind"] = json!(row.cost_kind.as_str());
                    obj["estimated_cost_usd"] = if row.estimated_cost.is_nan() {
                        serde_json::Value::Null
                    } else {
                        json!((row.estimated_cost * 100_000.0).round() / 100_000.0)
                    };
                    if let Some(conv) = currency
                        && !row.estimated_cost.is_nan()
                    {
                        obj["estimated_cost_local"] = json!(conv.format(row.estimated_cost));
                    }
                }
            }
            obj
        })
        .collect();

    json!({
        "dimension": match dim {
            TopDimension::Model => "model",
            TopDimension::Project => "project",
        },
        "limit": limit,
        "displayed": limited.len(),
        "total_rows": rows.len(),
        "share_basis": match basis {
            ShareBasis::Cost => "cost",
            ShareBasis::Tokens => "tokens",
        },
        "entries": entries,
    })
    .to_string()
}

/// CSV output. Header columns mirror the JSON keys.
pub(crate) fn output_top_csv(
    rows: &[TopRow],
    dim: TopDimension,
    limit: usize,
    show_cost: bool,
    currency: Option<&CurrencyConverter>,
) -> String {
    let limited = take_top(rows, limit);
    let total_cost = sum_cost(&limited);
    let total_tokens = sum_tokens(&limited);
    let basis = share_basis(&limited);

    let mut out = String::new();
    let dim_col = match dim {
        TopDimension::Model => "model",
        TopDimension::Project => "project",
    };
    let _ = write!(
        out,
        "rank,{dim_col},count,input_tokens,output_tokens,cache_creation,cache_read,reasoning_tokens,total_tokens,share_percent"
    );
    if show_cost {
        out.push_str(",cost_usd");
        if currency.is_some() {
            out.push_str(",cost_local");
        }
        if limited.iter().any(|row| row.estimated_cost > 0.0) {
            out.push_str(",cost_kind,estimated_cost_usd");
            if currency.is_some() {
                out.push_str(",estimated_cost_local");
            }
        }
    }
    out.push('\n');
    let include_estimated = show_cost && limited.iter().any(|row| row.estimated_cost > 0.0);

    for (idx, row) in limited.iter().enumerate() {
        let share = share_of(row, total_cost, total_tokens, basis);
        let _ = write!(
            out,
            "{},{},{},{},{},{},{},{},{},{:.2}",
            idx + 1,
            csv_escape(&row.name),
            row.count,
            row.stats.input_tokens,
            row.stats.output_tokens,
            row.stats.cache_creation,
            row.stats.cache_read,
            row.stats.reasoning_tokens,
            row.stats.total_tokens(),
            share,
        );
        if show_cost {
            if row.cost.is_nan() {
                out.push(',');
                if currency.is_some() {
                    out.push(',');
                }
            } else {
                let _ = write!(out, ",{:.6}", row.cost);
                if let Some(conv) = currency {
                    let _ = write!(out, ",{}", csv_escape(&conv.format(row.cost)));
                }
            }
            if include_estimated {
                let _ = write!(out, ",{}", row.cost_kind.as_str());
                if row.estimated_cost.is_nan() {
                    out.push(',');
                } else {
                    let _ = write!(out, ",{:.6}", row.estimated_cost);
                }
                if let Some(conv) = currency {
                    if row.estimated_cost.is_nan() {
                        out.push(',');
                    } else {
                        let _ = write!(out, ",{}", csv_escape(&conv.format(row.estimated_cost)));
                    }
                }
            }
        }
        out.push('\n');
    }
    out
}

fn take_top(rows: &[TopRow], limit: usize) -> Vec<TopRow> {
    let n = limit.min(rows.len());
    rows.iter().take(n).cloned().collect()
}

fn sum_cost(rows: &[TopRow]) -> f64 {
    let mut total = 0.0;
    for row in rows {
        if row.cost.is_nan() {
            return f64::NAN;
        }
        total += row.cost;
    }
    total
}

fn sum_tokens(rows: &[TopRow]) -> i64 {
    rows.iter().map(|r| r.stats.total_tokens()).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pricing::PricingDb;

    fn stats_of(input: i64, output: i64, count: i64) -> Stats {
        Stats {
            input_tokens: input,
            output_tokens: output,
            cache_creation: 0,
            cache_read: 0,
            reasoning_tokens: 0,
            count,
            skipped_chunks: 0,
            estimated_proxy: crate::core::CostTokens::default(),
        }
    }

    fn day_with(model: &str, stats: &Stats) -> DayStats {
        let mut day = DayStats::default();
        day.add_stats(model.to_string(), stats);
        day
    }

    #[test]
    fn rank_by_model_aggregates_across_days() {
        let mut day_stats = HashMap::new();
        day_stats.insert(
            "2025-01-01".to_string(),
            day_with("claude-sonnet", &stats_of(100, 50, 1)),
        );
        let mut day2 = DayStats::default();
        day2.add_stats("claude-sonnet".into(), &stats_of(200, 80, 2));
        day2.add_stats("gpt-4".into(), &stats_of(400, 60, 3));
        day_stats.insert("2025-01-02".into(), day2);

        // PricingDb::default() applies a fallback price to every model, so
        // costs are non-NaN and ranking follows cost desc. With the same
        // fallback per-model price, gpt-4 (460 total tokens) outranks
        // claude-sonnet (430 total tokens).
        let rows = rank_by_model(&day_stats, &PricingDb::default());
        assert_eq!(rows.len(), 2);
        // Both models receive a positive cost from the fallback table; the
        // model with more billable tokens wins.
        let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
        assert!(names.contains(&"claude-sonnet"));
        assert!(names.contains(&"gpt-4"));
        // Aggregated counts are independent of sort order.
        for row in &rows {
            if row.name == "claude-sonnet" {
                assert_eq!(row.stats.input_tokens, 300);
                assert_eq!(row.stats.output_tokens, 130);
                assert_eq!(row.count, 3);
            } else if row.name == "gpt-4" {
                assert_eq!(row.stats.input_tokens, 400);
                assert_eq!(row.stats.output_tokens, 60);
                assert_eq!(row.count, 3);
            }
        }
    }

    #[test]
    fn share_basis_uses_tokens_when_costs_unknown() {
        let rows = vec![
            TopRow {
                name: "a".into(),
                count: 1,
                stats: stats_of(100, 0, 1),
                cost: f64::NAN,
                estimated_cost: 0.0,
                cost_kind: CostKind::Real,
            },
            TopRow {
                name: "b".into(),
                count: 1,
                stats: stats_of(300, 0, 1),
                cost: f64::NAN,
                estimated_cost: 0.0,
                cost_kind: CostKind::Real,
            },
        ];
        assert_eq!(share_basis(&rows), ShareBasis::Tokens);
        assert!((share_of(&rows[0], 0.0, 400, ShareBasis::Tokens) - 25.0).abs() < 0.001);
        assert!((share_of(&rows[1], 0.0, 400, ShareBasis::Tokens) - 75.0).abs() < 0.001);
    }

    #[test]
    fn share_basis_uses_cost_when_all_known() {
        let rows = vec![
            TopRow {
                name: "a".into(),
                count: 1,
                stats: stats_of(100, 0, 1),
                cost: 0.25,
                estimated_cost: 0.0,
                cost_kind: CostKind::Real,
            },
            TopRow {
                name: "b".into(),
                count: 1,
                stats: stats_of(100, 0, 1),
                cost: 0.75,
                estimated_cost: 0.0,
                cost_kind: CostKind::Real,
            },
        ];
        assert_eq!(share_basis(&rows), ShareBasis::Cost);
        let total = sum_cost(&rows);
        assert!((share_of(&rows[0], total, 0, ShareBasis::Cost) - 25.0).abs() < 0.001);
        assert!((share_of(&rows[1], total, 0, ShareBasis::Cost) - 75.0).abs() < 0.001);
    }

    #[test]
    fn sort_rows_pushes_nan_costs_to_end() {
        let mut rows = vec![
            TopRow {
                name: "nan-a".into(),
                count: 1,
                stats: stats_of(100, 0, 1),
                cost: f64::NAN,
                estimated_cost: 0.0,
                cost_kind: CostKind::Real,
            },
            TopRow {
                name: "high-cost".into(),
                count: 1,
                stats: stats_of(10, 0, 1),
                cost: 5.0,
                estimated_cost: 0.0,
                cost_kind: CostKind::Real,
            },
            TopRow {
                name: "low-cost".into(),
                count: 1,
                stats: stats_of(10, 0, 1),
                cost: 1.0,
                estimated_cost: 0.0,
                cost_kind: CostKind::Real,
            },
        ];
        sort_rows(&mut rows);
        assert_eq!(rows[0].name, "high-cost");
        assert_eq!(rows[1].name, "low-cost");
        assert_eq!(rows[2].name, "nan-a");
    }

    #[test]
    fn limit_caps_the_displayed_count() {
        let rows: Vec<TopRow> = (0..20)
            .map(|i| TopRow {
                name: format!("m{i}"),
                count: 1,
                stats: stats_of(100 - i, 0, 1),
                cost: f64::NAN,
                estimated_cost: 0.0,
                cost_kind: CostKind::Real,
            })
            .collect();
        let csv = output_top_csv(&rows, TopDimension::Model, 5, false, None);
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines.len(), 6); // header + 5 rows
    }

    #[test]
    fn json_includes_dimension_and_basis() {
        let rows = vec![TopRow {
            name: "m".into(),
            count: 1,
            stats: stats_of(100, 50, 1),
            cost: 1.5,
            estimated_cost: 0.0,
            cost_kind: CostKind::Real,
        }];
        let json = output_top_json(&rows, TopDimension::Model, 10, true, None);
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val["dimension"], "model");
        assert_eq!(val["share_basis"], "cost");
        assert_eq!(val["entries"][0]["name"], "m");
        assert_eq!(val["entries"][0]["rank"], 1);
        assert_eq!(val["entries"][0]["share_percent"], 100.0);
        assert_eq!(val["entries"][0]["cost_usd"], 1.5);
    }

    #[test]
    fn json_share_basis_falls_back_to_tokens() {
        let rows = vec![
            TopRow {
                name: "a".into(),
                count: 1,
                stats: stats_of(100, 0, 1),
                cost: f64::NAN,
                estimated_cost: 0.0,
                cost_kind: CostKind::Real,
            },
            TopRow {
                name: "b".into(),
                count: 1,
                stats: stats_of(300, 0, 1),
                cost: f64::NAN,
                estimated_cost: 0.0,
                cost_kind: CostKind::Real,
            },
        ];
        let json = output_top_json(&rows, TopDimension::Model, 10, true, None);
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val["share_basis"], "tokens");
        // cost_usd should be JSON null for NaN
        assert!(val["entries"][0]["cost_usd"].is_null());
    }

    #[test]
    fn csv_escapes_names_with_commas() {
        let rows = vec![TopRow {
            name: "weird,name".into(),
            count: 1,
            stats: stats_of(10, 0, 1),
            cost: f64::NAN,
            estimated_cost: 0.0,
            cost_kind: CostKind::Real,
        }];
        let csv = output_top_csv(&rows, TopDimension::Project, 1, false, None);
        let lines: Vec<&str> = csv.lines().collect();
        assert!(lines[1].contains("\"weird,name\""), "csv: {}", lines[1]);
    }

    #[test]
    fn csv_includes_local_cost_when_currency_is_set() {
        let rows = vec![TopRow {
            name: "model".into(),
            count: 1,
            stats: stats_of(10, 0, 1),
            cost: 1.5,
            estimated_cost: 0.0,
            cost_kind: CostKind::Real,
        }];
        let converter = CurrencyConverter::from_rate_for_test("CNY", 7.0, "CNY ");

        let csv = output_top_csv(&rows, TopDimension::Model, 1, true, Some(&converter));

        let lines: Vec<&str> = csv.lines().collect();
        assert!(lines[0].ends_with(",cost_usd,cost_local"));
        assert!(lines[1].ends_with(",1.500000,CNY 10.50"));
    }

    #[test]
    fn empty_rows_produce_header_only_csv() {
        let csv = output_top_csv(&[], TopDimension::Model, 10, false, None);
        assert_eq!(csv.lines().count(), 1);
    }
}
