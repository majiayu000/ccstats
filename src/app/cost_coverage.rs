use std::collections::HashMap;
use std::fmt::Write as _;

use crate::core::DayStats;
use crate::output::{Period, aggregate_day_stats_by_period, format_cost, period_key};
use crate::pricing::CurrencyConverter;
use crate::source::{CostCoverage, GrokCostReport};

fn metadata(coverage: CostCoverage) -> serde_json::Value {
    serde_json::json!({
        "total_tokens": coverage.total_tokens,
        "priced_tokens": coverage.priced_tokens,
        "percent": coverage.percent(),
        "complete": !coverage.is_partial(),
        "cost_is_lower_bound": coverage.cost_is_lower_bound(),
    })
}

pub(crate) fn annotate_json(
    json: &str,
    day_stats: &HashMap<String, DayStats>,
    period: Period,
    grok_reports: Option<&HashMap<String, GrokCostReport>>,
) -> String {
    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(json) else {
        return json.to_string();
    };
    let aggregated;
    let rows_by_key = if period == Period::Day {
        day_stats
    } else {
        aggregated = aggregate_day_stats_by_period(day_stats, period);
        &aggregated
    };
    let aggregated_grok = grok_reports.map(|reports| aggregate_grok_reports(reports, period));
    if let serde_json::Value::Array(rows) = &mut value {
        for row in rows {
            let Some(key) = row
                .get(period.label())
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
            else {
                continue;
            };
            let Some(coverage) = rows_by_key
                .get(&key)
                .and_then(|row| CostCoverage::from_stats(std::iter::once(&row.stats)))
            else {
                continue;
            };
            row["api_equivalent_cost_coverage"] = metadata(coverage);
            if let Some(report) = aggregated_grok
                .as_ref()
                .and_then(|reports| reports.get(&key))
            {
                row["grok_cost_summary"] = grok_summary(*report);
                row["pricing_source"] = serde_json::json!("calculated_api_equivalent");
                row["cost_semantics"] = serde_json::json!("observed_api_equivalent");
                if let Some(breakdown) = row
                    .get_mut("breakdown")
                    .and_then(|value| value.as_array_mut())
                {
                    for model in breakdown {
                        model["pricing_source"] = serde_json::json!("calculated_api_equivalent");
                        model["cost_semantics"] = serde_json::json!("observed_api_equivalent");
                    }
                }
            }
        }
    }
    serde_json::to_string(&value).unwrap_or_else(|_| json.to_string())
}

pub(crate) fn annotate_csv(
    mut csv: String,
    coverage: Option<CostCoverage>,
    grok_report: Option<GrokCostReport>,
) -> String {
    if let Some(coverage) = coverage {
        if !csv.ends_with('\n') {
            csv.push('\n');
        }
        writeln!(
            csv,
            "# api_equivalent_cost_coverage,{},{},{:.2},{},{}",
            coverage.total_tokens,
            coverage.priced_tokens,
            coverage.percent(),
            !coverage.is_partial(),
            coverage.cost_is_lower_bound()
        )
        .expect("writing to String cannot fail");
    }
    if let Some(report) = grok_report {
        writeln!(
            csv,
            "# grok_api_equivalent_usd,{:.12},{},{},{},{}",
            report.observed_api_cost_usd,
            optional_number(report.estimated_api_cost_usd()),
            optional_number(report.api_cost_lower_bound_usd),
            optional_number(report.api_cost_upper_bound_usd),
            report.status()
        )
        .expect("writing to String cannot fail");
        writeln!(
            csv,
            "# grok_provider_metric_usd,{},{},{},{:.2}",
            optional_number(report.provider_reported_cost_usd),
            report.provider_priced_tokens,
            report.total_tokens,
            report.provider_percent()
        )
        .expect("writing to String cannot fail");
        csv.push_str("# grok_actual_billed_usd,unavailable\n");
        csv.push_str("# pricing_source,calculated_api_equivalent\n");
    }
    csv
}

pub(crate) fn print_note(report: Option<GrokCostReport>, currency: Option<&CurrencyConverter>) {
    let Some(report) = report else {
        return;
    };
    println!("\n  Grok cost summary (selected range)");
    println!(
        "  Observed API equivalent: {}",
        format_cost(report.observed_api_cost_usd, currency)
    );
    match report.estimated_api_cost_usd() {
        Some(cost) => println!(
            "  Estimated API equivalent: ~{}",
            format_cost(cost, currency)
        ),
        None => println!("  Estimated API equivalent: unavailable"),
    }
    match (
        report.api_cost_lower_bound_usd,
        report.api_cost_upper_bound_usd,
    ) {
        (Some(minimum), Some(maximum)) => println!(
            "  API equivalent range: {} - {}",
            format_cost(minimum, currency),
            format_cost(maximum, currency)
        ),
        _ => println!("  API equivalent range: unavailable"),
    }
    println!(
        "  Request coverage: {} / {} tokens ({:.2}%, {})",
        report.priced_tokens,
        report.total_tokens,
        report.coverage_percent(),
        report.status()
    );
    match report.provider_reported_cost_usd {
        Some(cost) => println!(
            "  Provider metric: {} ({:.2}% token coverage)",
            format_cost(cost, currency),
            report.provider_percent()
        ),
        None => println!("  Provider metric: unavailable"),
    }
    println!("  Actual billed: unavailable");
}

pub(crate) fn selected_grok_report(
    reports: Option<&HashMap<String, GrokCostReport>>,
) -> Option<GrokCostReport> {
    reports.and_then(|reports| reports.values().copied().reduce(GrokCostReport::merge))
}

fn aggregate_grok_reports(
    reports: &HashMap<String, GrokCostReport>,
    period: Period,
) -> HashMap<String, GrokCostReport> {
    let mut aggregated: HashMap<String, GrokCostReport> = HashMap::new();
    for (date, report) in reports {
        let key = period_key(date, period);
        aggregated
            .entry(key)
            .and_modify(|current| *current = current.merge(*report))
            .or_insert(*report);
    }
    aggregated
}

fn grok_summary(report: GrokCostReport) -> serde_json::Value {
    serde_json::json!({
        "api_equivalent": {
            "observed_usd": report.observed_api_cost_usd,
            "estimated_usd": report.estimated_api_cost_usd(),
            "range_usd": {
                "minimum": report.api_cost_lower_bound_usd,
                "maximum": report.api_cost_upper_bound_usd,
            },
            "priced_tokens": report.priced_tokens,
            "total_tokens": report.total_tokens,
            "coverage_percent": report.coverage_percent(),
            "coverage_status": report.status(),
        },
        "provider_metric": {
            "reported_usd": report.provider_reported_cost_usd,
            "priced_tokens": report.provider_priced_tokens,
            "total_tokens": report.total_tokens,
            "coverage_percent": report.provider_percent(),
        },
        "actual_billed_usd": serde_json::Value::Null,
    })
}

fn optional_number(value: Option<f64>) -> String {
    value.map_or_else(|| "unavailable".to_string(), |value| format!("{value:.12}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{CostTokens, Stats};

    fn day(total_tokens: i64, priced_tokens: i64, estimated_proxy_tokens: i64) -> DayStats {
        DayStats {
            stats: Stats {
                input_tokens: total_tokens,
                api_equivalent_priced_tokens: priced_tokens,
                api_equivalent_coverage_tokens: total_tokens,
                estimated_proxy: CostTokens {
                    input_tokens: estimated_proxy_tokens,
                    count: i64::from(estimated_proxy_tokens > 0),
                    ..Default::default()
                },
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn json_marks_partial_api_cost_as_lower_bound() {
        let day_stats = HashMap::from([("2026-08-24".to_string(), day(200, 100, 0))]);
        let output = annotate_json(r#"[{"date":"2026-08-24"}]"#, &day_stats, Period::Day, None);
        let value: serde_json::Value = serde_json::from_str(&output).expect("valid json");
        let coverage = &value[0]["api_equivalent_cost_coverage"];
        assert_eq!(coverage["total_tokens"], 200);
        assert_eq!(coverage["priced_tokens"], 100);
        assert_eq!(coverage["percent"], 50.0);
        assert_eq!(coverage["complete"], false);
        assert_eq!(coverage["cost_is_lower_bound"], true);
    }

    #[test]
    fn snapshot_only_estimate_is_not_labeled_a_lower_bound() {
        let day_stats = HashMap::from([("2026-08-24".to_string(), day(200, 0, 200))]);
        let output = annotate_json(
            r#"[{"date":"2026-08-24","total_tokens":200,"estimated_cost":0.1}]"#,
            &day_stats,
            Period::Day,
            None,
        );
        let value: serde_json::Value = serde_json::from_str(&output).expect("valid json");

        assert_eq!(
            value[0]["api_equivalent_cost_coverage"]["cost_is_lower_bound"],
            false
        );
    }

    #[test]
    fn json_does_not_copy_range_coverage_into_empty_period_rows() {
        let day_stats = HashMap::from([("2026-08-24".to_string(), day(200, 100, 0))]);
        let output = annotate_json(
            r#"[{"date":"2026-08-24","total_tokens":200},{"date":"2026-08-25","total_tokens":0}]"#,
            &day_stats,
            Period::Day,
            None,
        );
        let value: serde_json::Value = serde_json::from_str(&output).expect("valid json");

        assert!(value[0].get("api_equivalent_cost_coverage").is_some());
        assert!(value[1].get("api_equivalent_cost_coverage").is_none());
    }
}
