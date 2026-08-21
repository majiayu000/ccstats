use comfy_table::{Cell, Color};
use serde_json::json;

use crate::source::{CodexQuotaStatus, CodexWeeklyQuota};
use crate::utils::Timezone;

use super::format::{create_styled_table, header_cell, right_cell, styled_cell};

fn rounded_pct(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

fn timestamp(value: chrono::DateTime<chrono::Utc>) -> String {
    value.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

pub(crate) fn output_quota_json(report: &CodexWeeklyQuota) -> String {
    json!({
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
    })
    .to_string()
}

pub(crate) fn output_quota_csv(report: &CodexWeeklyQuota) -> String {
    let depletion = report
        .estimated_depletion_at
        .map(timestamp)
        .unwrap_or_default();
    format!(
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
    )
}

pub(crate) fn print_quota_table(report: &CodexWeeklyQuota, timezone: Timezone, use_color: bool) {
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

    #[test]
    fn json_contains_machine_readable_quota_fields() {
        let value: serde_json::Value = serde_json::from_str(&output_quota_json(&report())).unwrap();

        assert_eq!(value["window"], "weekly");
        assert_eq!(value["used_pct"], 25.0);
        assert_eq!(value["projected_pct_at_reset"], 175.0);
        assert_eq!(value["status"], "likely_exhausted");
        assert_eq!(value["resets_at"], "2026-08-28T00:00:00Z");
    }

    #[test]
    fn csv_has_stable_header_and_values() {
        let csv = output_quota_csv(&report());
        let mut lines = csv.lines();

        assert_eq!(
            lines.next(),
            Some(
                "source,window,window_minutes,used_pct,remaining_pct,projected_pct_at_reset,status,observed_at,resets_at,estimated_depletion_at"
            )
        );
        assert!(
            lines
                .next()
                .unwrap()
                .starts_with("codex,weekly,10080,25.00,75.00,175.00,likely_exhausted")
        );
    }
}
