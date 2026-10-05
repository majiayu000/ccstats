mod common;

use std::fs;
use std::path::Path;

use chrono::{Duration, SecondsFormat, Timelike, Utc};
use common::{run_ccstats, run_ccstats_with_stdin, unique_temp_dir, write_file};
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

fn write_codex_quota(home: &Path) {
    let observed_at = Utc::now().with_nanosecond(0).unwrap();
    let resets_at = observed_at + Duration::days(6);
    write_file(
        &home.join(".codex/sessions/newer.jsonl"),
        &quota_event(observed_at, 25.0, resets_at, true),
    );
}

fn write_active_claude_block(home: &Path) {
    let timestamp = Utc::now()
        .with_nanosecond(0)
        .unwrap()
        .to_rfc3339_opts(SecondsFormat::Secs, true);
    write_file(
        &home.join(".claude/projects/limits-app/session.jsonl"),
        &format!(
            r#"{{"timestamp":"{timestamp}","message":{{"id":"msg_limits_1","model":"claude-3-5-sonnet-20241022","stop_reason":"end_turn","usage":{{"input_tokens":100,"output_tokens":50,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}}}}
"#
        ),
    );
}

fn run_home(home: &Path, args: &[&str]) -> (bool, Vec<u8>, Vec<u8>) {
    run_ccstats(args, &[("HOME", home)])
}

#[test]
fn limits_codex_only_home_shows_quota_without_fake_claude_percent() {
    let home = unique_temp_dir("limits-codex-only");
    write_codex_quota(&home);

    let (ok, stdout, stderr) = run_home(
        &home,
        &["limits", "--offline", "--no-color", "--timezone", "UTC"],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    let text = String::from_utf8_lossy(&stdout);
    assert!(text.contains("Codex weekly quota"), "{text}");
    assert!(text.contains("25.0%"), "{text}");
    assert!(text.contains("Claude estimated session window"), "{text}");
    assert!(text.contains("No active estimated 5-hour window"), "{text}");
    assert!(text.contains("not an official"), "{text}");

    let (json_ok, json_stdout, json_stderr) = run_home(&home, &["limits", "--json", "--offline"]);
    assert!(json_ok, "stderr: {}", String::from_utf8_lossy(&json_stderr));
    let value: Value = serde_json::from_slice(&json_stdout).unwrap();
    assert_eq!(value["codex"]["used_pct"], 25.0);
    assert!(value["claude_blocks"].is_null());

    let _ = fs::remove_dir_all(home);
}

#[test]
fn limits_claude_only_json_codex_is_null_and_table_has_disclaimer() {
    let home = unique_temp_dir("limits-claude-only");
    write_active_claude_block(&home);

    let (ok, stdout, stderr) = run_home(
        &home,
        &[
            "limits",
            "--offline",
            "--no-color",
            "--no-cost",
            "--timezone",
            "UTC",
        ],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    let text = String::from_utf8_lossy(&stdout);
    assert!(text.contains("Claude estimated session window"), "{text}");
    assert!(text.contains("not an official"), "{text}");
    assert!(text.contains("Codex weekly quota"), "{text}");
    assert!(text.contains("unavailable:"), "{text}");

    let (json_ok, json_stdout, json_stderr) = run_home(
        &home,
        &[
            "limits",
            "--json",
            "--offline",
            "--no-cost",
            "--timezone",
            "UTC",
        ],
    );
    assert!(json_ok, "stderr: {}", String::from_utf8_lossy(&json_stderr));
    let value: Value = serde_json::from_slice(&json_stdout).unwrap();
    assert!(
        value["codex"].is_null(),
        "missing Codex must be null: {value}"
    );
    assert!(
        value["codex"].get("used_pct").is_none(),
        "missing Codex must not be used_pct=0: {value}"
    );
    assert_eq!(value["claude_blocks"]["total_tokens"], 150);
    assert!(
        value["claude_blocks"]["disclaimer"]
            .as_str()
            .unwrap()
            .contains("not an official")
    );
    assert!(
        !value["claude_blocks"]
            .as_object()
            .unwrap()
            .contains_key("used_pct")
    );

    let _ = fs::remove_dir_all(home);
}

#[test]
fn limits_source_cursor_json_is_null_without_credentials() {
    let home = unique_temp_dir("limits-cursor-source");
    let (ok, stdout, stderr) = run_home(
        &home,
        &[
            "limits",
            "--source",
            "cursor",
            "--json",
            "--offline",
            "--no-cost",
        ],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    let value: Value = serde_json::from_slice(&stdout).unwrap();
    assert!(value["cursor"].is_null(), "{value}");
    assert!(value["windows"].as_array().unwrap().is_empty());
    assert!(
        value["notes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|note| note.as_str().unwrap_or_default().contains("Cursor")),
        "{value}"
    );

    let _ = fs::remove_dir_all(home);
}

#[test]
fn limits_source_claude_omits_codex_attempt_as_null() {
    let home = unique_temp_dir("limits-source-claude");
    write_active_claude_block(&home);

    let (ok, stdout, stderr) = run_home(
        &home,
        &[
            "limits",
            "--source",
            "claude",
            "--json",
            "--offline",
            "--no-cost",
        ],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    let value: Value = serde_json::from_slice(&stdout).unwrap();
    assert!(value["codex"].is_null());
    assert_eq!(value["claude_blocks"]["total_tokens"], 150);

    let _ = fs::remove_dir_all(home);
}

#[test]
fn limits_and_watch_do_not_scale_local_cost_into_official_claude_windows() {
    let home = unique_temp_dir("limits-official-claude-cost");
    let data = home.join("data");
    let envs = [("HOME", home.as_path()), ("XDG_DATA_HOME", data.as_path())];
    write_active_claude_block(&home);

    let (ok, stdout, stderr) = run_ccstats(
        &["limits", "--source", "claude", "--json", "--offline"],
        &envs,
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    let local: Value = serde_json::from_slice(&stdout).unwrap();
    let local_cost = local["claude_blocks"]["cost"].as_f64().unwrap();
    assert!(local_cost > 0.0, "{local}");
    assert!(
        local["claude_blocks"]["remaining_minutes"]
            .as_i64()
            .unwrap()
            > 60
    );
    assert_eq!(local["windows"][0]["source"], "estimated");
    assert_eq!(local["windows"][0]["value_estimate_usd"], local_cost);

    let now = Utc::now().with_nanosecond(0).unwrap();
    let five_reset = now + Duration::hours(1);
    let seven_reset = now + Duration::days(1);
    let hook = json!({
        "rate_limits": {
            "five_hour": {"used_percentage": 80.0, "resets_at": five_reset.timestamp()},
            "seven_day": {"used_percentage": 90.0, "resets_at": seven_reset.timestamp()},
        }
    });
    let (ok, _, stderr) = run_ccstats_with_stdin(
        &["statusline", "--source", "claude", "--json", "--offline"],
        &envs,
        &hook.to_string(),
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));

    for command in [vec!["limits"], vec!["watch", "--once"]] {
        for hide_cost in [false, true] {
            let mut args = command.clone();
            args.extend(["--source", "claude", "--json", "--offline"]);
            if hide_cost {
                args.push("--no-cost");
            }
            let (ok, stdout, stderr) = run_ccstats(&args, &envs);
            assert_eq!(
                ok,
                command[0] == "limits",
                "watch must retain its quota-warning exit: {}",
                String::from_utf8_lossy(&stderr)
            );
            let value: Value = serde_json::from_slice(&stdout).unwrap();
            if command[0] == "watch" {
                assert_eq!(value["hot"], true);
            }
            let windows = value["windows"].as_array().unwrap();
            assert_eq!(windows.len(), 2, "{value}");
            for (name, used_pct, reset) in [
                ("five_hour", 80.0, five_reset),
                ("seven_day", 90.0, seven_reset),
            ] {
                let window = windows.iter().find(|row| row["window"] == name).unwrap();
                assert_eq!(window["source"], "official");
                assert_eq!(window["used_pct"], used_pct);
                assert_eq!(
                    window["resets_at"],
                    reset.to_rfc3339_opts(SecondsFormat::Secs, true)
                );
                assert_eq!(window["stale"], false);
                assert!(
                    window.get("value_estimate_usd").is_none(),
                    "local cost must not be scaled into an official window: {window}"
                );
            }
            if command[0] == "limits" && !hide_cost {
                assert_eq!(value["claude_blocks"]["cost"], local_cost);
            }
        }
    }
    fs::remove_dir_all(home).unwrap();
}

#[test]
fn limits_csv_blanks_missing_codex_instead_of_zero() {
    let home = unique_temp_dir("limits-csv-missing-codex");
    write_active_claude_block(&home);

    let (ok, stdout, stderr) = run_home(
        &home,
        &[
            "limits",
            "--csv",
            "--offline",
            "--no-cost",
            "--timezone",
            "UTC",
        ],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    let csv = String::from_utf8(stdout).unwrap();
    let mut lines = csv.lines();
    let header = lines.next().unwrap();
    assert!(header.starts_with("section,source,window,"));
    let codex = lines.next().unwrap();
    let fields: Vec<_> = codex.split(',').collect();
    assert_eq!(fields[0], "codex");
    assert_eq!(
        fields[4], "",
        "used_pct must be blank when Codex is missing: {codex}"
    );
    assert_eq!(fields[5], "", "remaining_pct must be blank: {codex}");
    assert_eq!(fields[6], "", "projected_pct must be blank: {codex}");

    let _ = fs::remove_dir_all(home);
}

#[test]
fn limits_both_missing_tells_user_to_run_doctor() {
    let home = unique_temp_dir("limits-both-missing");
    fs::create_dir_all(home.join(".empty")).unwrap();

    let (ok, stdout, stderr) = run_home(&home, &["limits", "--no-color", "--offline"]);
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    let text = String::from_utf8_lossy(&stdout);
    assert!(text.contains("ccstats doctor"), "{text}");
    assert!(!text.contains("0%"), "{text}");

    let _ = fs::remove_dir_all(home);
}

#[test]
fn quota_help_is_codex_only_and_limits_help_is_combined() {
    let home = unique_temp_dir("limits-help");
    let (quota_ok, quota_stdout, quota_stderr) = run_home(&home, &["quota", "--help"]);
    assert!(
        quota_ok,
        "stderr: {}",
        String::from_utf8_lossy(&quota_stderr)
    );
    let quota_help = String::from_utf8_lossy(&quota_stdout);
    assert!(
        quota_help.to_ascii_lowercase().contains("codex-only") || quota_help.contains("Codex-only"),
        "{quota_help}"
    );

    let (limits_ok, limits_stdout, limits_stderr) = run_home(&home, &["limits", "--help"]);
    assert!(
        limits_ok,
        "stderr: {}",
        String::from_utf8_lossy(&limits_stderr)
    );
    let limits_help = String::from_utf8_lossy(&limits_stdout);
    assert!(
        limits_help.contains("claude") || limits_help.contains("Claude"),
        "{limits_help}"
    );
    assert!(
        limits_help.contains("codex") || limits_help.contains("Codex"),
        "{limits_help}"
    );

    let _ = fs::remove_dir_all(home);
}

#[test]
fn limits_empty_home_keeps_its_schema_and_ignores_unrelated_credentials() {
    let home = unique_temp_dir("limits-empty-json");
    for corrupt_cursor in [false, true] {
        if corrupt_cursor {
            write_file(
                &home.join(".config/ccstats/credentials.toml"),
                "[cursor]\napi_key = \"unterminated",
            );
        }
        let (ok, stdout, stderr) = run_home(&home, &["limits", "--json", "--offline", "--no-cost"]);
        assert!(ok, "{}", String::from_utf8_lossy(&stderr));
        let value: Value = serde_json::from_slice(&stdout).unwrap();
        assert!(
            value.is_object(),
            "limits must not dispatch doctor: {value}"
        );
        assert!(value.get("codex").unwrap().is_null());
        assert!(value.get("claude_blocks").unwrap().is_null());
        assert!(value.get("cursor").unwrap().is_null());
        assert!(value.get("windows").unwrap().as_array().unwrap().is_empty());
    }
    fs::remove_dir_all(home).unwrap();
}

#[test]
fn limits_codex_forecast_uses_per_session_snapshot_history() {
    let home = unique_temp_dir("limits-codex-forecast");
    let now = Utc::now().with_nanosecond(0).unwrap();
    let resets_at = now + Duration::days(2);
    for (name, hours_ago, used) in [("a", 2, 10.0), ("b", 1, 20.0), ("c", 0, 30.0)] {
        write_file(
            &home.join(format!(".codex/sessions/{name}.jsonl")),
            &quota_event(now - Duration::hours(hours_ago), used, resets_at, true),
        );
    }

    let (ok, stdout, stderr) = run_home(
        &home,
        &[
            "limits",
            "--source",
            "codex",
            "--json",
            "--offline",
            "--no-cost",
        ],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    let value: Value = serde_json::from_slice(&stdout).unwrap();
    let forecast = &value["windows"][0]["forecast"];
    assert_eq!(forecast["basis"], "snapshot_history", "{value}");
    assert_eq!(forecast["confidence"], "medium");
    assert_eq!(forecast["samples"], 3);
    assert_eq!(forecast["burn_pct_per_hour"], 10.0);
    assert_eq!(forecast["exhausts_before_reset"], true);
    assert_eq!(forecast["source"], "estimated");
    assert!(forecast["reason"].is_null());
    let projected = forecast["projected_exhaustion_at"].as_str().unwrap();
    let projected: chrono::DateTime<Utc> = projected.parse().unwrap();
    assert_eq!(projected, now + Duration::hours(7));

    let (ok, stdout, stderr) = run_home(
        &home,
        &[
            "limits",
            "--source",
            "codex",
            "--offline",
            "--no-color",
            "--no-cost",
            "--timezone",
            "UTC",
        ],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    let text = String::from_utf8_lossy(&stdout);
    assert!(text.contains("Limit forecast (est.)"), "{text}");
    assert!(text.contains("10.0%/h est."), "{text}");
    assert!(text.contains("snapshot_history"), "{text}");

    let (ok, stdout, _) = run_home(
        &home,
        &[
            "watch",
            "--once",
            "--source",
            "codex",
            "--offline",
            "--no-color",
            "--no-cost",
            "--warn-pct",
            "0",
        ],
    );
    assert!(ok);
    let text = String::from_utf8_lossy(&stdout);
    assert!(text.contains("pace 10.0%/h est."), "{text}");
    assert!(
        text.contains("warning: codex weekly projected to run out before reset"),
        "{text}"
    );
    let (ok, stdout, _) = run_home(
        &home,
        &[
            "watch",
            "--once",
            "--json",
            "--source",
            "codex",
            "--offline",
            "--no-cost",
            "--warn-pct",
            "0",
        ],
    );
    assert!(ok);
    let value: Value = serde_json::from_slice(&stdout).unwrap();
    assert_eq!(value["exhaustion_warning"], true);

    fs::remove_dir_all(home).unwrap();
}

#[test]
fn limits_claude_forecast_is_null_with_reason_when_history_is_too_short() {
    let home = unique_temp_dir("limits-claude-forecast-null");
    let data = home.join("data");
    let envs = [("HOME", home.as_path()), ("XDG_DATA_HOME", data.as_path())];
    let now = Utc::now().with_nanosecond(0).unwrap();
    let five_reset = now + Duration::hours(5) - Duration::minutes(5);
    let snapshot = json!({
        "captured_at": now.to_rfc3339_opts(SecondsFormat::Secs, true),
        "version": null,
        "five_hour": {"used_percentage": 3.0, "resets_at": five_reset.timestamp()},
        "seven_day": null,
    });
    write_file(
        &data.join("ccstats/quota/claude.jsonl"),
        &format!("{snapshot}\n"),
    );

    let (ok, stdout, stderr) = run_ccstats(
        &[
            "limits",
            "--source",
            "claude",
            "--json",
            "--offline",
            "--no-cost",
        ],
        &envs,
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    let value: Value = serde_json::from_slice(&stdout).unwrap();
    let window = value["windows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["window"] == "five_hour")
        .unwrap();
    let forecast = &window["forecast"];
    assert_eq!(forecast["reason"], "insufficient_history", "{value}");
    assert!(forecast["burn_pct_per_hour"].is_null());
    assert!(forecast["projected_exhaustion_at"].is_null());
    assert!(forecast["exhausts_before_reset"].is_null());

    let (ok, stdout, _) = run_ccstats(
        &[
            "limits",
            "--source",
            "claude",
            "--offline",
            "--no-color",
            "--no-cost",
            "--timezone",
            "UTC",
        ],
        &envs,
    );
    assert!(ok);
    let text = String::from_utf8_lossy(&stdout);
    assert!(text.contains("unknown (insufficient history)"), "{text}");

    fs::remove_dir_all(home).unwrap();
}
