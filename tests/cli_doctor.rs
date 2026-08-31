mod common;

use std::{fs, path::Path};

use common::{run_ccstats, unique_temp_dir, write_file};
use serde_json::Value;

#[test]
fn doctor_reports_detected_and_configured_sources_without_exposing_credentials() {
    let root = unique_temp_dir("doctor-diagnostics");
    let claude_root = root.join("claude-home");
    write_file(&claude_root.join("projects/example/session.jsonl"), "{}\n");
    let cursor_key = Path::new("cursor-secret-must-not-appear");

    let (ok, stdout, stderr) = run_ccstats(
        &["doctor", "--json"],
        &[
            ("HOME", &root),
            ("CLAUDE_CONFIG_DIR", &claude_root),
            ("CURSOR_API_KEY", cursor_key),
        ],
    );

    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    assert!(
        stderr.is_empty(),
        "stderr: {}",
        String::from_utf8_lossy(&stderr)
    );
    let output = String::from_utf8(stdout).expect("utf8 output");
    assert!(!output.contains("cursor-secret-must-not-appear"));

    let rows: Value = serde_json::from_str(&output).expect("doctor json");
    let rows = rows.as_array().expect("array output");
    assert_eq!(rows.len(), 5);

    let claude = rows
        .iter()
        .find(|row| row["name"] == "claude")
        .expect("Claude diagnostic");
    assert_eq!(claude["status"], "detected");
    assert_eq!(claude["files"], 1);

    let cursor = rows
        .iter()
        .find(|row| row["name"] == "cursor")
        .expect("Cursor diagnostic");
    assert_eq!(cursor["status"], "configured");
    assert_eq!(cursor["files"], 0);
    assert!(
        cursor["detail"]
            .as_str()
            .is_some_and(|detail| detail.contains("not contacted"))
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn doctor_table_gives_a_next_step_when_no_sources_are_detected() {
    let root = unique_temp_dir("doctor-empty");
    let (ok, stdout, stderr) = run_ccstats(&["doctor"], &[("HOME", &root)]);

    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    assert!(
        stderr.is_empty(),
        "stderr: {}",
        String::from_utf8_lossy(&stderr)
    );
    let output = String::from_utf8(stdout).expect("utf8 output");
    assert!(output.contains("No source data detected"));
    assert!(output.contains("ccstats doctor --json"));

    let _ = fs::remove_dir_all(root);
}
