mod common;

use common::{run_ccstats, unique_temp_dir, write_file};
use serde_json::Value;
use std::fs;

#[test]
fn codex_daily_json_reads_session_data() {
    let root = unique_temp_dir("codex-daily");
    let codex_home = root.join("codex-home");
    let session_file = codex_home.join("sessions").join("test-session.jsonl");
    write_file(
        &session_file,
        r#"{"timestamp":"2026-02-06T10:00:00Z","type":"turn_context","payload":{"model":"gpt-5"}}
{"timestamp":"2026-02-06T10:00:00Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":100,"cached_input_tokens":20,"output_tokens":30,"reasoning_output_tokens":10,"total_tokens":140},"last_token_usage":{"input_tokens":100,"cached_input_tokens":20,"output_tokens":30,"reasoning_output_tokens":10,"total_tokens":140},"model":"gpt-5"}}}
"#,
    );

    let (ok, stdout, stderr) = run_ccstats(
        &[
            "codex",
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
        &[("CODEX_HOME", &codex_home)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));

    let json: Value = serde_json::from_slice(&stdout).expect("json");
    let arr = json.as_array().expect("array output");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["date"].as_str(), Some("2026-02-06"));
    // OpenAI output_tokens(30) includes reasoning_output_tokens(10).
    // After separating: non_cached_input=80, output=20, reasoning=10, cache_read=20 → total=130
    assert_eq!(arr[0]["total_tokens"].as_i64(), Some(130));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn codex_daily_json_counts_component_growth_when_total_tokens_missing_or_zero() {
    let root = unique_temp_dir("codex-missing-total-delta");
    let codex_home = root.join("codex-home");
    let session_file = codex_home.join("sessions").join("missing-total.jsonl");
    write_file(
        &session_file,
        r#"{"timestamp":"2026-02-06T10:00:00Z","type":"turn_context","payload":{"model":"gpt-5"}}
{"timestamp":"2026-02-06T10:00:00Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":100,"cached_input_tokens":20,"output_tokens":40,"reasoning_output_tokens":10},"model":"gpt-5"}}}
{"timestamp":"2026-02-06T10:00:01Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":100,"cached_input_tokens":20,"output_tokens":40,"reasoning_output_tokens":10},"model":"gpt-5"}}}
{"timestamp":"2026-02-06T10:00:02Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":180,"cached_input_tokens":35,"output_tokens":90,"reasoning_output_tokens":30,"total_tokens":0},"model":"gpt-5"}}}
"#,
    );

    let (ok, stdout, stderr) = run_ccstats(
        &[
            "codex",
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
        &[("CODEX_HOME", &codex_home)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));

    let json: Value = serde_json::from_slice(&stdout).expect("json");
    let arr = json.as_array().expect("array output");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["date"].as_str(), Some("2026-02-06"));
    assert_eq!(arr[0]["input_tokens"].as_i64(), Some(145));
    assert_eq!(arr[0]["cache_read_tokens"].as_i64(), Some(35));
    assert_eq!(arr[0]["output_tokens"].as_i64(), Some(60));
    assert_eq!(arr[0]["reasoning_tokens"].as_i64(), Some(30));
    assert_eq!(arr[0]["total_tokens"].as_i64(), Some(270));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn codex_daily_json_deduplicates_replayed_token_counts_across_files() {
    let root = unique_temp_dir("codex-replay-dedup");
    let codex_home = root.join("codex-home");
    let replay_a = codex_home.join("sessions").join("replay-a.jsonl");
    let replay_b = codex_home.join("sessions").join("replay-b.jsonl");
    let parent_meta = r#"{"timestamp":"2026-02-06T10:00:00Z","type":"session_meta","payload":{"id":"parent-session"}}"#;
    let fork_meta = r#"{"timestamp":"2026-02-06T10:00:00Z","type":"session_meta","payload":{"id":"forked-session"}}"#;
    let replayed = r#"{"timestamp":"2026-02-06T10:00:00Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":220,"cached_input_tokens":40,"output_tokens":80,"reasoning_output_tokens":20,"total_tokens":300},"last_token_usage":{"input_tokens":120,"cached_input_tokens":20,"output_tokens":50,"reasoning_output_tokens":10,"total_tokens":160},"model":"gpt-5"}}}"#;
    write_file(&replay_a, &format!("{parent_meta}\n{replayed}\n"));
    write_file(
        &replay_b,
        &format!(
            r#"{fork_meta}
{parent_meta}
{replayed}
{{"timestamp":"2026-02-06T10:01:00Z","type":"event_msg","payload":{{"type":"token_count","info":{{"total_token_usage":{{"input_tokens":360,"cached_input_tokens":70,"output_tokens":140,"reasoning_output_tokens":40,"total_tokens":500}},"last_token_usage":{{"input_tokens":140,"cached_input_tokens":30,"output_tokens":60,"reasoning_output_tokens":20,"total_tokens":200}},"model":"gpt-5"}}}}}}
"#
        ),
    );

    let (ok, stdout, stderr) = run_ccstats(
        &[
            "codex",
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
        &[("CODEX_HOME", &codex_home)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));

    let json: Value = serde_json::from_slice(&stdout).expect("json");
    let arr = json.as_array().expect("array output");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["date"].as_str(), Some("2026-02-06"));
    assert_eq!(arr[0]["total_tokens"].as_i64(), Some(370));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn codex_daily_json_keeps_identical_token_count_sequences_per_session() {
    let root = unique_temp_dir("codex-initial-session-scope");
    let codex_home = root.join("codex-home");
    let session_a = codex_home.join("sessions").join("session-a.jsonl");
    let session_b = codex_home.join("sessions").join("session-b.jsonl");
    let meta_a = r#"{"timestamp":"2026-02-06T10:00:00Z","type":"session_meta","payload":{"id":"session-a"}}"#;
    let meta_b = r#"{"timestamp":"2026-02-06T10:00:00Z","type":"session_meta","payload":{"id":"session-b"}}"#;
    let initial = r#"{"timestamp":"2026-02-06T10:00:00Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":100,"cached_input_tokens":20,"output_tokens":30,"reasoning_output_tokens":10,"total_tokens":130},"last_token_usage":{"input_tokens":100,"cached_input_tokens":20,"output_tokens":30,"reasoning_output_tokens":10,"total_tokens":130},"model":"gpt-5"}}}"#;
    let second = r#"{"timestamp":"2026-02-06T10:01:00Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":220,"cached_input_tokens":40,"output_tokens":80,"reasoning_output_tokens":20,"total_tokens":300},"last_token_usage":{"input_tokens":120,"cached_input_tokens":20,"output_tokens":50,"reasoning_output_tokens":10,"total_tokens":160},"model":"gpt-5"}}}"#;
    write_file(&session_a, &format!("{meta_a}\n{initial}\n{second}\n"));
    write_file(&session_b, &format!("{meta_b}\n{initial}\n{second}\n"));

    let (ok, stdout, stderr) = run_ccstats(
        &[
            "codex",
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
        &[("CODEX_HOME", &codex_home)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));

    let json: Value = serde_json::from_slice(&stdout).expect("json");
    let arr = json.as_array().expect("array output");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["total_tokens"].as_i64(), Some(600));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn source_flag_can_select_codex_without_subcommand() {
    let root = unique_temp_dir("source-flag-codex");
    let codex_home = root.join("codex-home");
    let session_file = codex_home.join("sessions").join("flag-session.jsonl");
    write_file(
        &session_file,
        r#"{"timestamp":"2026-02-06T10:00:00Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":10,"cached_input_tokens":0,"output_tokens":5,"reasoning_output_tokens":0,"total_tokens":15},"last_token_usage":{"input_tokens":10,"cached_input_tokens":0,"output_tokens":5,"reasoning_output_tokens":0,"total_tokens":15},"model":"gpt-5"}}}
"#,
    );

    let (ok, stdout, stderr) = run_ccstats(
        &[
            "daily",
            "--source",
            "codex",
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
        &[("CODEX_HOME", &codex_home)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));

    let json: Value = serde_json::from_slice(&stdout).expect("json");
    let arr = json.as_array().expect("array output");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["date"].as_str(), Some("2026-02-06"));
    assert_eq!(arr[0]["total_tokens"].as_i64(), Some(15));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn source_all_daily_json_merges_registered_sources() {
    let root = unique_temp_dir("source-all-daily");
    let codex_home = root.join("codex-home");
    let claude_session = root.join(".claude/projects/myapp/claude-session.jsonl");
    let codex_session = codex_home.join("sessions").join("codex-session.jsonl");

    write_file(
        &claude_session,
        r#"{"timestamp":"2026-02-06T10:00:00Z","message":{"id":"msg_1","model":"claude-3-5-sonnet-20241022","stop_reason":"end_turn","usage":{"input_tokens":100,"output_tokens":50,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}
"#,
    );
    write_file(
        &codex_session,
        r#"{"timestamp":"2026-02-06T11:00:00Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":10,"cached_input_tokens":0,"output_tokens":5,"reasoning_output_tokens":0,"total_tokens":15},"last_token_usage":{"input_tokens":10,"cached_input_tokens":0,"output_tokens":5,"reasoning_output_tokens":0,"total_tokens":15},"model":"gpt-5"}}}
"#,
    );

    let (ok, stdout, stderr) = run_ccstats(
        &[
            "daily",
            "--source",
            "all",
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
        &[("HOME", &root), ("CODEX_HOME", &codex_home)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));

    let json: Value = serde_json::from_slice(&stdout).expect("json");
    let arr = json.as_array().expect("array output");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["date"].as_str(), Some("2026-02-06"));
    assert_eq!(arr[0]["input_tokens"].as_i64(), Some(110));
    assert_eq!(arr[0]["output_tokens"].as_i64(), Some(55));
    assert_eq!(arr[0]["total_tokens"].as_i64(), Some(165));

    let models = arr[0]["models"].as_array().expect("models");
    assert_eq!(models.len(), 2);
    assert!(
        models
            .iter()
            .any(|model| model.as_str() == Some("3-5-sonnet"))
    );
    assert!(models.iter().any(|model| model.as_str() == Some("gpt-5")));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn sources_outputs_expose_same_capability_columns() {
    let expected = [
        "has_projects",
        "has_billing_blocks",
        "has_reasoning_tokens",
        "has_cache_creation",
        "needs_dedup",
    ];

    let (ok, stdout, stderr) = run_ccstats(&["sources"], &[]);
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    let table = String::from_utf8(stdout).expect("utf8 table");
    for column in expected {
        assert!(
            table.contains(column),
            "table output missing {column}: {table}"
        );
    }

    let (ok, stdout, stderr) = run_ccstats(&["sources", "--csv"], &[]);
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    let csv = String::from_utf8(stdout).expect("utf8 csv");
    let header = csv.lines().next().expect("csv header");
    let columns: Vec<&str> = header.split(',').skip(3).collect();
    assert_eq!(columns, expected);

    let (ok, stdout, stderr) = run_ccstats(&["sources", "--json"], &[]);
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    let json: Value = serde_json::from_slice(&stdout).expect("json");
    let rows = json.as_array().expect("array output");
    assert!(!rows.is_empty());
    for row in rows {
        let capabilities = row["capabilities"].as_object().expect("capabilities");
        let mut columns: Vec<&str> = capabilities.keys().map(String::as_str).collect();
        columns.sort_unstable();
        let mut sorted_expected = expected.to_vec();
        sorted_expected.sort_unstable();
        assert_eq!(columns, sorted_expected);
    }
}

#[test]
fn codex_subcommand_conflicts_with_different_source_flag() {
    let root = unique_temp_dir("source-flag-conflict");
    let (ok, _stdout, stderr) = run_ccstats(
        &["codex", "daily", "--source", "claude", "-O", "--no-cost"],
        &[("HOME", &root)],
    );
    assert!(!ok, "expected conflict failure");
    let stderr = String::from_utf8_lossy(&stderr);
    assert!(stderr.contains("conflicts with --source"));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn codex_subcommand_conflicts_with_source_all() {
    let root = unique_temp_dir("source-all-conflict");
    let (ok, _stdout, stderr) = run_ccstats(
        &["codex", "daily", "--source", "all", "-O", "--no-cost"],
        &[("HOME", &root)],
    );
    assert!(!ok, "expected conflict failure");
    let stderr = String::from_utf8_lossy(&stderr);
    assert!(stderr.contains("conflicts with --source all"));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn malformed_records_are_reported_without_debug_flag() {
    let root = unique_temp_dir("malformed-record-warning");
    let codex_home = root.join("codex-home");
    let session_file = codex_home.join("sessions").join("malformed.jsonl");
    write_file(
        &session_file,
        r#"{"timestamp":"2026-02-06T10:00:00Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":10,"cached_input_tokens":0,"output_tokens":5,"reasoning_output_tokens":0,"total_tokens":15},"last_token_usage":{"input_tokens":10,"cached_input_tokens":0,"output_tokens":5,"reasoning_output_tokens":0,"total_tokens":15},"model":"gpt-5"}}}
{"timestamp":"not-json"
"#,
    );

    let (ok, stdout, stderr) = run_ccstats(
        &[
            "daily",
            "--source",
            "codex",
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
        &[("CODEX_HOME", &codex_home)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    let stderr = String::from_utf8_lossy(&stderr);
    assert!(stderr.contains("malformed records"), "stderr: {stderr}");
    let json: Value = serde_json::from_slice(&stdout).expect("json");
    let arr = json.as_array().expect("array output");
    assert_eq!(arr[0]["data_quality"]["valid_entries"].as_i64(), Some(1));
    assert_eq!(
        arr[0]["data_quality"]["dedup_skipped_entries"].as_i64(),
        Some(0)
    );
    assert_eq!(arr[0]["data_quality"]["parse_errors"].as_u64(), Some(1));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn malformed_records_are_reported_in_csv_metadata() {
    let root = unique_temp_dir("malformed-record-csv-metadata");
    let codex_home = root.join("codex-home");
    let session_file = codex_home.join("sessions").join("malformed.jsonl");
    write_file(
        &session_file,
        r#"{"timestamp":"2026-02-06T10:00:00Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":10,"cached_input_tokens":0,"output_tokens":5,"reasoning_output_tokens":0,"total_tokens":15},"last_token_usage":{"input_tokens":10,"cached_input_tokens":0,"output_tokens":5,"reasoning_output_tokens":0,"total_tokens":15},"model":"gpt-5"}}}
{"timestamp":"not-json"
"#,
    );

    let (ok, stdout, stderr) = run_ccstats(
        &[
            "daily",
            "--source",
            "codex",
            "--csv",
            "-O",
            "--no-cost",
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

    let output = String::from_utf8(stdout).expect("utf8 stdout");
    assert!(output.contains("# data_quality,1,0,1"), "stdout: {output}");

    let _ = fs::remove_dir_all(root);
}

#[test]
fn all_malformed_json_outputs_metadata_only_row() {
    let root = unique_temp_dir("all-malformed-json-metadata");
    let codex_home = root.join("codex-home");
    let session_file = codex_home.join("sessions").join("malformed.jsonl");
    write_file(
        &session_file,
        r#"{"timestamp":"not-json"
"#,
    );

    let (ok, stdout, stderr) = run_ccstats(
        &[
            "daily",
            "--source",
            "codex",
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
        &[("CODEX_HOME", &codex_home)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));

    let json: Value = serde_json::from_slice(&stdout).expect("json");
    let arr = json.as_array().expect("array output");
    assert_eq!(arr.len(), 1);
    assert!(arr[0].get("date").is_none());
    assert_eq!(arr[0]["data_quality"]["valid_entries"].as_i64(), Some(0));
    assert_eq!(arr[0]["data_quality"]["parse_errors"].as_u64(), Some(1));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn codex_reasoning_tokens_not_double_counted() {
    let root = unique_temp_dir("codex-reasoning");
    let codex_home = root.join("codex-home");
    let session_file = codex_home.join("sessions").join("reasoning-session.jsonl");
    // OpenAI's output_tokens INCLUDES reasoning_output_tokens as a subset.
    // output_tokens=500 with reasoning_output_tokens=200 means 300 non-reasoning + 200 reasoning.
    write_file(
        &session_file,
        r#"{"timestamp":"2026-02-06T10:00:00Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":1000,"cached_input_tokens":0,"output_tokens":500,"reasoning_output_tokens":200,"total_tokens":1500},"last_token_usage":{"input_tokens":1000,"cached_input_tokens":0,"output_tokens":500,"reasoning_output_tokens":200,"total_tokens":1500},"model":"gpt-5.2-codex"}}}
"#,
    );

    let (ok, stdout, stderr) = run_ccstats(
        &[
            "codex",
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
        &[("CODEX_HOME", &codex_home)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));

    let json: Value = serde_json::from_slice(&stdout).expect("json");
    let arr = json.as_array().expect("array output");
    assert_eq!(arr.len(), 1);
    // output_tokens should be non-reasoning only: 500 - 200 = 300
    assert_eq!(arr[0]["output_tokens"].as_i64(), Some(300));
    // reasoning_tokens should be the separated reasoning portion
    assert_eq!(arr[0]["reasoning_tokens"].as_i64(), Some(200));
    // total = input(1000) + output(300) + reasoning(200) + cache(0) = 1500, no double-counting
    assert_eq!(arr[0]["total_tokens"].as_i64(), Some(1500));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn codex_session_json_orders_by_actual_timestamp() {
    let root = unique_temp_dir("codex-session-order");
    let codex_home = root.join("codex-home");
    let session_a = codex_home.join("sessions").join("a.jsonl");
    let session_b = codex_home.join("sessions").join("b.jsonl");

    write_file(
        &session_a,
        r#"{"timestamp":"2026-02-06T23:00:00+08:00","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":1,"cached_input_tokens":0,"output_tokens":1,"reasoning_output_tokens":0,"total_tokens":2},"last_token_usage":{"input_tokens":1,"cached_input_tokens":0,"output_tokens":1,"reasoning_output_tokens":0,"total_tokens":2},"model":"gpt-5"}}}
"#,
    );
    write_file(
        &session_b,
        r#"{"timestamp":"2026-02-06T16:00:00Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":1,"cached_input_tokens":0,"output_tokens":1,"reasoning_output_tokens":0,"total_tokens":2},"last_token_usage":{"input_tokens":1,"cached_input_tokens":0,"output_tokens":1,"reasoning_output_tokens":0,"total_tokens":2},"model":"gpt-5"}}}
"#,
    );

    let (ok, stdout, stderr) = run_ccstats(
        &[
            "codex",
            "session",
            "-j",
            "-O",
            "--no-cost",
            "--timezone",
            "UTC",
            "--since",
            "2026-02-06",
            "--until",
            "2026-02-06",
            "--order",
            "desc",
        ],
        &[("CODEX_HOME", &codex_home)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));

    let json: Value = serde_json::from_slice(&stdout).expect("json");
    let arr = json.as_array().expect("array output");
    assert_eq!(arr.len(), 2);
    assert_eq!(arr[0]["session_id"].as_str(), Some("b")); // 16:00Z, newer
    assert_eq!(arr[1]["session_id"].as_str(), Some("a")); // 15:00Z, older

    let _ = fs::remove_dir_all(root);
}

#[test]
fn codex_session_csv_orders_by_actual_timestamp() {
    let root = unique_temp_dir("codex-session-csv-order");
    let codex_home = root.join("codex-home");
    let session_a = codex_home.join("sessions").join("a.jsonl");
    let session_b = codex_home.join("sessions").join("b.jsonl");

    write_file(
        &session_a,
        r#"{"timestamp":"2026-02-06T23:00:00+08:00","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":1,"cached_input_tokens":0,"output_tokens":1,"reasoning_output_tokens":0,"total_tokens":2},"last_token_usage":{"input_tokens":1,"cached_input_tokens":0,"output_tokens":1,"reasoning_output_tokens":0,"total_tokens":2},"model":"gpt-5"}}}
"#,
    );
    write_file(
        &session_b,
        r#"{"timestamp":"2026-02-06T16:00:00Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":1,"cached_input_tokens":0,"output_tokens":1,"reasoning_output_tokens":0,"total_tokens":2},"last_token_usage":{"input_tokens":1,"cached_input_tokens":0,"output_tokens":1,"reasoning_output_tokens":0,"total_tokens":2},"model":"gpt-5"}}}
"#,
    );

    let (ok, stdout, stderr) = run_ccstats(
        &[
            "codex",
            "session",
            "--csv",
            "-O",
            "--no-cost",
            "--timezone",
            "UTC",
            "--since",
            "2026-02-06",
            "--until",
            "2026-02-06",
            "--order",
            "desc",
        ],
        &[("CODEX_HOME", &codex_home)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));

    let csv = String::from_utf8(stdout).expect("utf8 csv");
    let mut lines = csv.lines();
    let _header = lines.next().expect("csv header");
    let first = lines.next().expect("first row");
    let second = lines.next().expect("second row");

    assert!(first.starts_with("b,"), "expected b first, got: {first}");
    assert!(second.starts_with("a,"), "expected a second, got: {second}");

    let _ = fs::remove_dir_all(root);
}

#[test]
fn codex_session_json_includes_reasoning_tokens() {
    let root = unique_temp_dir("codex-session-reasoning");
    let codex_home = root.join("codex-home");
    let session_file = codex_home.join("sessions").join("reasoning-session.jsonl");
    write_file(
        &session_file,
        r#"{"timestamp":"2026-02-06T10:00:00Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":1000,"cached_input_tokens":0,"output_tokens":500,"reasoning_output_tokens":200,"total_tokens":1500},"last_token_usage":{"input_tokens":1000,"cached_input_tokens":0,"output_tokens":500,"reasoning_output_tokens":200,"total_tokens":1500},"model":"gpt-5.2-codex"}}}
"#,
    );

    let (ok, stdout, stderr) = run_ccstats(
        &[
            "codex",
            "session",
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
        &[("CODEX_HOME", &codex_home)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));

    let json: Value = serde_json::from_slice(&stdout).expect("json");
    let arr = json.as_array().expect("array output");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["reasoning_tokens"].as_i64(), Some(200));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn codex_session_csv_includes_reasoning_and_cache_tokens() {
    let root = unique_temp_dir("codex-session-reasoning-csv");
    let codex_home = root.join("codex-home");
    let session_file = codex_home.join("sessions").join("reasoning-session.jsonl");
    write_file(
        &session_file,
        r#"{"timestamp":"2026-02-06T10:00:00Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":1000,"cached_input_tokens":100,"output_tokens":500,"reasoning_output_tokens":200,"total_tokens":1500},"last_token_usage":{"input_tokens":1000,"cached_input_tokens":100,"output_tokens":500,"reasoning_output_tokens":200,"total_tokens":1500},"model":"gpt-5.2-codex"}}}
"#,
    );

    let (ok, stdout, stderr) = run_ccstats(
        &[
            "codex",
            "session",
            "--csv",
            "-O",
            "--no-cost",
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

    let csv = String::from_utf8(stdout).expect("utf8 csv");
    let mut lines = csv.lines();
    let header = lines.next().expect("header");
    let row = lines.next().expect("row");
    assert_eq!(
        header,
        "session_id,project_path,first_timestamp,last_timestamp,input_tokens,output_tokens,reasoning_tokens,cache_creation_tokens,cache_read_tokens,total_tokens"
    );
    assert_eq!(
        row,
        "reasoning-session,,2026-02-06T10:00:00Z,2026-02-06T10:00:00Z,900,300,200,0,100,1500"
    );

    let _ = fs::remove_dir_all(root);
}
