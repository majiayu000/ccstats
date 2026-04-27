use std::collections::HashMap;

use chrono::{Datelike, NaiveDate};
use comfy_table::{Cell, Color};
use serde_json::{Map, Value};

use crate::cli::SortOrder;
use crate::core::DayStats;
use crate::output::format::{create_styled_table, header_cell, right_cell, styled_cell};
use crate::output::period::{Period, aggregate_day_stats_by_period};
use crate::pricing::{CurrencyConverter, PricingDb, sum_model_costs};

#[derive(Debug, Clone)]
pub(crate) struct MonthlyBudgetReport {
    pub(crate) month: String,
    pub(crate) limit: f64,
    pub(crate) spent: f64,
    pub(crate) projected: f64,
    pub(crate) remaining: f64,
    pub(crate) used_pct: f64,
    pub(crate) projected_pct: f64,
    pub(crate) days_elapsed: u32,
    pub(crate) days_in_month: u32,
    pub(crate) status: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct MonthlyBudgetOptions<'a> {
    pub(crate) order: SortOrder,
    pub(crate) breakdown: bool,
    pub(crate) show_cost: bool,
    pub(crate) limit: f64,
    pub(crate) as_of: NaiveDate,
    pub(crate) currency: Option<&'a CurrencyConverter>,
}

fn display_cost(usd: f64, currency: Option<&CurrencyConverter>) -> f64 {
    currency.map_or(usd, |conv| conv.convert(usd))
}

fn month_parts(month: &str) -> Option<(i32, u32)> {
    let (year, month) = month.split_once('-')?;
    let year = year.parse::<i32>().ok()?;
    let month = month.parse::<u32>().ok()?;
    (1..=12).contains(&month).then_some((year, month))
}

fn days_in_month(year: i32, month: u32) -> Option<u32> {
    let start = NaiveDate::from_ymd_opt(year, month, 1)?;
    let (next_year, next_month) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    let next = NaiveDate::from_ymd_opt(next_year, next_month, 1)?;
    Some((next - start).num_days() as u32)
}

fn budget_days(month: &str, as_of: NaiveDate) -> (u32, u32) {
    let Some((year, month_num)) = month_parts(month) else {
        return (1, 1);
    };
    let days_in_month = days_in_month(year, month_num).unwrap_or(1);
    let elapsed = if (as_of.year(), as_of.month()) == (year, month_num) {
        as_of.day().min(days_in_month)
    } else {
        days_in_month
    };
    (elapsed.max(1), days_in_month)
}

fn percentage(value: f64, limit: f64) -> f64 {
    if value.is_nan() || limit <= 0.0 {
        f64::NAN
    } else {
        value / limit * 100.0
    }
}

fn budget_status(spent: f64, projected: f64, projected_pct: f64, limit: f64) -> &'static str {
    if spent.is_nan() || projected.is_nan() {
        "unknown"
    } else if spent > limit || projected > limit {
        "over_budget"
    } else if projected_pct >= 90.0 {
        "watch"
    } else {
        "on_track"
    }
}

fn budget_report(month: String, spent: f64, limit: f64, as_of: NaiveDate) -> MonthlyBudgetReport {
    let (days_elapsed, days_in_month) = budget_days(&month, as_of);
    let projected = if spent.is_nan() || days_elapsed >= days_in_month {
        spent
    } else {
        spent * f64::from(days_in_month) / f64::from(days_elapsed)
    };
    let remaining = if spent.is_nan() {
        f64::NAN
    } else {
        limit - spent
    };
    let used_pct = percentage(spent, limit);
    let projected_pct = percentage(projected, limit);
    let status = budget_status(spent, projected, projected_pct, limit);

    MonthlyBudgetReport {
        month,
        limit,
        spent,
        projected,
        remaining,
        used_pct,
        projected_pct,
        days_elapsed,
        days_in_month,
        status,
    }
}

pub(crate) fn monthly_budget_reports(
    day_stats: &HashMap<String, DayStats>,
    pricing_db: &PricingDb,
    order: SortOrder,
    monthly_budget: f64,
    as_of: NaiveDate,
    currency: Option<&CurrencyConverter>,
) -> Vec<MonthlyBudgetReport> {
    let monthly = aggregate_day_stats_by_period(day_stats, Period::Month);
    let mut months: Vec<_> = monthly.keys().collect();
    match order {
        SortOrder::Asc => months.sort(),
        SortOrder::Desc => months.sort_by(|a, b| b.cmp(a)),
    }

    months
        .into_iter()
        .filter_map(|month| {
            monthly.get(month).map(|stats| {
                let spent = display_cost(sum_model_costs(&stats.models, pricing_db), currency);
                budget_report(month.clone(), spent, monthly_budget, as_of)
            })
        })
        .collect()
}

fn json_number(value: f64) -> Value {
    if value.is_nan() {
        Value::Null
    } else {
        serde_json::json!(value)
    }
}

fn report_json(report: &MonthlyBudgetReport) -> Value {
    let mut obj = Map::new();
    obj.insert("limit".to_string(), json_number(report.limit));
    obj.insert("spent".to_string(), json_number(report.spent));
    obj.insert("projected".to_string(), json_number(report.projected));
    obj.insert("remaining".to_string(), json_number(report.remaining));
    obj.insert("used_pct".to_string(), json_number(report.used_pct));
    obj.insert(
        "projected_pct".to_string(),
        json_number(report.projected_pct),
    );
    obj.insert(
        "days_elapsed".to_string(),
        serde_json::json!(report.days_elapsed),
    );
    obj.insert(
        "days_in_month".to_string(),
        serde_json::json!(report.days_in_month),
    );
    obj.insert("status".to_string(), serde_json::json!(report.status));
    Value::Object(obj)
}

pub(crate) fn add_monthly_budget_to_json(json: &str, reports: &[MonthlyBudgetReport]) -> String {
    let mut rows: Vec<Value> = serde_json::from_str(json).unwrap_or_default();
    let by_month: HashMap<&str, &MonthlyBudgetReport> = reports
        .iter()
        .map(|report| (report.month.as_str(), report))
        .collect();

    for row in &mut rows {
        let Some(obj) = row.as_object_mut() else {
            continue;
        };
        let Some(month) = obj.get("month").and_then(Value::as_str) else {
            continue;
        };
        if let Some(report) = by_month.get(month) {
            obj.insert("budget".to_string(), report_json(report));
        }
    }

    serde_json::to_string(&rows).unwrap_or_else(|_| "[]".to_string())
}

fn format_amount(value: f64, currency: Option<&CurrencyConverter>) -> String {
    if value.is_nan() {
        "N/A".to_string()
    } else if let Some(conv) = currency {
        let code = conv.currency_code();
        format!("{code} {value:.2}")
    } else {
        format!("${value:.2}")
    }
}

fn format_pct(value: f64) -> String {
    if value.is_nan() {
        "N/A".to_string()
    } else {
        format!("{value:.1}%")
    }
}

fn status_color(status: &str, use_color: bool) -> Option<Color> {
    if !use_color {
        return None;
    }
    match status {
        "over_budget" => Some(Color::Red),
        "watch" => Some(Color::Yellow),
        "on_track" => Some(Color::Green),
        _ => None,
    }
}

pub(crate) fn print_monthly_budget_table(
    reports: &[MonthlyBudgetReport],
    use_color: bool,
    currency: Option<&CurrencyConverter>,
) {
    if reports.is_empty() {
        return;
    }

    let mut table = create_styled_table();
    table.set_header(vec![
        header_cell("Month", use_color),
        header_cell("Budget", use_color),
        header_cell("Spent", use_color),
        header_cell("Projected", use_color),
        header_cell("Remaining", use_color),
        header_cell("Used", use_color),
        header_cell("Projected", use_color),
        header_cell("Status", use_color),
    ]);

    for report in reports {
        table.add_row(vec![
            Cell::new(&report.month),
            right_cell(&format_amount(report.limit, currency), None, false),
            right_cell(&format_amount(report.spent, currency), None, false),
            right_cell(&format_amount(report.projected, currency), None, false),
            right_cell(&format_amount(report.remaining, currency), None, false),
            right_cell(&format_pct(report.used_pct), None, false),
            right_cell(&format_pct(report.projected_pct), None, false),
            styled_cell(report.status, status_color(report.status, use_color), false),
        ]);
    }

    println!("\n  Monthly Budget Forecast\n");
    println!("{table}");
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;
    use crate::core::Stats;

    fn make_day_stats() -> HashMap<String, DayStats> {
        let mut stats = HashMap::new();
        let mut day = DayStats::default();
        day.add_stats(
            "sonnet".to_string(),
            &Stats {
                input_tokens: 1_000_000,
                output_tokens: 100_000,
                count: 1,
                ..Default::default()
            },
        );
        stats.insert("2026-02-10".to_string(), day);
        stats
    }

    #[test]
    fn monthly_budget_projects_partial_month() {
        let reports = monthly_budget_reports(
            &make_day_stats(),
            &PricingDb::default(),
            SortOrder::Asc,
            10.0,
            NaiveDate::from_ymd_opt(2026, 2, 10).unwrap(),
            None,
        );

        assert_eq!(reports.len(), 1);
        let report = &reports[0];
        assert_eq!(report.month, "2026-02");
        assert_eq!(report.days_elapsed, 10);
        assert_eq!(report.days_in_month, 28);
        assert!((report.spent - 4.5).abs() < 0.001);
        assert!((report.projected - 12.6).abs() < 0.001);
        assert_eq!(report.status, "over_budget");
    }

    #[test]
    fn add_monthly_budget_to_json_attaches_budget_object() {
        let report = budget_report(
            "2026-02".to_string(),
            4.5,
            10.0,
            NaiveDate::from_ymd_opt(2026, 2, 10).unwrap(),
        );
        let json = r#"[{"month":"2026-02","cost":4.5}]"#;
        let output = add_monthly_budget_to_json(json, &[report]);
        let parsed: Value = serde_json::from_str(&output).unwrap();

        assert_eq!(parsed[0]["budget"]["status"], "over_budget");
        assert_eq!(parsed[0]["budget"]["days_elapsed"], 10);
    }
}
