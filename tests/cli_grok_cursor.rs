mod common;

use chrono::Utc;
use common::{run_ccstats, unique_temp_dir, write_file};
use serde_json::Value;
use std::fs;
use std::path::Path;

fn write_cursor_usage_file(path: &Path) {
    write_file(
        path,
        r#"{
  "usageEventsDisplay": [
    {
      "timestamp": "2026-02-06T10:00:00Z",
      "model": "claude-4-sonnet",
      "conversationId": "composer-1",
      "tokenUsage": {
        "inputTokens": 100,
        "outputTokens": 40,
        "cacheWriteTokens": 0,
        "cacheReadTokens": 10
      },
      "chargedCents": 12.5
    }
  ]
}"#,
    );
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

fn write_grok_turn_session(grok_home: &Path) {
    let session_dir = grok_home
        .join("sessions")
        .join("%2Ftmp%2Fgrok-project")
        .join("grok-turn-session");
    write_file(
        &session_dir.join("signals.json"),
        r#"{
  "contextTokensUsed": 999999,
  "primaryModelId": "grok-build"
}"#,
    );
    write_file(
        &session_dir.join("summary.json"),
        r#"{
  "created_at": "2026-08-15T09:00:00Z",
  "updated_at": "2026-08-17T10:00:00Z",
  "current_model_id": "grok-4.5-build",
  "git_root_dir": "/tmp/grok-project/"
}"#,
    );
    write_file(
        &session_dir.join("updates.jsonl"),
        r#"{"timestamp":1786838400,"method":"_x.ai/session/update","params":{"sessionId":"grok-turn-session","update":{"sessionUpdate":"turn_completed","prompt_id":"p1","stop_reason":"end_turn","usage":{"inputTokens":100,"outputTokens":20,"cachedReadTokens":40,"reasoningTokens":5,"modelCalls":3,"costUsdTicks":20000000000,"modelUsage":{"grok-4.5-build":{"inputTokens":100,"outputTokens":20,"cachedReadTokens":40,"reasoningTokens":5,"modelCalls":3,"costUsdTicks":20000000000}}}},"_meta":{"eventId":"e1","agentTimestampMs":1786838400000}}}
{"timestamp":1786924800,"method":"_x.ai/session/update","params":{"sessionId":"grok-turn-session","update":{"sessionUpdate":"turn_completed","prompt_id":"p2","usage":{"inputTokens":50,"outputTokens":10,"cachedReadTokens":10,"reasoningTokens":2,"modelCalls":2,"costUsdTicks":5000000000,"modelUsage":{"grok-4.5-build":{"inputTokens":50,"outputTokens":10,"cachedReadTokens":10,"reasoningTokens":2,"modelCalls":2,"costUsdTicks":5000000000}}}},"_meta":{"agentTimestampMs":1786924800000}}}
"#,
    );
}

fn write_partial_grok_inference_log(grok_home: &Path) {
    write_file(
        &grok_home.join("logs/unified.jsonl"),
        r#"{"ts":"2026-08-16T00:00:00Z","sid":"grok-turn-session","msg":"shell.turn.inference_done","ctx":{"loop_index":1,"prompt_tokens":100,"cached_prompt_tokens":40,"completion_tokens":20,"reasoning_tokens":5}}"#,
    );
}

fn write_grok_snapshot_with_inference(grok_home: &Path) {
    let session_dir = grok_home
        .join("sessions")
        .join("%2Ftmp%2Fgrok-project")
        .join("grok-snapshot-session");
    write_file(
        &session_dir.join("signals.json"),
        r#"{"contextTokensUsed":1500,"primaryModelId":"grok-4.5-build"}"#,
    );
    write_file(
        &session_dir.join("summary.json"),
        r#"{"updated_at":"2026-08-16T10:00:00Z","current_model_id":"grok-4.5-build"}"#,
    );
    write_file(
        &grok_home.join("logs/unified.jsonl"),
        r#"{"ts":"2026-08-16T10:00:00Z","sid":"grok-snapshot-session","msg":"shell.turn.inference_done","ctx":{"loop_index":1,"prompt_tokens":100,"cached_prompt_tokens":50,"completion_tokens":10,"reasoning_tokens":0}}"#,
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
fn grok_keeps_complete_turn_tokens_and_marks_partial_api_cost() {
    let root = unique_temp_dir("grok-api-cost-coverage");
    let grok_home = root.join("grok-home");
    write_grok_turn_session(&grok_home);
    write_partial_grok_inference_log(&grok_home);

    let args = [
        "grok",
        "monthly",
        "-j",
        "-O",
        "--timezone",
        "UTC",
        "--since",
        "2026-08-16",
        "--until",
        "2026-08-17",
    ];
    let env = [("GROK_HOME", grok_home.as_path()), ("HOME", root.as_path())];
    let (ok, stdout, stderr) = run_ccstats(&args, &env);
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));

    let json: Value = serde_json::from_slice(&stdout).expect("json");
    let row = &json.as_array().expect("array output")[0];
    assert_eq!(row["total_tokens"].as_i64(), Some(180));
    assert_close(row["cost"].as_f64().unwrap(), 0.000_252);
    let coverage = &row["api_equivalent_cost_coverage"];
    assert_eq!(coverage["total_tokens"].as_i64(), Some(180));
    assert_eq!(coverage["priced_tokens"].as_i64(), Some(120));
    assert_close(coverage["percent"].as_f64().unwrap(), 66.666_666_666_666_66);
    assert_eq!(coverage["complete"].as_bool(), Some(false));
    assert_eq!(coverage["cost_is_lower_bound"].as_bool(), Some(true));
    let summary = &row["grok_cost_summary"];
    assert_close(
        summary["api_equivalent"]["observed_usd"].as_f64().unwrap(),
        0.000_252,
    );
    assert_close(
        summary["api_equivalent"]["estimated_usd"].as_f64().unwrap(),
        0.000_395,
    );
    assert_close(
        summary["api_equivalent"]["range_usd"]["minimum"]
            .as_f64()
            .unwrap(),
        0.000_395,
    );
    assert_close(
        summary["api_equivalent"]["range_usd"]["maximum"]
            .as_f64()
            .unwrap(),
        0.000_538,
    );
    assert_close(
        summary["provider_metric"]["reported_usd"].as_f64().unwrap(),
        2.5,
    );
    assert!(summary["actual_billed_usd"].is_null());

    let table_args = [
        "grok",
        "monthly",
        "-O",
        "--timezone",
        "UTC",
        "--since",
        "2026-08-16",
        "--until",
        "2026-08-17",
    ];
    let (ok, stdout, stderr) = run_ccstats(&table_args, &env);
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    let table = String::from_utf8(stdout).expect("utf8 table");
    assert!(
        table.contains("120 / 180 completed-turn tokens (66.67%, partial)"),
        "table: {table}"
    );
    assert!(
        table.contains("API Eq. Price") && table.contains("~$0.00"),
        "table: {table}"
    );
    assert!(
        table.contains("Provider metric: $2.50") && table.contains("Actual billed: unavailable"),
        "table: {table}"
    );

    let daily_args = [
        "grok",
        "daily",
        "-O",
        "--timezone",
        "UTC",
        "--since",
        "2026-08-17",
        "--until",
        "2026-08-17",
    ];
    let (ok, stdout, stderr) = run_ccstats(&daily_args, &env);
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    let table = String::from_utf8(stdout).expect("utf8 daily table");
    let row = table
        .lines()
        .find(|line| line.contains("2026-08-17"))
        .expect("zero-coverage daily row");
    assert!(row.contains("N/A"), "row: {row}");

    let _ = fs::remove_dir_all(root);
}

#[test]
fn grok_does_not_add_snapshot_estimate_to_inference_cost() {
    let root = unique_temp_dir("grok-snapshot-inference-cost");
    let grok_home = root.join("grok-home");
    write_grok_snapshot_with_inference(&grok_home);

    let (ok, stdout, stderr) = run_ccstats(
        &[
            "grok",
            "daily",
            "-j",
            "-O",
            "--timezone",
            "UTC",
            "--since",
            "2026-08-16",
            "--until",
            "2026-08-16",
        ],
        &[("GROK_HOME", &grok_home), ("HOME", &root)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));

    let json: Value = serde_json::from_slice(&stdout).expect("json");
    let row = &json.as_array().expect("array output")[0];
    assert_eq!(row["total_tokens"].as_i64(), Some(1500));
    assert_close(row["cost"].as_f64().expect("inference cost"), 0.000_175);
    assert!(row.get("estimated_cost").is_none());

    let _ = fs::remove_dir_all(root);
}

#[test]
fn explicit_grok_home_never_reads_default_sessions() {
    let root = unique_temp_dir("grok-home-isolation");
    let explicit_home = root.join("isolated-grok-home");
    write_grok_turn_session(&root.join(".grok"));
    write_file(
        &explicit_home.join("logs/unified.jsonl"),
        r#"{"ts":"2026-08-16T00:00:00Z","sid":"isolated-session","msg":"shell.turn.inference_done","ctx":{"loop_index":1,"prompt_tokens":100,"cached_prompt_tokens":40,"completion_tokens":20,"reasoning_tokens":5}}"#,
    );

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
            "2026-08-16",
            "--until",
            "2026-08-16",
        ],
        &[("GROK_HOME", &explicit_home), ("HOME", &root)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));

    let json: Value = serde_json::from_slice(&stdout).expect("json");
    let row = &json.as_array().expect("array output")[0];
    assert_eq!(row["total_tokens"].as_i64(), Some(0));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn all_sources_reports_grok_cost_coverage_per_period() {
    let root = unique_temp_dir("all-sources-grok-coverage");
    let grok_home = root.join("grok-home");
    write_grok_turn_session(&grok_home);
    write_partial_grok_inference_log(&grok_home);
    write_claude_session_at(&root, "2026-08-16T12:00:00Z");

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
            "2026-08-16",
            "--until",
            "2026-08-17",
        ],
        &[("GROK_HOME", &grok_home), ("HOME", &root)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));

    let json: Value = serde_json::from_slice(&stdout).expect("json");
    let rows = json.as_array().expect("array output");
    let first = rows
        .iter()
        .find(|row| row["date"] == "2026-08-16")
        .expect("first day");
    let second = rows
        .iter()
        .find(|row| row["date"] == "2026-08-17")
        .expect("second day");
    let first_coverage = &first["api_equivalent_cost_coverage"];
    assert_eq!(first_coverage["total_tokens"].as_i64(), Some(120));
    assert_eq!(first_coverage["priced_tokens"].as_i64(), Some(120));
    assert_eq!(first_coverage["cost_is_lower_bound"].as_bool(), Some(false));
    let second_coverage = &second["api_equivalent_cost_coverage"];
    assert_eq!(second_coverage["total_tokens"].as_i64(), Some(60));
    assert_eq!(second_coverage["priced_tokens"].as_i64(), Some(0));
    assert_eq!(second_coverage["cost_is_lower_bound"].as_bool(), Some(true));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn no_cost_table_omits_api_cost_lower_bound_note() {
    let root = unique_temp_dir("grok-no-cost-coverage-note");
    let grok_home = root.join("grok-home");
    write_grok_turn_session(&grok_home);
    write_partial_grok_inference_log(&grok_home);

    let (ok, stdout, stderr) = run_ccstats(
        &[
            "grok",
            "daily",
            "-O",
            "--no-cost",
            "--timezone",
            "UTC",
            "--since",
            "2026-08-16",
            "--until",
            "2026-08-17",
        ],
        &[("GROK_HOME", &grok_home), ("HOME", &root)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    let table = String::from_utf8(stdout).expect("utf8 table");
    assert!(
        !table.contains("displayed cost is a lower bound"),
        "table: {table}"
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn source_flag_can_select_cursor_without_subcommand() {
    let root = unique_temp_dir("source-flag-cursor");
    let cursor_usage = root.join("cursor-usage.json");
    write_cursor_usage_file(&cursor_usage);

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
        &[("CURSOR_USAGE_FILE", &cursor_usage)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));

    let json: Value = serde_json::from_slice(&stdout).expect("json");
    let arr = json.as_array().expect("array output");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["date"].as_str(), Some("2026-02-06"));
    assert_eq!(arr[0]["input_tokens"].as_i64(), Some(100));
    assert_eq!(arr[0]["output_tokens"].as_i64(), Some(40));
    assert_eq!(arr[0]["cache_read_tokens"].as_i64(), Some(10));
    assert_eq!(arr[0]["total_tokens"].as_i64(), Some(150));
    assert_close(arr[0]["cache_hit_rate"].as_f64().unwrap(), 9.09);
    assert_eq!(
        arr[0]["models"].as_array().unwrap()[0].as_str(),
        Some("claude-4-sonnet")
    );

    let (ok, stdout, stderr) = run_ccstats(
        &[
            "daily",
            "--source",
            "cursor",
            "-O",
            "--no-cost",
            "--timezone",
            "UTC",
            "--since",
            "2026-02-06",
            "--until",
            "2026-02-06",
        ],
        &[("CURSOR_USAGE_FILE", &cursor_usage)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    let table = String::from_utf8(stdout).expect("utf8 table");
    assert!(table.contains("Cache Hit"), "table: {table}");
    assert!(table.contains("9.1%"), "table: {table}");

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
    assert_eq!(arr[0]["cache_hit_rate"].as_f64(), Some(0.0));
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
fn grok_daily_json_uses_turn_tokens_without_treating_cost_ticks_as_api_price() {
    let root = unique_temp_dir("grok-turn-usage");
    let grok_home = root.join("grok-home");
    write_grok_turn_session(&grok_home);

    let (ok, stdout, stderr) = run_ccstats(
        &[
            "grok",
            "daily",
            "-j",
            "-O",
            "--timezone",
            "UTC",
            "--since",
            "2026-08-16",
            "--until",
            "2026-08-16",
        ],
        &[("GROK_HOME", &grok_home), ("HOME", &root)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));

    let json: Value = serde_json::from_slice(&stdout).expect("json");
    let arr = json.as_array().expect("array output");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["date"].as_str(), Some("2026-08-16"));
    assert_eq!(arr[0]["input_tokens"].as_i64(), Some(60));
    assert_eq!(arr[0]["output_tokens"].as_i64(), Some(15));
    assert_eq!(arr[0]["reasoning_tokens"].as_i64(), Some(5));
    assert_eq!(arr[0]["cache_read_tokens"].as_i64(), Some(40));
    assert_eq!(arr[0]["total_tokens"].as_i64(), Some(120));
    assert_close(arr[0]["cache_hit_rate"].as_f64().unwrap(), 40.0);
    assert_close(arr[0]["cost"].as_f64().unwrap(), 0.0);
    assert_eq!(
        arr[0]["pricing_source"].as_str(),
        Some("calculated_api_equivalent")
    );
    assert!(arr[0].get("cost_kind").is_none() || arr[0]["cost_kind"].is_null());
    assert_eq!(
        arr[0]["models"].as_array().unwrap()[0].as_str(),
        Some("grok-4.5")
    );
    let coverage = &arr[0]["api_equivalent_cost_coverage"];
    assert_eq!(coverage["total_tokens"].as_i64(), Some(120));
    assert_eq!(coverage["priced_tokens"].as_i64(), Some(0));
    assert_eq!(coverage["cost_is_lower_bound"].as_bool(), Some(true));
    let summary = &arr[0]["grok_cost_summary"];
    assert!(summary["api_equivalent"]["estimated_usd"].is_null());
    assert_close(
        summary["provider_metric"]["reported_usd"].as_f64().unwrap(),
        2.0,
    );
    assert!(summary["actual_billed_usd"].is_null());

    let _ = fs::remove_dir_all(root);
}

#[test]
fn grok_daily_table_counts_model_calls_not_sessions() {
    let root = unique_temp_dir("grok-turn-calls");
    let grok_home = root.join("grok-home");
    write_grok_turn_session(&grok_home);

    let (ok, stdout, stderr) = run_ccstats(
        &[
            "grok",
            "daily",
            "-O",
            "--no-cost",
            "--timezone",
            "UTC",
            "--since",
            "2026-08-16",
            "--until",
            "2026-08-17",
        ],
        &[("GROK_HOME", &grok_home), ("HOME", &root)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    let table = String::from_utf8(stdout).expect("utf8 table");
    assert!(
        table.contains("5") && table.contains("Calls"),
        "table should count 5 model calls: {table}"
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn all_sources_does_not_treat_grok_cost_ticks_as_api_price() {
    let root = unique_temp_dir("all-sources-grok-real");
    let grok_home = root.join("grok-home");
    write_claude_session(&root);
    write_grok_turn_session(&grok_home);

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
            "2026-08-16",
            "--until",
            "2026-08-16",
        ],
        &[("HOME", &root), ("GROK_HOME", &grok_home)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));

    let json: Value = serde_json::from_slice(&stdout).expect("json");
    let arr = json.as_array().expect("array output");
    assert_eq!(arr.len(), 1);
    assert_close(arr[0]["cost"].as_f64().unwrap(), 0.0);
    assert!(arr[0].get("estimated_cost").is_none() || arr[0]["estimated_cost"].is_null());

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
