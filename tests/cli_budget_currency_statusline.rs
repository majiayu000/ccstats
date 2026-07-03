mod common;

use chrono::Utc;
use common::{run_ccstats, unique_temp_dir, write_file};
use serde_json::Value;
use std::fs;
use std::path::Path;

fn write_exchange_rates_cache(home: &Path, rates: &str) {
    write_file(&home.join(".cache/ccstats/exchange_rates.json"), rates);
}

fn write_pricing_cache(home: &Path, xdg_cache: &Path, contents: &str) {
    write_file(&xdg_cache.join("ccstats/pricing.json"), contents);
    write_file(&home.join("Library/Caches/ccstats/pricing.json"), contents);
    write_file(&home.join(".cache/ccstats/pricing.json"), contents);
}

#[test]
fn monthly_budget_json_includes_forecast() {
    let root = unique_temp_dir("monthly-budget-json");
    let session_file = root.join(".claude/projects/myapp/session.jsonl");
    write_file(
        &session_file,
        r#"{"timestamp":"2025-02-10T10:00:00Z","message":{"id":"msg_1","model":"claude-3-5-sonnet-20241022","stop_reason":"end_turn","usage":{"input_tokens":1000000,"output_tokens":100000,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}
"#,
    );

    let (ok, stdout, stderr) = run_ccstats(
        &[
            "monthly",
            "-j",
            "-O",
            "--timezone",
            "UTC",
            "--since",
            "2025-02-01",
            "--until",
            "2025-02-10",
            "--monthly-budget",
            "10",
        ],
        &[("HOME", &root)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));

    let json: Value = serde_json::from_slice(&stdout).expect("json");
    let arr = json.as_array().expect("array output");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["month"].as_str(), Some("2025-02"));

    let budget = &arr[0]["budget"];
    assert_eq!(budget["days_elapsed"].as_i64(), Some(10));
    assert_eq!(budget["days_in_month"].as_i64(), Some(28));
    assert_eq!(budget["status"].as_str(), Some("over_budget"));
    assert!((budget["spent"].as_f64().unwrap() - 4.5).abs() < 0.001);
    assert!((budget["projected"].as_f64().unwrap() - 12.6).abs() < 0.001);
    assert!((budget["projected_pct"].as_f64().unwrap() - 126.0).abs() < 0.001);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn monthly_budget_csv_includes_budget_columns() {
    let root = unique_temp_dir("monthly-budget-csv");
    let session_file = root.join(".claude/projects/myapp/session.jsonl");
    write_file(
        &session_file,
        r#"{"timestamp":"2025-02-10T10:00:00Z","message":{"id":"msg_1","model":"claude-3-5-sonnet-20241022","stop_reason":"end_turn","usage":{"input_tokens":1000000,"output_tokens":100000,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}
"#,
    );

    let (ok, stdout, stderr) = run_ccstats(
        &[
            "monthly",
            "--csv",
            "-O",
            "--timezone",
            "UTC",
            "--since",
            "2025-02-01",
            "--until",
            "2025-02-10",
            "--monthly-budget",
            "10",
        ],
        &[("HOME", &root)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));

    let output = String::from_utf8_lossy(&stdout);
    let mut lines = output.lines();
    let header = lines.next().expect("header");
    let row = lines.next().expect("row");
    assert!(header.contains("budget_projected"));
    assert!(header.contains("budget_status"));
    assert!(row.contains("over_budget"));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn monthly_budget_rejects_hidden_costs() {
    let root = unique_temp_dir("monthly-budget-no-cost");
    let (ok, _stdout, stderr) = run_ccstats(
        &["monthly", "-O", "--monthly-budget", "10", "--no-cost"],
        &[("HOME", &root)],
    );
    assert!(!ok, "expected hidden-cost failure");
    let stderr = String::from_utf8_lossy(&stderr);
    assert!(stderr.contains("--monthly-budget requires cost display"));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn strict_pricing_sets_unknown_cost_to_null() {
    let root = unique_temp_dir("strict-pricing");
    let codex_home = root.join("codex-home");
    let session_file = codex_home.join("sessions").join("strict-session.jsonl");
    write_file(
        &session_file,
        r#"{"timestamp":"2026-02-06T11:00:00Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":50,"cached_input_tokens":0,"output_tokens":10,"reasoning_output_tokens":0,"total_tokens":60},"last_token_usage":{"input_tokens":50,"cached_input_tokens":0,"output_tokens":10,"reasoning_output_tokens":0,"total_tokens":60},"model":"mystery-model"}}}
"#,
    );

    let (ok, stdout, stderr) = run_ccstats(
        &[
            "codex",
            "daily",
            "-j",
            "-O",
            "--strict-pricing",
            "--timezone",
            "UTC",
            "--since",
            "2026-02-06",
            "--until",
            "2026-02-06",
        ],
        &[("CODEX_HOME", &codex_home)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));

    let json: Value = serde_json::from_slice(&stdout).expect("json");
    let arr = json.as_array().expect("array output");
    assert_eq!(arr.len(), 1);
    assert!(arr[0]["cost"].is_null());

    let _ = fs::remove_dir_all(root);
}

#[test]
fn invalid_currency_rejects_missing_rate() {
    let root = unique_temp_dir("invalid-currency-rejected");
    let claude_file = root.join(".claude/projects/myproject/session-a.jsonl");
    write_file(
        &claude_file,
        r#"{"timestamp":"2026-02-06T12:00:00Z","message":{"id":"msg_1","model":"claude-3-5-sonnet-20241022","stop_reason":"end_turn","usage":{"input_tokens":100,"output_tokens":50}}}
"#,
    );

    let (ok, stdout, stderr) = run_ccstats(
        &[
            "daily",
            "-j",
            "-O",
            "--timezone",
            "UTC",
            "--currency",
            "ZZZ",
            "--since",
            "2026-02-06",
            "--until",
            "2026-02-06",
        ],
        &[("HOME", &root)],
    );
    assert!(!ok, "stdout: {}", String::from_utf8_lossy(&stdout));

    let stderr = String::from_utf8(stderr).expect("utf8 stderr");
    assert!(stderr.contains("Error: failed to load exchange rate for 'ZZZ'"));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn daily_csv_uses_requested_currency() {
    let root = unique_temp_dir("daily-csv-currency");
    write_exchange_rates_cache(&root, r#"{"CNY":7.0}"#);
    let claude_file = root.join(".claude/projects/myproject/session-a.jsonl");
    write_file(
        &claude_file,
        r#"{"timestamp":"2026-02-06T12:00:00Z","message":{"id":"msg_1","model":"claude-3-5-sonnet-20241022","stop_reason":"end_turn","usage":{"input_tokens":100,"output_tokens":50}}}
"#,
    );

    let (ok, stdout, stderr) = run_ccstats(
        &[
            "daily",
            "--csv",
            "-O",
            "--timezone",
            "UTC",
            "--currency",
            "CNY",
            "--since",
            "2026-02-06",
            "--until",
            "2026-02-06",
        ],
        &[("HOME", &root)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));

    let output = String::from_utf8(stdout).expect("utf8 stdout");
    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(
        lines[0],
        "date,input_tokens,output_tokens,reasoning_tokens,cache_creation_tokens,cache_read_tokens,total_tokens,cost,pricing_source"
    );
    assert!(
        lines[1].ends_with(",0.007350,fallback"),
        "row: {}",
        lines[1]
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn daily_outputs_fresh_cache_pricing_source() {
    let root = unique_temp_dir("daily-cache-pricing-source");
    let xdg_cache = root.join("xdg-cache");
    write_pricing_cache(
        &root,
        &xdg_cache,
        r#"{"claude-3-5-sonnet-20241022":{"input_cost_per_token":0.000001,"output_cost_per_token":0.000002}}"#,
    );
    let claude_file = root.join(".claude/projects/myproject/session-a.jsonl");
    write_file(
        &claude_file,
        r#"{"timestamp":"2026-02-06T12:00:00Z","message":{"id":"msg_1","model":"claude-3-5-sonnet-20241022","stop_reason":"end_turn","usage":{"input_tokens":100,"output_tokens":50}}}
"#,
    );

    let envs = [
        ("HOME", root.as_path()),
        ("XDG_CACHE_HOME", xdg_cache.as_path()),
    ];
    let (ok, stdout, stderr) = run_ccstats(
        &[
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
        &envs,
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    let json: Value = serde_json::from_slice(&stdout).expect("json");
    let row = &json.as_array().expect("array output")[0];
    assert_eq!(row["pricing_source"].as_str(), Some("cache"));
    assert!(row["pricing_cache_age_seconds"].as_u64().is_some());
    assert!(row["pricing_cache_mtime_epoch_seconds"].as_u64().is_some());

    let (ok, stdout, stderr) = run_ccstats(
        &[
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
        &envs,
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    let output = String::from_utf8(stdout).expect("utf8 stdout");
    let lines: Vec<&str> = output.lines().collect();
    assert!(lines[0].ends_with(
        ",cost,pricing_source,pricing_cache_age_seconds,pricing_cache_mtime_epoch_seconds"
    ));
    let fields: Vec<&str> = lines[1].split(',').collect();
    assert_eq!(fields[8], "cache");
    assert!(fields[9].parse::<u64>().is_ok());
    assert!(fields[10].parse::<u64>().is_ok());

    let _ = fs::remove_dir_all(root);
}

#[test]
fn daily_csv_fallback_source_omits_empty_cache_columns() {
    let root = unique_temp_dir("daily-fallback-pricing-source");
    let xdg_cache = root.join("xdg-cache");
    write_pricing_cache(
        &root,
        &xdg_cache,
        r#"{"unrelated-model":{"input_cost_per_token":0.000001,"output_cost_per_token":0.000002}}"#,
    );
    let claude_file = root.join(".claude/projects/myproject/session-a.jsonl");
    write_file(
        &claude_file,
        r#"{"timestamp":"2026-02-06T12:00:00Z","message":{"id":"msg_1","model":"claude-3-5-sonnet-20241022","stop_reason":"end_turn","usage":{"input_tokens":100,"output_tokens":50}}}
"#,
    );

    let (ok, stdout, stderr) = run_ccstats(
        &[
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
        &[("HOME", &root), ("XDG_CACHE_HOME", &xdg_cache)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));

    let output = String::from_utf8(stdout).expect("utf8 stdout");
    let lines: Vec<&str> = output.lines().collect();
    assert!(lines[0].ends_with(",cost,pricing_source"));
    assert!(lines[1].ends_with(",fallback"));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn statusline_json_uses_requested_currency() {
    let root = unique_temp_dir("statusline-currency");
    write_exchange_rates_cache(&root, r#"{"CNY":7.0}"#);
    let today = Utc::now().format("%Y-%m-%dT12:00:00Z").to_string();
    let claude_file = root.join(".claude/projects/myproject/session-a.jsonl");
    write_file(
        &claude_file,
        &format!(
            r#"{{"timestamp":"{today}","message":{{"id":"msg_1","model":"claude-3-5-sonnet-20241022","stop_reason":"end_turn","usage":{{"input_tokens":100,"output_tokens":50}}}}}}
"#
        ),
    );

    let (ok, stdout, stderr) = run_ccstats(
        &[
            "statusline",
            "-j",
            "-O",
            "--timezone",
            "UTC",
            "--currency",
            "CNY",
        ],
        &[("HOME", &root)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));

    let json: Value = serde_json::from_slice(&stdout).expect("json");
    assert_eq!(json["source"].as_str(), Some("Claude Code"));
    let cost = json["cost"].as_f64().expect("numeric cost");
    assert!((cost - 0.00735).abs() < f64::EPSILON);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn statusline_json_includes_data_quality_metadata() {
    let root = unique_temp_dir("statusline-quality");
    let codex_home = root.join("codex-home");
    let session_file = codex_home.join("sessions").join("statusline.jsonl");
    let today = Utc::now().format("%Y-%m-%dT12:00:00Z").to_string();
    write_file(
        &session_file,
        &format!(
            r#"{{"timestamp":"{today}","type":"event_msg","payload":{{"type":"token_count","info":{{"total_token_usage":{{"input_tokens":10,"cached_input_tokens":0,"output_tokens":5,"reasoning_output_tokens":0,"total_tokens":15}},"last_token_usage":{{"input_tokens":10,"cached_input_tokens":0,"output_tokens":5,"reasoning_output_tokens":0,"total_tokens":15}},"model":"gpt-5"}}}}}}
{{"timestamp":"not-json"
"#
        ),
    );

    let (ok, stdout, stderr) = run_ccstats(
        &[
            "statusline",
            "--source",
            "codex",
            "-j",
            "-O",
            "--timezone",
            "UTC",
        ],
        &[("CODEX_HOME", &codex_home)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));

    let json: Value = serde_json::from_slice(&stdout).expect("json");
    assert_eq!(json["source"].as_str(), Some("OpenAI Codex"));
    assert_eq!(json["data_quality"]["valid_entries"].as_i64(), Some(1));
    assert_eq!(
        json["data_quality"]["dedup_skipped_entries"].as_i64(),
        Some(0)
    );
    assert_eq!(json["data_quality"]["parse_errors"].as_u64(), Some(1));

    let _ = fs::remove_dir_all(root);
}
