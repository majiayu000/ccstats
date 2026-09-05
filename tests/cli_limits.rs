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
fn limits_rejects_cursor_source() {
    let home = unique_temp_dir("limits-cursor-source");
    let (ok, _stdout, stderr) = run_home(&home, &["limits", "--source", "cursor"]);
    assert!(!ok);
    let err = String::from_utf8_lossy(&stderr);
    assert!(
        err.contains("limits does not support --source cursor"),
        "{err}"
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
    }
    fs::remove_dir_all(home).unwrap();
}
