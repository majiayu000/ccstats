mod common;

use chrono::Utc;
use common::{run_ccstats, unique_temp_dir, write_file};
use rusqlite::Connection;
use serde_json::Value;
use std::fs;
use std::path::Path;

fn write_cursor_state_db(path: &Path) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create cursor db parent");
    }
    let conn = Connection::open(path).expect("open cursor db");
    conn.execute(
        "CREATE TABLE cursorDiskKV (key TEXT PRIMARY KEY, value BLOB)",
        [],
    )
    .expect("create cursorDiskKV");
    conn.execute(
        "INSERT INTO cursorDiskKV (key, value) VALUES (?1, ?2)",
        (
            "composerData:composer-1",
            r#"{"composerId":"composer-1","modelConfig":{"modelName":"claude-4-sonnet"},"workspaceIdentifier":{"uri":{"fsPath":"/tmp/cursor-project"}}}"#,
        ),
    )
    .expect("insert composer");
    conn.execute(
        "INSERT INTO cursorDiskKV (key, value) VALUES (?1, ?2)",
        (
            "bubbleId:composer-1:bubble-1",
            r#"{"createdAt":"2026-02-06T10:00:00Z","tokenCount":{"inputTokens":100,"outputTokens":40}}"#,
        ),
    )
    .expect("insert bubble");
}

fn write_grok_session(grok_home: &Path) {
    write_grok_session_at(grok_home, "2026-02-06T09:00:00Z", "2026-02-06T10:00:00Z");
}

fn write_grok_session_at(grok_home: &Path, created_at: &str, updated_at: &str) {
    let session_dir = grok_home
        .join("sessions")
        .join("%2Ftmp%2Fgrok-project")
        .join("grok-session-1");
    write_file(
        &session_dir.join("signals.json"),
        r#"{
  "contextTokensUsed": 1200,
  "totalTokensBeforeCompaction": 300,
  "primaryModelId": "grok-build",
  "modelsUsed": ["grok-build"]
}"#,
    );
    write_file(
        &session_dir.join("summary.json"),
        &format!(
            r#"{{
  "created_at": "{created_at}",
  "updated_at": "{updated_at}",
  "current_model_id": "grok-build",
  "git_root_dir": "/tmp/grok-project/"
}}"#
        ),
    );
}

fn write_grok_update_only_session(grok_home: &Path) {
    let session_dir = grok_home
        .join("sessions")
        .join("%2Ftmp%2Fgrok-project")
        .join("grok-update-only");
    write_file(
        &session_dir.join("summary.json"),
        r#"{
  "created_at": "2026-02-06T09:00:00Z",
  "updated_at": "2026-02-06T10:30:00Z",
  "current_model_id": "grok-build",
  "git_root_dir": "/tmp/grok-project/"
}"#,
    );
    write_file(
        &session_dir.join("updates.jsonl"),
        r#"{"timestamp":1779096277,"params":{"sessionId":"grok-update-only","_meta":{"updateType":"AvailableCommandsUpdate","totalTokens":100}}}
{"timestamp":1779096277,"params":{"sessionId":"grok-update-only","_meta":{"updateType":"AvailableCommandsUpdate","totalTokens":250}}}
"#,
    );
}

fn write_claude_session(root: &Path) {
    write_claude_session_at(root, "2026-02-06T10:00:00Z");
}

fn write_claude_session_at(root: &Path, timestamp: &str) {
    write_file(
        &root.join(".claude/projects/mixed/session.jsonl"),
        &format!(
            r#"{{"timestamp":"{timestamp}","message":{{"id":"msg_real","model":"claude-3-5-sonnet-20241022","stop_reason":"end_turn","usage":{{"input_tokens":1000000,"output_tokens":100000,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}}}}
"#
        ),
    );
}

fn assert_close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() < 0.000_001,
        "expected {expected}, got {actual}"
    );
}

#[test]
fn source_flag_can_select_cursor_without_subcommand() {
    let root = unique_temp_dir("source-flag-cursor");
    let cursor_home = root.join("cursor-user");
    write_cursor_state_db(&cursor_home.join("globalStorage").join("state.vscdb"));

    let (ok, stdout, stderr) = run_ccstats(
        &[
            "daily",
            "--source",
            "cursor",
            "-j",
            "-O",
            "--no-cost",
            "--timezone",
            "UTC",
            "--since",
            "2026-02-06",
            "--until",
            "2026-02-06",
        ],
        &[("CURSOR_HOME", &cursor_home)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));

    let json: Value = serde_json::from_slice(&stdout).expect("json");
    let arr = json.as_array().expect("array output");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["date"].as_str(), Some("2026-02-06"));
    assert_eq!(arr[0]["input_tokens"].as_i64(), Some(100));
    assert_eq!(arr[0]["output_tokens"].as_i64(), Some(40));
    assert_eq!(arr[0]["total_tokens"].as_i64(), Some(140));
    assert_eq!(
        arr[0]["models"].as_array().unwrap()[0].as_str(),
        Some("claude-4-sonnet")
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn source_flag_can_select_grok_without_subcommand() {
    let root = unique_temp_dir("source-flag-grok");
    let grok_home = root.join("grok-home");
    write_grok_session(&grok_home);

    let (ok, stdout, stderr) = run_ccstats(
        &[
            "daily",
            "--source",
            "grok",
            "-j",
            "-O",
            "--no-cost",
            "--timezone",
            "UTC",
            "--since",
            "2026-02-06",
            "--until",
            "2026-02-06",
        ],
        &[("GROK_HOME", &grok_home), ("HOME", &root)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));

    let json: Value = serde_json::from_slice(&stdout).expect("json");
    let arr = json.as_array().expect("array output");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["date"].as_str(), Some("2026-02-06"));
    assert_eq!(arr[0]["input_tokens"].as_i64(), Some(1500));
    assert_eq!(arr[0]["output_tokens"].as_i64(), Some(0));
    assert_eq!(arr[0]["total_tokens"].as_i64(), Some(1500));
    assert_eq!(
        arr[0]["models"].as_array().unwrap()[0].as_str(),
        Some("grok-build")
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn grok_daily_json_marks_estimated_proxy_cost() {
    let root = unique_temp_dir("grok-estimated-json");
    let grok_home = root.join("grok-home");
    write_grok_session(&grok_home);

    let (ok, stdout, stderr) = run_ccstats(
        &[
            "grok",
            "daily",
            "-j",
            "-O",
            "--timezone",
            "UTC",
            "--since",
            "2026-02-06",
            "--until",
            "2026-02-06",
        ],
        &[("GROK_HOME", &grok_home), ("HOME", &root)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));

    let json: Value = serde_json::from_slice(&stdout).expect("json");
    let arr = json.as_array().expect("array output");
    assert_eq!(arr[0]["cost_kind"].as_str(), Some("estimated_proxy"));
    assert_close(arr[0]["cost"].as_f64().unwrap(), 0.0015);
    assert_close(arr[0]["estimated_cost"].as_f64().unwrap(), 0.0015);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn grok_daily_csv_marks_estimated_proxy_cost() {
    let root = unique_temp_dir("grok-estimated-csv");
    let grok_home = root.join("grok-home");
    write_grok_session(&grok_home);

    let (ok, stdout, stderr) = run_ccstats(
        &[
            "grok",
            "daily",
            "--csv",
            "-O",
            "--timezone",
            "UTC",
            "--since",
            "2026-02-06",
            "--until",
            "2026-02-06",
        ],
        &[("GROK_HOME", &grok_home), ("HOME", &root)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));

    let output = String::from_utf8(stdout).expect("utf8 stdout");
    let lines: Vec<&str> = output.lines().collect();
    assert!(lines[0].ends_with(",cost,cost_kind,estimated_cost,pricing_source"));
    assert!(
        lines[1].ends_with(",0.001500,estimated_proxy,0.001500,fallback"),
        "stdout: {output}"
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn all_sources_json_separates_real_and_grok_estimated_cost() {
    let root = unique_temp_dir("all-sources-grok-estimated");
    let grok_home = root.join("grok-home");
    write_claude_session(&root);
    write_grok_session(&grok_home);

    let (ok, stdout, stderr) = run_ccstats(
        &[
            "daily",
            "--source",
            "all",
            "-j",
            "-O",
            "--timezone",
            "UTC",
            "--since",
            "2026-02-06",
            "--until",
            "2026-02-06",
        ],
        &[("HOME", &root), ("GROK_HOME", &grok_home)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));

    let json: Value = serde_json::from_slice(&stdout).expect("json");
    let arr = json.as_array().expect("array output");
    assert_eq!(arr[0]["cost_kind"].as_str(), Some("mixed"));
    assert_close(arr[0]["cost"].as_f64().unwrap(), 4.5);
    assert_close(arr[0]["estimated_cost"].as_f64().unwrap(), 0.0015);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn all_sources_statusline_json_excludes_grok_estimated_cost() {
    let root = unique_temp_dir("all-statusline-grok-estimated");
    let grok_home = root.join("grok-home");
    let today = Utc::now().format("%Y-%m-%dT12:00:00Z").to_string();
    write_claude_session_at(&root, &today);
    write_grok_session_at(&grok_home, &today, &today);

    let (ok, stdout, stderr) = run_ccstats(
        &[
            "statusline",
            "--source",
            "all",
            "-j",
            "-O",
            "--timezone",
            "UTC",
        ],
        &[("HOME", &root), ("GROK_HOME", &grok_home)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));

    let json: Value = serde_json::from_slice(&stdout).expect("json");
    assert_eq!(json["cost_kind"].as_str(), Some("mixed"));
    assert_close(json["cost"].as_f64().unwrap(), 4.5);
    assert_close(json["estimated_cost"].as_f64().unwrap(), 0.0015);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn all_sources_top_json_exposes_grok_estimate_without_real_cost() {
    let root = unique_temp_dir("all-top-grok-estimated");
    let grok_home = root.join("grok-home");
    write_claude_session(&root);
    write_grok_session(&grok_home);

    let (ok, stdout, stderr) = run_ccstats(
        &[
            "top",
            "--source",
            "all",
            "--dim",
            "model",
            "-j",
            "-O",
            "--timezone",
            "UTC",
            "--since",
            "2026-02-06",
            "--until",
            "2026-02-06",
        ],
        &[("HOME", &root), ("GROK_HOME", &grok_home)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));

    let json: Value = serde_json::from_slice(&stdout).expect("json");
    let entries = json["entries"].as_array().expect("entries");
    let grok = entries
        .iter()
        .find(|entry| entry["name"].as_str() == Some("grok-build"))
        .expect("grok row");
    assert_eq!(grok["cost_kind"].as_str(), Some("estimated_proxy"));
    assert_close(grok["cost_usd"].as_f64().unwrap(), 0.0);
    assert_close(grok["estimated_cost_usd"].as_f64().unwrap(), 0.0015);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn all_sources_monthly_budget_uses_real_cost_only() {
    let root = unique_temp_dir("all-budget-grok-estimated");
    let grok_home = root.join("grok-home");
    write_claude_session(&root);
    write_grok_session(&grok_home);

    let (ok, stdout, stderr) = run_ccstats(
        &[
            "monthly",
            "--source",
            "all",
            "-j",
            "-O",
            "--timezone",
            "UTC",
            "--since",
            "2026-02-01",
            "--until",
            "2026-02-28",
            "--monthly-budget",
            "10",
        ],
        &[("HOME", &root), ("GROK_HOME", &grok_home)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));

    let json: Value = serde_json::from_slice(&stdout).expect("json");
    let arr = json.as_array().expect("array output");
    assert_close(arr[0]["cost"].as_f64().unwrap(), 4.5);
    assert_close(arr[0]["estimated_cost"].as_f64().unwrap(), 0.0015);
    assert_close(arr[0]["budget"]["spent"].as_f64().unwrap(), 4.5);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn grok_subcommand_defaults_to_daily() {
    let root = unique_temp_dir("grok-subcommand");
    let grok_home = root.join("grok-home");
    write_grok_session(&grok_home);

    let (ok, stdout, stderr) = run_ccstats(
        &[
            "grok",
            "-j",
            "-O",
            "--no-cost",
            "--timezone",
            "UTC",
            "--since",
            "2026-02-06",
            "--until",
            "2026-02-06",
        ],
        &[("GROK_HOME", &grok_home)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));

    let json: Value = serde_json::from_slice(&stdout).expect("json");
    let arr = json.as_array().expect("array output");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["date"].as_str(), Some("2026-02-06"));
    assert_eq!(arr[0]["total_tokens"].as_i64(), Some(1500));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn grok_subcommand_supports_project_view() {
    let root = unique_temp_dir("grok-subcommand-project");
    let grok_home = root.join("grok-home");
    write_grok_session(&grok_home);

    let (ok, stdout, stderr) = run_ccstats(
        &[
            "grok",
            "project",
            "-j",
            "-O",
            "--no-cost",
            "--timezone",
            "UTC",
            "--since",
            "2026-02-06",
            "--until",
            "2026-02-06",
        ],
        &[("GROK_HOME", &grok_home)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));

    let json: Value = serde_json::from_slice(&stdout).expect("json");
    let arr = json.as_array().expect("array output");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["project_path"].as_str(), Some("/tmp/grok-project/"));
    assert_eq!(arr[0]["total_tokens"].as_i64(), Some(1500));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn grok_source_falls_back_to_updates_when_signals_missing() {
    let root = unique_temp_dir("grok-updates-fallback");
    let grok_home = root.join("grok-home");
    write_grok_update_only_session(&grok_home);

    let (ok, stdout, stderr) = run_ccstats(
        &[
            "grok",
            "daily",
            "-j",
            "-O",
            "--no-cost",
            "--timezone",
            "UTC",
            "--since",
            "2026-02-06",
            "--until",
            "2026-02-06",
        ],
        &[("GROK_HOME", &grok_home)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));

    let json: Value = serde_json::from_slice(&stdout).expect("json");
    let arr = json.as_array().expect("array output");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["input_tokens"].as_i64(), Some(250));
    assert_eq!(
        arr[0]["models"].as_array().unwrap()[0].as_str(),
        Some("grok-build")
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn grok_project_json_uses_summary_git_root() {
    let root = unique_temp_dir("grok-project");
    let grok_home = root.join("grok-home");
    write_grok_session(&grok_home);

    let (ok, stdout, stderr) = run_ccstats(
        &[
            "project",
            "--source",
            "grok",
            "-j",
            "-O",
            "--no-cost",
            "--timezone",
            "UTC",
            "--since",
            "2026-02-06",
            "--until",
            "2026-02-06",
        ],
        &[("GROK_HOME", &grok_home)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));

    let json: Value = serde_json::from_slice(&stdout).expect("json");
    let arr = json.as_array().expect("array output");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["project_path"].as_str(), Some("/tmp/grok-project/"));
    assert_eq!(arr[0]["total_tokens"].as_i64(), Some(1500));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn grok_subcommand_conflicts_with_different_source_flag() {
    let root = unique_temp_dir("grok-source-flag-conflict");
    let (ok, _stdout, stderr) = run_ccstats(
        &["grok", "daily", "--source", "claude", "-O", "--no-cost"],
        &[("HOME", &root)],
    );
    assert!(!ok, "expected conflict failure");
    let stderr = String::from_utf8_lossy(&stderr);
    assert!(stderr.contains("conflicts with --source"));

    let _ = fs::remove_dir_all(root);
}
