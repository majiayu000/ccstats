mod common;

use std::fs;
use std::path::Path;

use chrono::{Duration, SecondsFormat, Timelike, Utc};
use common::{run_ccstats, unique_temp_dir, write_file};
use serde_json::{Value, json};

fn quota_event(
    observed_at: chrono::DateTime<Utc>,
    used_pct: f64,
    resets_at: chrono::DateTime<Utc>,
    weekly_in_secondary: bool,
) -> String {
    let weekly = json!({
        "used_percent": used_pct,
        "window_minutes": 10_080,
        "resets_at": resets_at.timestamp(),
    });
    let rate_limits = if weekly_in_secondary {
        json!({
            "primary": {
                "used_percent": 10.0,
                "window_minutes": 300,
                "resets_at": (observed_at + Duration::hours(4)).timestamp(),
            },
            "secondary": weekly,
        })
    } else {
        json!({"primary": weekly, "secondary": null})
    };

    format!(
        "{}\n",
        json!({
            "timestamp": observed_at.to_rfc3339_opts(SecondsFormat::Secs, true),
            "type": "event_msg",
            "payload": {
                "type": "token_count",
                "info": {
                    "total_token_usage": {
                        "input_tokens": 1_000_000,
                        "cached_input_tokens": 200_000,
                        "output_tokens": 100_000,
                        "reasoning_output_tokens": 50_000,
                        "total_tokens": 1_100_000,
                    },
                    "last_token_usage": {
                        "input_tokens": 1_000_000,
                        "cached_input_tokens": 200_000,
                        "output_tokens": 100_000,
                        "reasoning_output_tokens": 50_000,
                        "total_tokens": 1_100_000,
                    },
                    "model": "gpt-5",
                },
                "rate_limits": rate_limits,
            },
        })
    )
}

fn write_current_quota_fixture(codex_home: &Path) -> chrono::DateTime<Utc> {
    let observed_at = Utc::now().with_nanosecond(0).unwrap();
    let resets_at = observed_at + Duration::days(6);
    write_file(
        &codex_home.join("sessions/older.jsonl"),
        &quota_event(observed_at - Duration::hours(1), 20.0, resets_at, false),
    );
    write_file(
        &codex_home.join("sessions/newer.jsonl"),
        &quota_event(observed_at, 25.0, resets_at, true),
    );
    resets_at
}

#[test]
fn quota_and_codex_quota_return_the_same_json() {
    let root = unique_temp_dir("codex-quota-json");
    let codex_home = root.join("codex-home");
    let resets_at = write_current_quota_fixture(&codex_home);

    let (top_ok, top_stdout, top_stderr) = run_ccstats(
        &["quota", "--json", "--offline"],
        &[("CODEX_HOME", &codex_home)],
    );
    assert!(top_ok, "stderr: {}", String::from_utf8_lossy(&top_stderr));

    let (nested_ok, nested_stdout, nested_stderr) = run_ccstats(
        &["codex", "quota", "--json", "--offline"],
        &[("CODEX_HOME", &codex_home)],
    );
    assert!(
        nested_ok,
        "stderr: {}",
        String::from_utf8_lossy(&nested_stderr)
    );
    assert_eq!(top_stdout, nested_stdout);

    let value: Value = serde_json::from_slice(&top_stdout).unwrap();
    assert_eq!(value["source"], "codex");
    assert_eq!(value["window"], "weekly");
    assert_eq!(value["window_minutes"], 10_080);
    assert_eq!(value["used_pct"], 25.0);
    assert_eq!(value["remaining_pct"], 75.0);
    assert_eq!(value["projected_pct_at_reset"], 175.0);
    assert_eq!(value["status"], "likely_exhausted");
    assert_eq!(
        value["resets_at"],
        resets_at.to_rfc3339_opts(SecondsFormat::Secs, true)
    );
    assert!(value["estimated_depletion_at"].is_string());
    assert_eq!(value["value_estimate"]["kind"], "api_equivalent");
    let estimate = &value["value_estimate"];
    let observed_cost = estimate["observed_cost_usd"].as_f64().unwrap();
    let weekly_value = estimate["estimated_weekly_value_usd"].as_f64().unwrap();
    let observed_tokens = estimate["observed_tokens"].as_f64().unwrap();
    let weekly_tokens = estimate["estimated_weekly_tokens"].as_f64().unwrap();
    assert!(observed_cost > 0.0);
    assert!((weekly_value / observed_cost - 4.0).abs() < 0.001);
    assert!(observed_tokens > 0.0);
    assert!((weekly_tokens / observed_tokens - 4.0).abs() < f64::EPSILON);
    assert_eq!(
        estimate["window_started_at"],
        (resets_at - Duration::minutes(10_080)).to_rfc3339_opts(SecondsFormat::Secs, true)
    );
    assert!(value["value_estimate_error"].is_null());

    let _ = fs::remove_dir_all(root);
}

#[test]
fn quota_csv_exposes_stable_columns() {
    let root = unique_temp_dir("codex-quota-csv");
    let codex_home = root.join("codex-home");
    write_current_quota_fixture(&codex_home);

    let (ok, stdout, stderr) = run_ccstats(
        &["quota", "--csv", "--offline"],
        &[("CODEX_HOME", &codex_home)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    let csv = String::from_utf8(stdout).unwrap();
    let lines: Vec<_> = csv.lines().collect();
    assert_eq!(
        lines[0],
        "source,window,window_minutes,used_pct,remaining_pct,projected_pct_at_reset,status,observed_at,resets_at,estimated_depletion_at,observed_cost_usd,estimated_weekly_value_usd,observed_tokens,estimated_weekly_tokens,value_window_started_at,value_estimate_error"
    );
    assert!(lines[1].starts_with("codex,weekly,10080,25.00,75.00,175.00,likely_exhausted"));
    let columns: Vec<_> = lines[1].split(',').collect();
    let observed_cost: f64 = columns[10].parse().unwrap();
    let weekly_value: f64 = columns[11].parse().unwrap();
    let observed_tokens: f64 = columns[12].parse().unwrap();
    let weekly_tokens: f64 = columns[13].parse().unwrap();
    assert!((weekly_value / observed_cost - 4.0).abs() < 0.001);
    assert!((weekly_tokens / observed_tokens - 4.0).abs() < f64::EPSILON);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn quota_no_cost_omits_value_estimate() {
    let root = unique_temp_dir("codex-quota-no-cost");
    let codex_home = root.join("codex-home");
    write_current_quota_fixture(&codex_home);

    let (ok, stdout, stderr) = run_ccstats(
        &["quota", "--json", "--no-cost", "--offline"],
        &[("CODEX_HOME", &codex_home)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    let value: Value = serde_json::from_slice(&stdout).unwrap();
    assert!(value.get("value_estimate").is_none());
    assert!(value.get("value_estimate_error").is_none());

    let (csv_ok, csv_stdout, csv_stderr) = run_ccstats(
        &["quota", "--csv", "--no-cost", "--offline"],
        &[("CODEX_HOME", &codex_home)],
    );
    assert!(csv_ok, "stderr: {}", String::from_utf8_lossy(&csv_stderr));
    let csv = String::from_utf8(csv_stdout).unwrap();
    assert_eq!(
        csv.lines().next().unwrap(),
        "source,window,window_minutes,used_pct,remaining_pct,projected_pct_at_reset,status,observed_at,resets_at,estimated_depletion_at"
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn quota_rejects_non_usd_currency() {
    let root = unique_temp_dir("codex-quota-currency");
    let codex_home = root.join("codex-home");
    write_current_quota_fixture(&codex_home);

    let (ok, _stdout, stderr) = run_ccstats(
        &["quota", "--currency", "EUR", "--offline"],
        &[("CODEX_HOME", &codex_home)],
    );
    assert!(!ok);
    assert!(
        String::from_utf8_lossy(&stderr)
            .contains("--currency is not supported for quota estimates")
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn quota_fails_when_weekly_snapshot_is_missing() {
    let root = unique_temp_dir("codex-quota-missing");
    let codex_home = root.join("codex-home");
    fs::create_dir_all(codex_home.join("sessions")).unwrap();

    let (ok, _stdout, stderr) = run_ccstats(&["quota", "--json"], &[("CODEX_HOME", &codex_home)]);
    assert!(!ok);
    assert!(String::from_utf8_lossy(&stderr).contains("no Codex weekly quota snapshot was found"));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn quota_does_not_fall_back_when_explicit_codex_home_is_missing() {
    let root = unique_temp_dir("codex-quota-missing-home");
    let codex_home = root.join("does-not-exist");
    let observed_at = Utc::now().with_nanosecond(0).unwrap();
    write_file(
        &root.join(".codex/sessions/fallback.jsonl"),
        &quota_event(observed_at, 25.0, observed_at + Duration::days(6), false),
    );

    let (ok, _stdout, stderr) = run_ccstats(
        &["quota", "--json"],
        &[("HOME", &root), ("CODEX_HOME", &codex_home)],
    );

    assert!(!ok);
    assert!(String::from_utf8_lossy(&stderr).contains("no Codex weekly quota snapshot was found"));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn quota_fails_when_latest_snapshot_is_stale() {
    let root = unique_temp_dir("codex-quota-stale");
    let codex_home = root.join("codex-home");
    let observed_at = Utc::now() - Duration::days(2);
    let resets_at = Utc::now() - Duration::days(1);
    write_file(
        &codex_home.join("sessions/stale.jsonl"),
        &quota_event(observed_at, 80.0, resets_at, false),
    );

    let (ok, _stdout, stderr) = run_ccstats(&["quota", "--json"], &[("CODEX_HOME", &codex_home)]);
    assert!(!ok);
    assert!(String::from_utf8_lossy(&stderr).contains("snapshot expired"));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn quota_rejects_non_codex_source_override() {
    let root = unique_temp_dir("codex-quota-source");
    let codex_home = root.join("codex-home");
    write_current_quota_fixture(&codex_home);

    let (ok, _stdout, stderr) = run_ccstats(
        &["quota", "--source", "claude"],
        &[("CODEX_HOME", &codex_home)],
    );
    assert!(!ok);
    assert!(String::from_utf8_lossy(&stderr).contains("conflicts with --source 'claude'"));

    let _ = fs::remove_dir_all(root);
}
