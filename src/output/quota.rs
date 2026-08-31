use comfy_table::{Cell, Color};
use serde_json::json;

use crate::sdk::{CodexWeeklyValueError, CodexWeeklyValueEstimate};
use crate::source::{CodexQuotaStatus, CodexWeeklyQuota};
use crate::utils::Timezone;

use super::format::{
    NumberFormat, create_styled_table, csv_escape, format_compact, header_cell, right_cell,
    styled_cell,
};

fn rounded_pct(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

fn rounded_cost(value: f64) -> f64 {
    (value * 1_000_000.0).round() / 1_000_000.0
}

fn timestamp(value: chrono::DateTime<chrono::Utc>) -> String {
    value.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

pub(crate) type QuotaValueEstimate<'a> =
    Option<Result<&'a CodexWeeklyValueEstimate, &'a CodexWeeklyValueError>>;

pub(crate) fn output_quota_json(
    report: &CodexWeeklyQuota,
    value_estimate: QuotaValueEstimate<'_>,
) -> String {
    let mut output = json!({
        "source": "codex",
        "window": "weekly",
        "window_minutes": report.window_minutes,
        "used_pct": rounded_pct(report.used_pct),
        "remaining_pct": rounded_pct(report.remaining_pct),
        "projected_pct_at_reset": rounded_pct(report.projected_pct_at_reset),
        "status": report.status.as_str(),
        "observed_at": timestamp(report.observed_at),
        "resets_at": timestamp(report.resets_at),
        "estimated_depletion_at": report.estimated_depletion_at.map(timestamp),
    });

    match value_estimate {
        Some(Ok(estimate)) => {
            output["value_estimate"] = json!({
                "kind": "api_equivalent",
                "observed_cost_usd": rounded_cost(estimate.observed_cost_usd),
                "estimated_weekly_value_usd": rounded_cost(estimate.estimated_weekly_value_usd),
                "observed_tokens": estimate.observed_tokens,
                "estimated_weekly_tokens": estimate.estimated_weekly_tokens.round(),
                "window_started_at": timestamp(estimate.window_started_at),
                "valid_entries": estimate.valid_entries,
                "dedup_skipped_entries": estimate.dedup_skipped_entries,
            });
            output["value_estimate_error"] = serde_json::Value::Null;
        }
        Some(Err(error)) => {
            output["value_estimate"] = serde_json::Value::Null;
            output["value_estimate_error"] = json!(error.to_string());
        }
        None => {}
    }

    output.to_string()
}

pub(crate) fn output_quota_csv(
    report: &CodexWeeklyQuota,
    value_estimate: QuotaValueEstimate<'_>,
) -> String {
    let depletion = report
        .estimated_depletion_at
        .map(timestamp)
        .unwrap_or_default();
    let Some(value_estimate) = value_estimate else {
        return format!(
            "source,window,window_minutes,used_pct,remaining_pct,projected_pct_at_reset,status,observed_at,resets_at,estimated_depletion_at\n\
codex,weekly,{},{:.2},{:.2},{:.2},{},{},{},{}\n",
            report.window_minutes,
            report.used_pct,
            report.remaining_pct,
            report.projected_pct_at_reset,
            report.status.as_str(),
            timestamp(report.observed_at),
            timestamp(report.resets_at),
            depletion,
        );
    };
    let (observed_cost, weekly_value, observed_tokens, weekly_tokens, window_start, error) =
        match value_estimate {
            Ok(estimate) => (
                format!("{:.6}", estimate.observed_cost_usd),
                format!("{:.6}", estimate.estimated_weekly_value_usd),
                estimate.observed_tokens.to_string(),
                format!("{:.0}", estimate.estimated_weekly_tokens),
                timestamp(estimate.window_started_at),
                String::new(),
            ),
            Err(error) => (
                String::new(),
                String::new(),
                String::new(),
                String::new(),
                String::new(),
                csv_escape(&error.to_string()),
            ),
        };
    format!(
        "source,window,window_minutes,used_pct,remaining_pct,projected_pct_at_reset,status,observed_at,resets_at,estimated_depletion_at,observed_cost_usd,estimated_weekly_value_usd,observed_tokens,estimated_weekly_tokens,value_window_started_at,value_estimate_error\n\
codex,weekly,{},{:.2},{:.2},{:.2},{},{},{},{},{},{},{},{},{},{}\n",
        report.window_minutes,
        report.used_pct,
        report.remaining_pct,
        report.projected_pct_at_reset,
        report.status.as_str(),
        timestamp(report.observed_at),
        timestamp(report.resets_at),
        depletion,
        observed_cost,
        weekly_value,
        observed_tokens,
        weekly_tokens,
        window_start,
        error,
    )
}

pub(crate) fn print_quota_table(
    report: &CodexWeeklyQuota,
    value_estimate: QuotaValueEstimate<'_>,
    timezone: Timezone,
    number_format: NumberFormat,
    use_color: bool,
) {
    let mut table = create_styled_table();
    table.set_header(vec![
        header_cell("Window", use_color),
        header_cell("Used", use_color),
        header_cell("Remaining", use_color),
        header_cell("Projected", use_color),
        header_cell("Status", use_color),
        header_cell("Resets", use_color),
        header_cell("Est. depletion", use_color),
    ]);

    let status_color = if use_color {
        match report.status {
            CodexQuotaStatus::OnTrack => Some(Color::Green),
            CodexQuotaStatus::Watch => Some(Color::Yellow),
            CodexQuotaStatus::LikelyExhausted | CodexQuotaStatus::Exhausted => Some(Color::Red),
        }
    } else {
        None
    };
    let depletion = report.estimated_depletion_at.map_or_else(
        || "—".to_string(),
        |value| {
            timezone
                .to_fixed_offset(value)
                .format("%Y-%m-%d %H:%M %:z")
                .to_string()
        },
    );

    table.add_row(vec![
        Cell::new("7 days"),
        right_cell(&format!("{:.1}%", report.used_pct), status_color, false),
        right_cell(&format!("{:.1}%", report.remaining_pct), None, false),
        right_cell(
            &format!("{:.1}%", report.projected_pct_at_reset),
            status_color,
            false,
        ),
        styled_cell(report.status.as_str(), status_color, true),
        Cell::new(
            timezone
                .to_fixed_offset(report.resets_at)
                .format("%Y-%m-%d %H:%M %:z")
                .to_string(),
        ),
        Cell::new(depletion),
    ]);

    println!("{table}");
    println!(
        "Observed: {} · Projection is a pace estimate, not a token allowance.",
        timezone
            .to_fixed_offset(report.observed_at)
            .format("%Y-%m-%d %H:%M:%S %:z")
    );
    match value_estimate {
        Some(Ok(estimate)) => {
            println!(
                "API-equivalent weekly value: ≈${:.2} (observed ${:.2} at {:.1}% used)",
                estimate.estimated_weekly_value_usd, estimate.observed_cost_usd, estimate.used_pct,
            );
            println!(
                "Estimated weekly tokens at the current model/cache mix: ≈{} (observed {})",
                format_compact(
                    estimate.estimated_weekly_tokens.round() as i64,
                    number_format
                ),
                format_compact(estimate.observed_tokens, number_format),
            );
            println!(
                "Value window: {} to the quota observation above.",
                timezone
                    .to_fixed_offset(estimate.window_started_at)
                    .format("%Y-%m-%d %H:%M:%S %:z")
            );
            println!(
                "Dollar and token values are local API-pricing estimates, not official provider allowances."
            );
        }
        Some(Err(error)) => println!("API-equivalent weekly value unavailable: {error}"),
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use chrono::DateTime;

    use super::*;
    fn report() -> CodexWeeklyQuota {
        CodexWeeklyQuota {
            observed_at: "2026-08-22T00:00:00Z".parse::<DateTime<_>>().unwrap(),
            resets_at: "2026-08-28T00:00:00Z".parse::<DateTime<_>>().unwrap(),
            estimated_depletion_at: Some("2026-08-25T00:00:00Z".parse::<DateTime<_>>().unwrap()),
            window_minutes: 10_080,
            used_pct: 25.0,
            remaining_pct: 75.0,
            projected_pct_at_reset: 175.0,
            status: CodexQuotaStatus::LikelyExhausted,
        }
    }

    fn value_estimate() -> CodexWeeklyValueEstimate {
        CodexWeeklyValueEstimate {
            observed_at: "2026-08-22T00:00:00Z".parse::<DateTime<_>>().unwrap(),
            window_started_at: "2026-08-21T00:00:00Z".parse::<DateTime<_>>().unwrap(),
            resets_at: "2026-08-28T00:00:00Z".parse::<DateTime<_>>().unwrap(),
            used_pct: 25.0,
            observed_cost_usd: 50.0,
            estimated_weekly_value_usd: 200.0,
            observed_tokens: 1_000_000,
            estimated_weekly_tokens: 4_000_000.0,
            valid_entries: 12,
            dedup_skipped_entries: 2,
        }
    }

    #[test]
    fn json_contains_machine_readable_quota_fields() {
        let estimate = value_estimate();
        let value: serde_json::Value =
            serde_json::from_str(&output_quota_json(&report(), Some(Ok(&estimate)))).unwrap();

        assert_eq!(value["window"], "weekly");
        assert_eq!(value["used_pct"], 25.0);
        assert_eq!(value["projected_pct_at_reset"], 175.0);
        assert_eq!(value["status"], "likely_exhausted");
        assert_eq!(value["resets_at"], "2026-08-28T00:00:00Z");
        assert_eq!(value["value_estimate"]["kind"], "api_equivalent");
        assert_eq!(value["value_estimate"]["observed_cost_usd"], 50.0);
        assert_eq!(value["value_estimate"]["estimated_weekly_value_usd"], 200.0);
        assert_eq!(
            value["value_estimate"]["estimated_weekly_tokens"],
            4_000_000.0
        );
        assert!(value["value_estimate_error"].is_null());
    }

    #[test]
    fn csv_has_stable_header_and_values() {
        let estimate = value_estimate();
        let csv = output_quota_csv(&report(), Some(Ok(&estimate)));
        let mut lines = csv.lines();

        assert_eq!(
            lines.next(),
            Some(
                "source,window,window_minutes,used_pct,remaining_pct,projected_pct_at_reset,status,observed_at,resets_at,estimated_depletion_at,observed_cost_usd,estimated_weekly_value_usd,observed_tokens,estimated_weekly_tokens,value_window_started_at,value_estimate_error"
            )
        );
        let values = lines.next().unwrap();
        assert!(values.starts_with("codex,weekly,10080,25.00,75.00,175.00,likely_exhausted"));
        assert!(values.contains(",50.000000,200.000000,1000000,4000000,"));
    }

    #[test]
    fn json_exposes_value_estimate_error_without_hiding_quota() {
        let error = CodexWeeklyValueError::ZeroUsagePercentage;
        let value: serde_json::Value =
            serde_json::from_str(&output_quota_json(&report(), Some(Err(&error)))).unwrap();

        assert_eq!(value["used_pct"], 25.0);
        assert!(value["value_estimate"].is_null());
        assert!(
            value["value_estimate_error"]
                .as_str()
                .unwrap()
                .contains("used percentage is zero")
        );
    }
}
