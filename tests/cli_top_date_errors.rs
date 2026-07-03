mod common;

use common::{run_ccstats, unique_temp_dir, write_file};
use serde_json::Value;
use std::fs;

#[test]
fn top_model_json_ranks_consumers_with_share() {
    let root = unique_temp_dir("top-model-json");
    // Two models in the same day; gpt-5 has more billable tokens, so it
    // should rank #1 regardless of cost knowledge.
    let claude_file = root.join(".claude/projects/myproject/session.jsonl");
    write_file(
        &claude_file,
        r#"{"timestamp":"2026-02-06T10:00:00Z","message":{"id":"msg_1","model":"claude-3-5-sonnet-20241022","stop_reason":"end_turn","usage":{"input_tokens":100,"output_tokens":50,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}
{"timestamp":"2026-02-06T11:00:00Z","message":{"id":"msg_2","model":"claude-3-5-sonnet-20241022","stop_reason":"end_turn","usage":{"input_tokens":200,"output_tokens":80,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}
{"timestamp":"2026-02-06T12:00:00Z","message":{"id":"msg_3","model":"claude-3-haiku-20240307","stop_reason":"end_turn","usage":{"input_tokens":50,"output_tokens":10,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}
"#,
    );

    let (ok, stdout, stderr) = run_ccstats(
        &[
            "top",
            "-j",
            "-O",
            "--timezone",
            "UTC",
            "--since",
            "2026-02-06",
            "--until",
            "2026-02-06",
        ],
        &[("HOME", &root)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));

    let json: Value = serde_json::from_slice(&stdout).expect("json");
    assert_eq!(json["dimension"].as_str(), Some("model"));
    let entries = json["entries"].as_array().expect("entries");
    assert_eq!(entries.len(), 2);
    // First row is whichever model has more cost; both rows must report
    // a numeric share that sums close to 100.
    let share_sum: f64 = entries
        .iter()
        .map(|e| e["share_percent"].as_f64().unwrap_or(0.0))
        .sum();
    assert!(
        (share_sum - 100.0).abs() < 0.5,
        "share_sum should be ~100, got {share_sum}"
    );
    // Rank field must be present and start at 1.
    assert_eq!(entries[0]["rank"].as_i64(), Some(1));
    assert_eq!(entries[1]["rank"].as_i64(), Some(2));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn top_csv_includes_rank_and_share_columns() {
    let root = unique_temp_dir("top-csv");
    let claude_file = root.join(".claude/projects/myproject/session.jsonl");
    write_file(
        &claude_file,
        r#"{"timestamp":"2026-02-06T10:00:00Z","message":{"id":"msg_1","model":"claude-3-5-sonnet-20241022","stop_reason":"end_turn","usage":{"input_tokens":100,"output_tokens":50,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}
"#,
    );

    let (ok, stdout, stderr) = run_ccstats(
        &[
            "top",
            "--csv",
            "-O",
            "--timezone",
            "UTC",
            "--since",
            "2026-02-06",
            "--until",
            "2026-02-06",
        ],
        &[("HOME", &root)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));

    let output = String::from_utf8(stdout).expect("utf8");
    let lines: Vec<&str> = output.lines().collect();
    assert!(
        lines[0].starts_with("rank,model,count,"),
        "csv header: {}",
        lines[0]
    );
    assert!(
        lines[0].contains("share_percent"),
        "csv header missing share_percent: {}",
        lines[0]
    );
    assert!(
        lines[0].contains("cost_usd"),
        "cost_usd should be present when --no-cost is absent: {}",
        lines[0]
    );
    assert_eq!(lines.len(), 2, "header + 1 model row");
    assert!(lines[1].starts_with("1,3-5-sonnet,"), "row: {}", lines[1]);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn top_limit_caps_displayed_rows() {
    let root = unique_temp_dir("top-limit");
    // Five distinct models; limit=2 should cap output to two entries.
    let claude_file = root.join(".claude/projects/myproject/session.jsonl");
    write_file(
        &claude_file,
        r#"{"timestamp":"2026-02-06T10:00:00Z","message":{"id":"m1","model":"claude-3-5-sonnet-20241022","stop_reason":"end_turn","usage":{"input_tokens":500,"output_tokens":50,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}
{"timestamp":"2026-02-06T10:01:00Z","message":{"id":"m2","model":"claude-3-5-haiku-20241022","stop_reason":"end_turn","usage":{"input_tokens":400,"output_tokens":40,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}
{"timestamp":"2026-02-06T10:02:00Z","message":{"id":"m3","model":"claude-3-opus-20240229","stop_reason":"end_turn","usage":{"input_tokens":300,"output_tokens":30,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}
{"timestamp":"2026-02-06T10:03:00Z","message":{"id":"m4","model":"claude-2","stop_reason":"end_turn","usage":{"input_tokens":200,"output_tokens":20,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}
{"timestamp":"2026-02-06T10:04:00Z","message":{"id":"m5","model":"claude-instant","stop_reason":"end_turn","usage":{"input_tokens":100,"output_tokens":10,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}
"#,
    );

    let (ok, stdout, stderr) = run_ccstats(
        &[
            "top",
            "--limit",
            "2",
            "-j",
            "-O",
            "--timezone",
            "UTC",
            "--since",
            "2026-02-06",
            "--until",
            "2026-02-06",
        ],
        &[("HOME", &root)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));

    let json: Value = serde_json::from_slice(&stdout).expect("json");
    assert_eq!(json["limit"].as_i64(), Some(2));
    assert_eq!(json["displayed"].as_i64(), Some(2));
    assert_eq!(json["total_rows"].as_i64(), Some(5));
    assert_eq!(json["entries"].as_array().unwrap().len(), 2);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn top_dim_project_ranks_projects() {
    let root = unique_temp_dir("top-project");
    let p1 = root.join(".claude/projects/alpha/s1.jsonl");
    let p2 = root.join(".claude/projects/beta/s2.jsonl");
    write_file(
        &p1,
        r#"{"timestamp":"2026-02-06T10:00:00Z","message":{"id":"a1","model":"claude-3-5-sonnet-20241022","stop_reason":"end_turn","usage":{"input_tokens":1000,"output_tokens":500,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}
"#,
    );
    write_file(
        &p2,
        r#"{"timestamp":"2026-02-06T10:00:00Z","message":{"id":"b1","model":"claude-3-5-sonnet-20241022","stop_reason":"end_turn","usage":{"input_tokens":100,"output_tokens":50,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}
"#,
    );

    let (ok, stdout, stderr) = run_ccstats(
        &[
            "top",
            "--dim",
            "project",
            "-j",
            "-O",
            "--timezone",
            "UTC",
            "--since",
            "2026-02-06",
            "--until",
            "2026-02-06",
        ],
        &[("HOME", &root)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));

    let json: Value = serde_json::from_slice(&stdout).expect("json");
    assert_eq!(json["dimension"].as_str(), Some("project"));
    let entries = json["entries"].as_array().expect("entries");
    assert_eq!(entries.len(), 2);
    // Alpha has 10x the tokens of beta, so it must rank first.
    assert_eq!(entries[0]["name"].as_str(), Some("alpha"));
    assert_eq!(entries[1]["name"].as_str(), Some("beta"));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn top_limit_zero_exits_with_error() {
    let root = unique_temp_dir("top-limit-zero");
    let claude_file = root.join(".claude/projects/myproject/session.jsonl");
    write_file(
        &claude_file,
        r#"{"timestamp":"2026-02-06T10:00:00Z","message":{"id":"x1","model":"claude-3-5-sonnet-20241022","stop_reason":"end_turn","usage":{"input_tokens":10,"output_tokens":5,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}
"#,
    );

    let (ok, _stdout, stderr) = run_ccstats(&["top", "--limit", "0", "-O"], &[("HOME", &root)]);
    assert!(!ok, "limit=0 must fail");
    let err = String::from_utf8_lossy(&stderr);
    assert!(
        err.contains("--limit"),
        "error should mention --limit: {err}"
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn since_after_until_exits_with_error() {
    let root = unique_temp_dir("date-range");
    let claude_file = root.join(".claude/projects/myproject/session.jsonl");
    write_file(
        &claude_file,
        r#"{"timestamp":"2026-02-06T12:00:00Z","message":{"id":"msg_1","model":"claude-3-5-sonnet-20241022","stop_reason":"end_turn","usage":{"input_tokens":100,"output_tokens":50,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}
"#,
    );

    let (ok, _stdout, stderr) = run_ccstats(
        &[
            "daily",
            "-O",
            "--since",
            "2026-03-01",
            "--until",
            "2026-01-01",
        ],
        &[("HOME", &root)],
    );
    assert!(!ok, "should fail when --since is after --until");
    let err = String::from_utf8_lossy(&stderr);
    assert!(
        err.contains("--since") && err.contains("--until"),
        "error should mention both flags: {err}"
    );

    let _ = fs::remove_dir_all(root);
}
