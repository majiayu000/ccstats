use std::collections::HashMap;
use std::fmt::Write as _;

use crate::core::DayStats;
use crate::output::{Period, aggregate_day_stats_by_period};
use crate::source::CostCoverage;

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
    if let serde_json::Value::Array(rows) = &mut value {
        for row in rows {
            let Some(key) = row.get(period.label()).and_then(serde_json::Value::as_str) else {
                continue;
            };
            let Some(coverage) = rows_by_key
                .get(key)
                .and_then(|row| CostCoverage::from_stats(std::iter::once(&row.stats)))
            else {
                continue;
            };
            row["api_equivalent_cost_coverage"] = metadata(coverage);
        }
    }
    serde_json::to_string(&value).unwrap_or_else(|_| json.to_string())
}

pub(crate) fn annotate_csv(mut csv: String, coverage: Option<CostCoverage>) -> String {
    if let Some(coverage) = coverage {
        if !csv.ends_with('\n') {
            csv.push('\n');
        }
        let _ = writeln!(
            csv,
            "# api_equivalent_cost_coverage,{},{},{:.2},{},{}",
            coverage.total_tokens,
            coverage.priced_tokens,
            coverage.percent(),
            !coverage.is_partial(),
            coverage.cost_is_lower_bound()
        );
    }
    csv
}

pub(crate) fn print_note(coverage: Option<CostCoverage>) {
    let Some(coverage) = coverage else {
        return;
    };
    if coverage.cost_is_lower_bound() {
        println!(
            "\n  API-equivalent cost coverage: {} / {} tokens ({:.2}%); displayed cost is a lower bound.",
            coverage.priced_tokens,
            coverage.total_tokens,
            coverage.percent()
        );
    }
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
        let output = annotate_json(r#"[{"date":"2026-08-24"}]"#, &day_stats, Period::Day);
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
        );
        let value: serde_json::Value = serde_json::from_str(&output).expect("valid json");

        assert!(value[0].get("api_equivalent_cost_coverage").is_some());
        assert!(value[1].get("api_equivalent_cost_coverage").is_none());
    }
}
