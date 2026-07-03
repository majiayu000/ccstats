mod common;

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
        r#"{
  "created_at": "2026-02-06T09:00:00Z",
  "updated_at": "2026-02-06T10:00:00Z",
  "current_model_id": "grok-build",
  "git_root_dir": "/tmp/grok-project/"
}"#,
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
        &[("GROK_HOME", &grok_home)],
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
