mod common;

use chrono::Utc;
use common::{run_ccstats, run_ccstats_with_stdin, unique_temp_dir, write_file};
use serde_json::Value;
use std::fs;

fn write_today_claude(root: &std::path::Path) {
    let today = Utc::now().format("%Y-%m-%dT12:00:00Z");
    write_file(
        &root.join(".claude/projects/hook-app/session.jsonl"),
        &format!(
            r#"{{"timestamp":"{today}","message":{{"id":"msg_hook","model":"claude-3-5-sonnet-20241022","stop_reason":"end_turn","usage":{{"input_tokens":100,"output_tokens":50}}}}}}
"#
        ),
    );
}

#[test]
fn statusline_hook_json_marks_official_windows() {
    let home = unique_temp_dir("statusline-hook-json");
    write_today_claude(&home);
    let stdin = r#"{
        "version": "2.1.80",
        "cost": {"total_cost_usd": 1.5},
        "context_window": {"used_percentage": 42.0},
        "rate_limits": {
            "five_hour": {"used_percentage": 23.0, "resets_at": 4102444800},
            "seven_day": {"used_percentage": 41.0, "resets_at": 4102531200}
        }
    }"#;
    let (ok, stdout, stderr) = run_ccstats_with_stdin(
        &[
            "statusline",
            "--source",
            "claude",
            "--json",
            "--offline",
            "--timezone",
            "UTC",
        ],
        &[("HOME", &home)],
        stdin,
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    let value: Value = serde_json::from_slice(&stdout).unwrap();
    assert_eq!(value["confidence"], "official");
    assert_eq!(value["context_used_pct"], 42.0);
    assert_eq!(value["rate_limits"]["five_hour"]["used_pct"], 23.0);
    assert_eq!(value["cc_cost"], 1.5);

    let _ = fs::remove_dir_all(home);
}

#[test]
fn statusline_text_includes_ctx_and_windows() {
    let home = unique_temp_dir("statusline-hook-text");
    write_today_claude(&home);
    let stdin = r#"{
        "cost": {"total_cost_usd": 0.2},
        "context_window": {"used_percentage": 10.0},
        "rate_limits": {
            "five_hour": {"used_percentage": 5.0, "resets_at": 4102444800},
            "seven_day": {"used_percentage": 8.0}
        }
    }"#;
    let (ok, stdout, stderr) = run_ccstats_with_stdin(
        &[
            "statusline",
            "--source",
            "claude",
            "--offline",
            "--no-color",
            "--timezone",
            "UTC",
        ],
        &[("HOME", &home)],
        stdin,
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    let text = String::from_utf8_lossy(&stdout);
    assert!(text.contains("ctx 10%"), "{text}");
    assert!(text.contains("5h 5%"), "{text}");
    assert!(text.contains("7d 8%"), "{text}");

    let _ = fs::remove_dir_all(home);
}

#[test]
fn statusline_without_hook_is_estimated() {
    let home = unique_temp_dir("statusline-no-hook");
    write_today_claude(&home);
    let (ok, stdout, stderr) = run_ccstats(
        &[
            "statusline",
            "--source",
            "claude",
            "--json",
            "--offline",
            "--timezone",
            "UTC",
        ],
        &[("HOME", &home)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    let value: Value = serde_json::from_slice(&stdout).unwrap();
    assert_eq!(value["confidence"], "estimated");

    let _ = fs::remove_dir_all(home);
}
