mod common;

use common::{run_ccstats, unique_temp_dir, write_file};
use serde_json::Value;
use std::fs;

#[test]
fn verify_cursor_compares_recorded_cost() {
    let home = unique_temp_dir("verify-cursor");
    let usage = home.join("cursor-usage.json");
    write_file(
        &usage,
        r#"{
  "usageEventsDisplay": [
    {
      "timestamp": "2026-02-01T12:00:00.000Z",
      "model": "gpt-4.1",
      "conversationId": "composer-verify",
      "tokenUsage": {"inputTokens": 100, "outputTokens": 20},
      "chargedCents": 12.5
    }
  ]
}"#,
    );
    let (ok, stdout, stderr) = run_ccstats(
        &[
            "verify",
            "--source",
            "cursor",
            "--json",
            "--offline",
            "--timezone",
            "UTC",
        ],
        &[("HOME", &home), ("CURSOR_USAGE_FILE", &usage)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    let value: Value = serde_json::from_slice(&stdout).unwrap();
    let sources = value["sources"].as_array().unwrap();
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0]["source"], "cursor");
    assert_eq!(sources[0]["records"], 1);
    assert!((sources[0]["recorded_usd"].as_f64().unwrap() - 0.125).abs() < 1e-9);

    let _ = fs::remove_dir_all(home);
}

#[test]
fn verify_without_recorded_costs_prints_hint() {
    let home = unique_temp_dir("verify-empty");
    let (ok, stdout, stderr) = run_ccstats(
        &["verify", "--source", "claude", "--offline", "--no-color"],
        &[("HOME", &home)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    let text = String::from_utf8_lossy(&stdout);
    assert!(text.contains("No source-recorded costs"), "{text}");

    let _ = fs::remove_dir_all(home);
}
