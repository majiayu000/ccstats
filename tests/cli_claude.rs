mod common;

use common::{run_ccstats, unique_temp_dir, write_file};
use serde_json::Value;
use std::fs;

#[test]
fn claude_project_json_aggregates_sessions() {
    let root = unique_temp_dir("claude-project");
    // Two sessions in the same project, one session in a different project
    let session_a = root.join(".claude/projects/myapp/session-a.jsonl");
    let session_b = root.join(".claude/projects/myapp/session-b.jsonl");
    let session_c = root.join(".claude/projects/other-project/session-c.jsonl");

    // Session A: sonnet, 100 input + 50 output + 10 cache_creation + 20 cache_read = 180 total
    write_file(
        &session_a,
        r#"{"timestamp":"2026-02-06T10:00:00Z","message":{"id":"msg_1","model":"claude-3-5-sonnet-20241022","stop_reason":"end_turn","usage":{"input_tokens":100,"output_tokens":50,"cache_creation_input_tokens":10,"cache_read_input_tokens":20}}}
"#,
    );
    // Session B: opus, 200 input + 80 output = 280 total
    write_file(
        &session_b,
        r#"{"timestamp":"2026-02-06T11:00:00Z","message":{"id":"msg_2","model":"claude-4-opus-20250514","stop_reason":"end_turn","usage":{"input_tokens":200,"output_tokens":80,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}
"#,
    );
    // Session C: sonnet, 50 input + 25 output = 75 total
    write_file(
        &session_c,
        r#"{"timestamp":"2026-02-06T12:00:00Z","message":{"id":"msg_3","model":"claude-3-5-sonnet-20241022","stop_reason":"end_turn","usage":{"input_tokens":50,"output_tokens":25,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}
"#,
    );

    let (ok, stdout, stderr) = run_ccstats(
        &[
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
        &[("HOME", &root)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));

    let json: Value = serde_json::from_slice(&stdout).expect("json");
    let arr = json.as_array().expect("array output");
    assert_eq!(arr.len(), 2, "should have 2 projects");

    // Find each project by name (sort order depends on cost, which is 0 with --no-cost)
    let myapp = arr
        .iter()
        .find(|p| p["project"].as_str() == Some("myapp"))
        .expect("myapp project");
    let other = arr
        .iter()
        .find(|p| p["project"].as_str() == Some("other-project"))
        .expect("other-project");

    assert_eq!(myapp["session_count"].as_i64(), Some(2));
    assert_eq!(myapp["total_tokens"].as_i64(), Some(460));
    // Models should be sorted alphabetically
    let models: Vec<&str> = myapp["models"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(models.len(), 2);
    assert!(models[0] < models[1], "models should be sorted: {models:?}");

    assert_eq!(other["session_count"].as_i64(), Some(1));
    assert_eq!(other["total_tokens"].as_i64(), Some(75));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn claude_project_json_ignores_sidechain_subagent_logs() {
    let root = unique_temp_dir("claude-project-subagents");
    let session_a = root.join(".claude/projects/myapp/session-a.jsonl");
    let session_b = root.join(".claude/projects/myapp/subagents/agent-a.jsonl");

    write_file(
        &session_a,
        r#"{"timestamp":"2026-02-06T10:00:00Z","message":{"id":"msg_1","model":"claude-3-5-sonnet-20241022","stop_reason":"end_turn","usage":{"input_tokens":100,"output_tokens":50}}}
"#,
    );
    write_file(
        &session_b,
        r#"{"timestamp":"2026-02-06T11:00:00Z","isSidechain":true,"message":{"id":"msg_2","model":"gpt-5.2-codex","stop_reason":"end_turn","usage":{"input_tokens":200,"output_tokens":80}}}
"#,
    );

    let (ok, stdout, stderr) = run_ccstats(
        &[
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
        &[("HOME", &root)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));

    let json: Value = serde_json::from_slice(&stdout).expect("json");
    let arr = json.as_array().expect("array output");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["project"].as_str(), Some("myapp"));
    assert_eq!(arr[0]["project_path"].as_str(), Some("myapp"));
    assert_eq!(arr[0]["session_count"].as_i64(), Some(1));
    assert_eq!(arr[0]["total_tokens"].as_i64(), Some(150));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn claude_session_json_keeps_same_file_stem_from_different_projects_separate() {
    let root = unique_temp_dir("claude-session-separate-stems");
    let session_a = root.join(".claude/projects/projA/shared.jsonl");
    let session_b = root.join(".claude/projects/projB/shared.jsonl");

    write_file(
        &session_a,
        r#"{"timestamp":"2026-02-06T10:00:00Z","message":{"id":"msg_a","model":"claude-3-5-sonnet-20241022","stop_reason":"end_turn","usage":{"input_tokens":100,"output_tokens":20}}}
"#,
    );
    write_file(
        &session_b,
        r#"{"timestamp":"2026-02-06T11:00:00Z","message":{"id":"msg_b","model":"claude-3-5-sonnet-20241022","stop_reason":"end_turn","usage":{"input_tokens":200,"output_tokens":30}}}
"#,
    );

    let (ok, stdout, stderr) = run_ccstats(
        &[
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
        &[("HOME", &root)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));

    let json: Value = serde_json::from_slice(&stdout).expect("json");
    let arr = json.as_array().expect("array output");
    assert_eq!(arr.len(), 2);
    assert_eq!(arr[0]["session_id"].as_str(), Some("shared"));
    assert_eq!(arr[0]["project_path"].as_str(), Some("projB"));
    assert_eq!(arr[0]["total_tokens"].as_i64(), Some(230));
    assert_eq!(arr[1]["session_id"].as_str(), Some("shared"));
    assert_eq!(arr[1]["project_path"].as_str(), Some("projA"));
    assert_eq!(arr[1]["total_tokens"].as_i64(), Some(120));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn claude_blocks_json_groups_by_5h_window() {
    let root = unique_temp_dir("claude-blocks");
    let session = root.join(".claude/projects/myapp/session-blocks.jsonl");

    // Entry at 10:00 UTC → block 10:00-15:00
    // Entry at 14:30 UTC → same block 10:00-15:00
    // Entry at 15:00 UTC → block 15:00-20:00
    write_file(
        &session,
        r#"{"timestamp":"2026-02-06T10:00:00Z","message":{"id":"msg_a","model":"claude-3-5-sonnet-20241022","stop_reason":"end_turn","usage":{"input_tokens":100,"output_tokens":50,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}
{"timestamp":"2026-02-06T14:30:00Z","message":{"id":"msg_b","model":"claude-3-5-sonnet-20241022","stop_reason":"end_turn","usage":{"input_tokens":200,"output_tokens":100,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}
{"timestamp":"2026-02-06T15:00:00Z","message":{"id":"msg_c","model":"claude-4-opus-20250514","stop_reason":"end_turn","usage":{"input_tokens":300,"output_tokens":150,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}
"#,
    );

    let (ok, stdout, stderr) = run_ccstats(
        &[
            "blocks",
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
        &[("HOME", &root)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));

    let json: Value = serde_json::from_slice(&stdout).expect("json");
    let arr = json.as_array().expect("array output");
    assert_eq!(arr.len(), 2, "should have 2 blocks");

    // Default sort is asc by block_start
    let block_10 = &arr[0];
    assert!(
        block_10["block_start"].as_str().unwrap().contains("10:00"),
        "first block should start at 10:00"
    );
    assert_eq!(block_10["block_end"].as_str(), Some("15:00"));
    // 100+50 + 200+100 = 450
    assert_eq!(block_10["total_tokens"].as_i64(), Some(450));

    let block_15 = &arr[1];
    assert!(
        block_15["block_start"].as_str().unwrap().contains("15:00"),
        "second block should start at 15:00"
    );
    assert_eq!(block_15["block_end"].as_str(), Some("20:00"));
    // 300+150 = 450
    assert_eq!(block_15["total_tokens"].as_i64(), Some(450));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn claude_dedup_keeps_completed_message() {
    let root = unique_temp_dir("claude-dedup");
    let session = root.join(".claude/projects/myapp/session-dedup.jsonl");

    // Same message ID: first without stop_reason (streaming), second with stop_reason (completed)
    // Dedup should keep the completed one with accurate token counts
    write_file(
        &session,
        r#"{"timestamp":"2026-02-06T10:00:00Z","message":{"id":"msg_dup","model":"claude-3-5-sonnet-20241022","usage":{"input_tokens":50,"output_tokens":10,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}
{"timestamp":"2026-02-06T10:00:01Z","message":{"id":"msg_dup","model":"claude-3-5-sonnet-20241022","stop_reason":"end_turn","usage":{"input_tokens":100,"output_tokens":50,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}
"#,
    );

    let (ok, stdout, stderr) = run_ccstats(
        &[
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
        &[("HOME", &root)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));

    let json: Value = serde_json::from_slice(&stdout).expect("json");
    let arr = json.as_array().expect("array output");
    assert_eq!(arr.len(), 1);
    // Should use the completed message's tokens (100+50=150), not the streaming one (50+10=60)
    assert_eq!(arr[0]["total_tokens"].as_i64(), Some(150));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn claude_dedup_keeps_same_message_id_from_different_files() {
    let root = unique_temp_dir("claude-dedup-scope");
    let file_a = root.join(".claude/projects/projA/a.jsonl");
    let file_b = root.join(".claude/projects/projB/b.jsonl");

    write_file(
        &file_a,
        r#"{"timestamp":"2026-02-06T10:00:00Z","message":{"id":"msg_dup","model":"claude-3-5-sonnet-20241022","stop_reason":"end_turn","usage":{"input_tokens":100,"output_tokens":50}}}
"#,
    );
    write_file(
        &file_b,
        r#"{"timestamp":"2026-02-06T11:00:00Z","message":{"id":"msg_dup","model":"claude-3-5-sonnet-20241022","stop_reason":"end_turn","usage":{"input_tokens":200,"output_tokens":80}}}
"#,
    );

    let (ok, stdout, stderr) = run_ccstats(
        &[
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
        &[("HOME", &root)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));

    let json: Value = serde_json::from_slice(&stdout).expect("json");
    let arr = json.as_array().expect("array output");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["total_tokens"].as_i64(), Some(430));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn claude_daily_json_reads_home_projects() {
    let root = unique_temp_dir("claude-daily");
    let claude_file = root.join(".claude/projects/myproject/session-a.jsonl");
    write_file(
        &claude_file,
        r#"{"timestamp":"2026-02-06T12:00:00Z","message":{"id":"msg_1","model":"anthropic.claude-3-5-sonnet-20241022","stop_reason":"end_turn","usage":{"input_tokens":100,"output_tokens":50,"cache_creation_input_tokens":10,"cache_read_input_tokens":20}}}
"#,
    );

    let (ok, stdout, stderr) = run_ccstats(
        &[
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
        &[("HOME", &root)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));

    let json: Value = serde_json::from_slice(&stdout).expect("json");
    let arr = json.as_array().expect("array output");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["date"].as_str(), Some("2026-02-06"));
    assert_eq!(arr[0]["total_tokens"].as_i64(), Some(180));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn claude_daily_json_reads_claude_config_dir_before_home() {
    let root = unique_temp_dir("claude-config-dir");
    let claude_config_dir = root.join("custom-claude");
    let home_file = root.join(".claude/projects/home-project/session-a.jsonl");
    let config_file = claude_config_dir.join("projects/config-project/session-a.jsonl");
    write_file(
        &home_file,
        r#"{"timestamp":"2026-02-06T12:00:00Z","message":{"id":"msg_home","model":"anthropic.claude-3-5-sonnet-20241022","stop_reason":"end_turn","usage":{"input_tokens":900,"output_tokens":90,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}
"#,
    );
    write_file(
        &config_file,
        r#"{"timestamp":"2026-02-06T12:00:00Z","message":{"id":"msg_config","model":"anthropic.claude-3-5-sonnet-20241022","stop_reason":"end_turn","usage":{"input_tokens":100,"output_tokens":50,"cache_creation_input_tokens":10,"cache_read_input_tokens":20}}}
"#,
    );

    let (ok, stdout, stderr) = run_ccstats(
        &[
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
        &[("HOME", &root), ("CLAUDE_CONFIG_DIR", &claude_config_dir)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));

    let json: Value = serde_json::from_slice(&stdout).expect("json");
    let arr = json.as_array().expect("array output");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["date"].as_str(), Some("2026-02-06"));
    assert_eq!(arr[0]["total_tokens"].as_i64(), Some(180));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn claude_daily_json_missing_claude_config_dir_returns_no_data() {
    let root = unique_temp_dir("claude-config-dir-missing");
    let missing_config_dir = root.join("missing-claude");
    let home_file = root.join(".claude/projects/home-project/session-a.jsonl");
    write_file(
        &home_file,
        r#"{"timestamp":"2026-02-06T12:00:00Z","message":{"id":"msg_home","model":"anthropic.claude-3-5-sonnet-20241022","stop_reason":"end_turn","usage":{"input_tokens":900,"output_tokens":90,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}
"#,
    );

    let (ok, stdout, stderr) = run_ccstats(
        &[
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
        &[("HOME", &root), ("CLAUDE_CONFIG_DIR", &missing_config_dir)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    assert!(
        String::from_utf8_lossy(&stdout).contains("No Claude Code usage data found"),
        "stdout: {}",
        String::from_utf8_lossy(&stdout)
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn claude_daily_ignores_sidechains_and_subagent_logs() {
    let root = unique_temp_dir("claude-ignore-sidechains");
    let claude_file = root.join(".claude/projects/myproject/session-a.jsonl");
    let subagent_file = root.join(".claude/projects/myproject/subagents/agent-a.jsonl");

    write_file(
        &claude_file,
        r#"{"timestamp":"2026-02-06T12:00:00Z","message":{"id":"msg_1","model":"claude-3-5-sonnet-20241022","stop_reason":"end_turn","usage":{"input_tokens":100,"output_tokens":50,"cache_creation_input_tokens":10,"cache_read_input_tokens":20}}}
{"timestamp":"2026-02-06T12:30:00Z","isSidechain":true,"message":{"id":"msg_2","model":"gpt-5.3-codex","stop_reason":"end_turn","usage":{"input_tokens":500,"output_tokens":200,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}
"#,
    );
    write_file(
        &subagent_file,
        r#"{"timestamp":"2026-02-06T14:00:00Z","isSidechain":true,"message":{"id":"msg_sub","model":"gpt-5.2-codex","stop_reason":"end_turn","usage":{"input_tokens":700,"output_tokens":300,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}
"#,
    );

    let (ok, stdout, stderr) = run_ccstats(
        &[
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
        &[("HOME", &root)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));

    let json: Value = serde_json::from_slice(&stdout).expect("json");
    let arr = json.as_array().expect("array output");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["date"].as_str(), Some("2026-02-06"));
    assert_eq!(arr[0]["total_tokens"].as_i64(), Some(180));
    let models = arr[0]["models"].as_array().expect("models array");
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].as_str(), Some("3-5-sonnet"));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn claude_daily_csv_outputs_correct_format() {
    let root = unique_temp_dir("claude-csv-daily");
    let claude_file = root.join(".claude/projects/myproject/session-a.jsonl");
    write_file(
        &claude_file,
        r#"{"timestamp":"2026-02-06T12:00:00Z","message":{"id":"msg_1","model":"claude-3-5-sonnet-20241022","stop_reason":"end_turn","usage":{"input_tokens":100,"output_tokens":50,"cache_creation_input_tokens":10,"cache_read_input_tokens":20}}}
"#,
    );

    let (ok, stdout, stderr) = run_ccstats(
        &[
            "daily",
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
        &[("HOME", &root)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));

    let output = String::from_utf8(stdout).expect("utf8");
    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(lines.len(), 2, "header + 1 data row");
    assert_eq!(
        lines[0],
        "date,input_tokens,output_tokens,reasoning_tokens,cache_creation_tokens,cache_read_tokens,total_tokens"
    );
    // input=100, output=50, reasoning=0, cache_creation=10, cache_read=20, total=180
    assert_eq!(lines[1], "2026-02-06,100,50,0,10,20,180");

    let _ = fs::remove_dir_all(root);
}

#[test]
fn claude_session_csv_outputs_correct_format() {
    let root = unique_temp_dir("claude-csv-session");
    let session_a = root.join(".claude/projects/myapp/session-a.jsonl");
    let session_b = root.join(".claude/projects/myapp/session-b.jsonl");

    write_file(
        &session_a,
        r#"{"timestamp":"2026-02-06T10:00:00Z","message":{"id":"msg_1","model":"claude-3-5-sonnet-20241022","stop_reason":"end_turn","usage":{"input_tokens":100,"output_tokens":50,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}
"#,
    );
    write_file(
        &session_b,
        r#"{"timestamp":"2026-02-06T11:00:00Z","message":{"id":"msg_2","model":"claude-3-5-sonnet-20241022","stop_reason":"end_turn","usage":{"input_tokens":200,"output_tokens":80,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}
"#,
    );

    let (ok, stdout, stderr) = run_ccstats(
        &[
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
        &[("HOME", &root)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));

    let output = String::from_utf8(stdout).expect("utf8");
    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(lines.len(), 3, "header + 2 sessions");
    assert_eq!(
        lines[0],
        "session_id,project_path,first_timestamp,last_timestamp,input_tokens,output_tokens,reasoning_tokens,cache_creation_tokens,cache_read_tokens,total_tokens"
    );
    // Sessions sorted by last_timestamp asc: session-a (10:00) then session-b (11:00)
    assert!(lines[1].starts_with("session-a,"));
    assert!(lines[1].ends_with(",100,50,0,0,0,150"));
    assert!(lines[2].starts_with("session-b,"));
    assert!(lines[2].ends_with(",200,80,0,0,0,280"));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn claude_project_csv_outputs_correct_format() {
    let root = unique_temp_dir("claude-csv-project");
    let session_a = root.join(".claude/projects/myapp/session-a.jsonl");
    let session_b = root.join(".claude/projects/other-project/session-b.jsonl");

    write_file(
        &session_a,
        r#"{"timestamp":"2026-02-06T10:00:00Z","message":{"id":"msg_1","model":"claude-3-5-sonnet-20241022","stop_reason":"end_turn","usage":{"input_tokens":100,"output_tokens":50,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}
"#,
    );
    write_file(
        &session_b,
        r#"{"timestamp":"2026-02-06T11:00:00Z","message":{"id":"msg_2","model":"claude-3-5-sonnet-20241022","stop_reason":"end_turn","usage":{"input_tokens":200,"output_tokens":80,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}
"#,
    );

    let (ok, stdout, stderr) = run_ccstats(
        &[
            "project",
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
        &[("HOME", &root)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));

    let output = String::from_utf8(stdout).expect("utf8");
    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(lines.len(), 3, "header + 2 projects");
    assert_eq!(
        lines[0],
        "project_name,project_path,sessions,input_tokens,output_tokens,total_tokens"
    );
    // With --no-cost, all costs are 0.0 so order is undefined — find by name
    let myapp_line = lines
        .iter()
        .find(|l| l.starts_with("myapp,"))
        .expect("myapp row");
    assert!(myapp_line.ends_with(",1,100,50,150"));
    let other_line = lines
        .iter()
        .find(|l| l.starts_with("other-project,"))
        .expect("other-project row");
    assert!(other_line.ends_with(",1,200,80,280"));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn claude_blocks_csv_outputs_correct_format() {
    let root = unique_temp_dir("claude-csv-blocks");
    let session = root.join(".claude/projects/myapp/session-blocks.jsonl");

    // Entry at 10:00 UTC → block 10:00-15:00
    // Entry at 15:00 UTC → block 15:00-20:00
    write_file(
        &session,
        r#"{"timestamp":"2026-02-06T10:00:00Z","message":{"id":"msg_a","model":"claude-3-5-sonnet-20241022","stop_reason":"end_turn","usage":{"input_tokens":100,"output_tokens":50,"cache_creation_input_tokens":10,"cache_read_input_tokens":20}}}
{"timestamp":"2026-02-06T15:00:00Z","message":{"id":"msg_b","model":"claude-3-5-sonnet-20241022","stop_reason":"end_turn","usage":{"input_tokens":300,"output_tokens":150,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}
"#,
    );

    let (ok, stdout, stderr) = run_ccstats(
        &[
            "blocks",
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
        &[("HOME", &root)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));

    let output = String::from_utf8(stdout).expect("utf8");
    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(lines.len(), 3, "header + 2 blocks");
    assert_eq!(
        lines[0],
        "block_start,block_end,input_tokens,output_tokens,cache_creation_tokens,cache_read_tokens,total_tokens"
    );
    // Block 1: 10:00-15:00, input=100, output=50, cache_creation=10, cache_read=20, total=180
    assert!(lines[1].contains("10:00"));
    assert!(lines[1].ends_with(",100,50,10,20,180"));
    // Block 2: 15:00-20:00, input=300, output=150, cache_creation=0, cache_read=0, total=450
    assert!(lines[2].contains("15:00"));
    assert!(lines[2].ends_with(",300,150,0,0,450"));

    let _ = fs::remove_dir_all(root);
}
