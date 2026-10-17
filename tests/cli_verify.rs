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

#[test]
fn verify_classifies_only_applicable_request_price_tiers() {
    let home = tempfile::tempdir().unwrap();
    let usage = home.path().join("cursor-usage.json");
    for (model, input, read, reason) in [
        ("claude-haiku-5-5", 100_000, 0, "priced"),
        ("claude-haiku-5-5", 100_001, 0, "tiered_pricing"),
        ("claude-haiku-5-5", 1, 100_000, "tiered_pricing"),
        ("claude-haiku-5-5", 0, 100_001, "tiered_pricing"),
        ("claude-sonnet-5-5", 100_001, 0, "priced"),
        ("gpt-6.1-sol", 272_000, 0, "priced"),
        ("gpt-6.1-sol", 272_001, 0, "tiered_pricing"),
    ] {
        let data = serde_json::json!({"usageEventsDisplay":[{
            "timestamp":"2026-10-17T12:00:00Z", "model":model,
            "tokenUsage":{"inputTokens":input,"cacheReadTokens":read},
            "chargedCents":1.0
        }]});
        write_file(&usage, &data.to_string());
        let (ok, stdout, stderr) = run_ccstats(
            &[
                "verify",
                "--source",
                "cursor",
                "--json",
                "--offline",
                "--no-cache",
            ],
            &[("HOME", home.path()), ("CURSOR_USAGE_FILE", &usage)],
        );
        assert!(ok, "{}", String::from_utf8_lossy(&stderr));
        let value: Value = serde_json::from_slice(&stdout).unwrap();
        assert_eq!(
            value["sources"][0]["reasons"][reason], 1,
            "{model}: {value}"
        );
    }
}
