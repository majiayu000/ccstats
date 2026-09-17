mod common;

use chrono::Utc;
use common::{run_ccstats, unique_temp_dir, write_file};
use serde_json::Value;
use std::fs;

fn write_today_claude(root: &std::path::Path) {
    let today = Utc::now().format("%Y-%m-%dT12:00:00Z");
    write_file(
        &root.join(".claude/projects/watch-app/session.jsonl"),
        &format!(
            r#"{{"timestamp":"{today}","message":{{"id":"msg_watch","model":"claude-3-5-sonnet-20241022","stop_reason":"end_turn","usage":{{"input_tokens":100,"output_tokens":50}}}}}}
"#
        ),
    );
}

#[test]
fn watch_once_json_includes_today_and_windows() {
    let home = unique_temp_dir("watch-once-json");
    write_today_claude(&home);
    let (ok, stdout, stderr) = run_ccstats(
        &[
            "watch",
            "--once",
            "--json",
            "--offline",
            "--source",
            "claude",
            "--timezone",
            "UTC",
        ],
        &[("HOME", &home)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    let value: Value = serde_json::from_slice(&stdout).unwrap();
    assert_eq!(value["today"]["label"], "today");
    assert!(value["windows"].is_array());
    assert_eq!(value["hot"], false);

    let _ = fs::remove_dir_all(home);
}

#[test]
fn watch_once_text_mentions_today() {
    let home = unique_temp_dir("watch-once-text");
    write_today_claude(&home);
    let (ok, stdout, stderr) = run_ccstats(
        &[
            "watch",
            "--once",
            "--offline",
            "--no-color",
            "--source",
            "claude",
            "--timezone",
            "UTC",
        ],
        &[("HOME", &home)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    let text = String::from_utf8_lossy(&stdout);
    assert!(text.contains("ccstats watch"), "{text}");
    assert!(text.contains("today"), "{text}");

    let _ = fs::remove_dir_all(home);
}
