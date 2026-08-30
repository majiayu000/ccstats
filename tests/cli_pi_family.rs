mod common;

use common::{run_ccstats, unique_temp_dir, write_file};
use serde_json::Value;
use std::fs;
use std::path::Path;

fn daily_json(source: &str, envs: &[(&str, &Path)]) -> Value {
    let (ok, stdout, stderr) = run_ccstats(
        &[
            "daily",
            "--source",
            source,
            "--json",
            "--offline",
            "--no-cost",
            "--timezone",
            "UTC",
            "--since",
            "2026-08-31",
            "--until",
            "2026-08-31",
        ],
        envs,
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    serde_json::from_slice(&stdout).expect("daily JSON")
}

#[test]
fn senpi_counts_all_authoritative_usage_carriers_and_deduplicates_fork() {
    let root = unique_temp_dir("senpi-e2e");
    let sessions = root.join("sessions");
    let original = sessions.join("--tmp-senpi--/original.jsonl");
    let copied = sessions.join("--tmp-senpi--/fork.jsonl");
    let header = r#"{"type":"session","version":3,"id":"senpi-original","timestamp":"2026-08-31T03:00:00Z","cwd":"/tmp/senpi"}"#;
    let entries = r#"
{"type":"model_change","id":"model","timestamp":"2026-08-31T03:00:01Z","provider":"anthropic","modelId":"claude-sonnet-4"}
{"type":"message","id":"assistant","timestamp":"2026-08-31T03:00:02Z","message":{"role":"assistant","provider":"anthropic","model":"claude-sonnet-4","usage":{"input":10,"output":2,"cacheRead":3,"cacheWrite":1,"cost":{"total":0.01}},"stopReason":"stop"}}
{"type":"compaction","id":"compaction","timestamp":"2026-08-31T03:00:03Z","usage":{"input":20,"output":4,"cacheRead":0,"cacheWrite":0,"cost":{"total":0.02}}}
{"type":"branch_summary","id":"branch","timestamp":"2026-08-31T03:00:04Z","usage":{"input":30,"output":6,"cacheRead":0,"cacheWrite":0,"cost":{"total":0.03}}}
{"type":"message","id":"tool-result","timestamp":"2026-08-31T03:00:05Z","message":{"role":"toolResult","usage":{"input":40,"output":8,"cacheRead":0,"cacheWrite":0,"cost":{"total":0.04}}}}
"#;
    let invalid_cost = r#"{"type":"message","id":"invalid-cost","timestamp":"2026-08-31T03:00:06Z","message":{"role":"assistant","provider":"anthropic","model":"claude-sonnet-4","usage":{"input":999,"output":99,"cacheRead":0,"cacheWrite":0,"cost":{"total":-1.0}},"stopReason":"stop"}}
"#;
    write_file(&original, &format!("{header}{entries}{invalid_cost}"));
    write_file(
        &copied,
        &format!(
            "{}{}",
            r#"{"type":"session","version":3,"id":"senpi-fork","timestamp":"2026-08-31T03:00:10Z","cwd":"/tmp/senpi","parentSession":"senpi-original"}"#,
            entries
        ),
    );

    let json = daily_json(
        "senpi",
        &[
            ("SENPI_CODING_AGENT_SESSION_DIR", &sessions),
            ("HOME", &root),
        ],
    );
    let row = &json.as_array().expect("array")[0];
    assert_eq!(row["input_tokens"].as_i64(), Some(100));
    assert_eq!(row["output_tokens"].as_i64(), Some(20));
    assert_eq!(row["cache_read_tokens"].as_i64(), Some(3));
    assert_eq!(row["cache_creation_tokens"].as_i64(), Some(1));
    assert_eq!(row["data_quality"]["valid_entries"].as_i64(), Some(4));
    assert_eq!(
        row["data_quality"]["dedup_skipped_entries"].as_i64(),
        Some(4)
    );
    assert_eq!(row["data_quality"]["parse_errors"].as_u64(), Some(1));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn kimchi_counts_child_transcript_without_parent_tool_result_rollup() {
    let root = unique_temp_dir("kimchi-e2e");
    let sessions = root.join(".config/kimchi/harness/sessions/project");
    let parent = sessions.join("parent.jsonl");
    let child = sessions.join("2026-08-31T03-00-03-000Z_kimchi-child.jsonl");
    let header_only_child = sessions.join("2026-08-31T03-00-06-000Z_header-only.jsonl");
    let invalid_child = sessions.join("2026-08-31T03-00-07-000Z_invalid-child.jsonl");
    let parent_line = serde_json::json!({
        "type": "message",
        "id": "child-rollup",
        "timestamp": "2026-08-31T03:00:02Z",
        "message": {
            "role": "toolResult",
            "details": {
                "modelName": "kimchi-dev/kimi-k2.6",
                "sessionFile": child.to_string_lossy(),
                "tokenUsage": {"input": 900, "output": 90, "cacheRead": 0, "cacheWrite": 0}
            }
        }
    });
    let remote_line = serde_json::json!({
        "type": "message",
        "id": "remote-rollup",
        "timestamp": "2026-08-31T03:00:05Z",
        "message": {
            "role": "toolResult",
            "details": {
                "modelName": "kimchi-dev/kimi-k2.6",
                "tokenUsage": {"input": 40, "output": 8, "cacheRead": 0, "cacheWrite": 0}
            }
        }
    });
    let header_only_line = serde_json::json!({
        "type": "message",
        "id": "header-only-rollup",
        "timestamp": "2026-08-31T03:00:06Z",
        "message": {
            "role": "toolResult",
            "details": {
                "modelName": "kimchi-dev/kimi-k2.6",
                "sessionFile": header_only_child.to_string_lossy(),
                "tokenUsage": {"input": 50, "output": 10, "cacheRead": 0, "cacheWrite": 0}
            }
        }
    });
    let invalid_child_line = serde_json::json!({
        "type": "message",
        "id": "invalid-child-rollup",
        "timestamp": "2026-08-31T03:00:07Z",
        "message": {
            "role": "toolResult",
            "details": {
                "modelName": "kimchi-dev/kimi-k2.6",
                "sessionFile": invalid_child.to_string_lossy(),
                "tokenUsage": {"input": 60, "output": 12, "cacheRead": 0, "cacheWrite": 0}
            }
        }
    });
    write_file(
        &parent,
        &format!(
            "{}\n{}\n{}\n{}\n{}\n{}\n",
            r#"{"type":"session","version":3,"id":"kimchi-parent","timestamp":"2026-08-31T03:00:00Z","cwd":"/tmp/kimchi"}"#,
            r#"{"type":"message","id":"parent-call","timestamp":"2026-08-31T03:00:01Z","message":{"role":"assistant","provider":"kimchi-dev","model":"kimi-k2.6","usage":{"input":10,"output":2,"cacheRead":0,"cacheWrite":0,"cost":{"total":0.01}},"stopReason":"stop"}}"#,
            parent_line,
            remote_line,
            header_only_line,
            invalid_child_line
        ),
    );
    write_file(
        &child,
        &format!(
            "{}\n{}\n",
            serde_json::json!({
                "type": "session",
                "version": 3,
                "id": "kimchi-child",
                "timestamp": "2026-08-31T03:00:03Z",
                "cwd": "/tmp/kimchi",
                "parentSession": parent.to_string_lossy()
            }),
            r#"{"type":"message","id":"child-call","timestamp":"2026-08-31T03:00:04Z","message":{"role":"assistant","provider":"kimchi-dev","model":"kimi-k2.6","usage":{"input":20,"output":4,"cacheRead":3,"cacheWrite":1,"cost":{"total":0.02}},"stopReason":"stop"}}"#
        ),
    );
    write_file(
        &header_only_child,
        &format!(
            "{}\n",
            serde_json::json!({
                "type": "session",
                "version": 3,
                "id": "kimchi-header-only",
                "timestamp": "2026-08-31T03:00:06Z",
                "cwd": "/tmp/kimchi",
                "parentSession": parent.to_string_lossy()
            })
        ),
    );
    write_file(
        &invalid_child,
        &format!(
            "{}\n{}\n",
            serde_json::json!({
                "type": "session",
                "version": 3,
                "id": "kimchi-invalid-child",
                "timestamp": "2026-08-31T03:00:07Z",
                "cwd": "/tmp/kimchi",
                "parentSession": parent.to_string_lossy()
            }),
            r#"{"type":"message","id":"invalid-child-call","timestamp":"not-a-time","message":{"role":"assistant","provider":"kimchi-dev","model":"kimi-k2.6","usage":{"input":999,"output":99,"cacheRead":0,"cacheWrite":0,"cost":{"total":-1.0}},"stopReason":"stop"}}"#
        ),
    );

    let json = daily_json("kimchi", &[("HOME", &root)]);
    let row = &json.as_array().expect("array")[0];
    assert_eq!(row["input_tokens"].as_i64(), Some(180));
    assert_eq!(row["output_tokens"].as_i64(), Some(36));
    assert_eq!(row["cache_read_tokens"].as_i64(), Some(3));
    assert_eq!(row["cache_creation_tokens"].as_i64(), Some(1));
    assert_eq!(row["data_quality"]["valid_entries"].as_i64(), Some(5));
    assert_eq!(row["data_quality"]["parse_errors"].as_u64(), Some(1));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn sources_listing_exposes_senpi_and_kimchi() {
    let root = unique_temp_dir("pi-family-sources");
    let (ok, stdout, stderr) = run_ccstats(&["sources", "--json"], &[("HOME", &root)]);
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    let rows: Value = serde_json::from_slice(&stdout).expect("sources JSON");
    let rows = rows.as_array().expect("array");
    assert!(rows.iter().any(|row| row["name"] == "senpi"));
    assert!(rows.iter().any(|row| row["name"] == "kimchi"));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn senpi_discovers_project_local_and_tilde_agent_directories() {
    let root = unique_temp_dir("senpi-discovery");
    let project = root.join("project");
    let cwd = project.join("nested/worktree");
    fs::create_dir_all(&cwd).expect("create project cwd");
    let session = r#"{"type":"session","version":3,"id":"senpi-discovery","timestamp":"2026-08-31T03:00:00Z","cwd":"/tmp/senpi-discovery"}
{"type":"message","id":"usage","timestamp":"2026-08-31T03:00:01Z","message":{"role":"assistant","provider":"test","model":"verified-model","usage":{"input":12,"output":3,"cacheRead":2,"cacheWrite":1},"stopReason":"stop"}}
"#;
    write_file(
        &project.join(".senpi/agent/sessions/project/session.jsonl"),
        session,
    );
    let project_json = daily_json("senpi", &[("HOME", &root), ("CCSTATS_TEST_CWD", &cwd)]);
    let row = &project_json.as_array().expect("array")[0];
    assert_eq!(row["input_tokens"].as_i64(), Some(12));
    assert_eq!(row["output_tokens"].as_i64(), Some(3));

    write_file(
        &root.join("custom-agent/sessions/project/session.jsonl"),
        session,
    );
    let tilde_json = daily_json(
        "senpi",
        &[
            ("HOME", &root),
            ("SENPI_CODING_AGENT_DIR", Path::new("~/custom-agent")),
        ],
    );
    let row = &tilde_json.as_array().expect("array")[0];
    assert_eq!(row["input_tokens"].as_i64(), Some(12));
    assert_eq!(row["cache_read_tokens"].as_i64(), Some(2));

    write_file(
        &root.join("windows-style-agent/sessions/project/session.jsonl"),
        session,
    );
    let windows_tilde_json = daily_json(
        "senpi",
        &[
            ("HOME", &root),
            (
                "SENPI_CODING_AGENT_DIR",
                Path::new(r"~\windows-style-agent"),
            ),
        ],
    );
    let row = &windows_tilde_json.as_array().expect("array")[0];
    assert_eq!(row["input_tokens"].as_i64(), Some(12));

    write_file(
        &root.join(".senpi/agent/settings.jsonc"),
        "\u{feff}{\n  // Senpi prefers JSONC when both formats exist.\n  \"sessionDir\": \"~/settings-sessions\",\n}\n",
    );
    write_file(&root.join("settings-sessions/session.jsonl"), session);
    let settings_json = daily_json("senpi", &[("HOME", &root)]);
    let row = &settings_json.as_array().expect("array")[0];
    assert_eq!(row["input_tokens"].as_i64(), Some(12));
    assert_eq!(row["cache_creation_tokens"].as_i64(), Some(1));

    let reset_project = root.join("settings-reset-project/nested");
    fs::create_dir_all(&reset_project).expect("create settings reset project");
    write_file(
        &root.join("settings-reset-project/.senpi/settings.jsonc"),
        "{\n  // Project null resets the global sessionDir.\n  \"sessionDir\": null,\n}\n",
    );
    write_file(
        &root.join(".senpi/agent/sessions/project/session.jsonl"),
        &session.replace("\"input\":12", "\"input\":14"),
    );
    let reset_json = daily_json(
        "senpi",
        &[("HOME", &root), ("CCSTATS_TEST_CWD", &reset_project)],
    );
    let row = &reset_json.as_array().expect("array")[0];
    assert_eq!(row["input_tokens"].as_i64(), Some(14));
    let _ = fs::remove_dir_all(root);
}
