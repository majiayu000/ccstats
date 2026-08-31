mod common;

use common::{run_ccstats, unique_temp_dir, write_file};
use serde_json::Value;
use std::fs;
use std::path::Path;

fn report(command: &str, source: &str, envs: &[(&str, &Path)]) -> Value {
    let (ok, stdout, stderr) = run_ccstats(
        &[
            command,
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
    serde_json::from_slice(&stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON report: {error}; stdout: {}; stderr: {}",
            String::from_utf8_lossy(&stdout),
            String::from_utf8_lossy(&stderr)
        )
    })
}

#[test]
fn prime_fork_keeps_local_usage_when_ancestor_record_is_malformed() {
    let root = unique_temp_dir("prime-malformed-ancestor");
    let sessions = root.join("sessions");
    write_file(
        &sessions.join("parent.jsonl"),
        concat!(
            r#"{"type":"session","version":3,"id":"parent","cwd":"/project"}"#,
            "\n{malformed\n"
        ),
    );
    write_file(
        &sessions.join("fork.jsonl"),
        concat!(
            r#"{"type":"session","version":3,"id":"fork","cwd":"/project","parentSession":"parent.jsonl"}"#,
            "\n",
            r#"{"type":"message","id":"local-call","timestamp":"2026-08-31T03:00:00Z","message":{"role":"assistant","provider":"openai","model":"gpt-5","timestamp":1788145200000,"usage":{"input":3,"output":1,"cacheRead":0,"cacheWrite":0}}}"#,
            "\n"
        ),
    );

    let json = report(
        "daily",
        "prime",
        &[("PRIME_AGENT_SESSION_DIR", &sessions), ("HOME", &root)],
    );

    assert_eq!(
        json.as_array().unwrap()[0]["input_tokens"].as_i64(),
        Some(3)
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn gjc_keeps_task_rollup_when_linked_child_has_no_usage() {
    let root = unique_temp_dir("gjc-empty-child");
    let sessions = root.join("agent/sessions/project");
    write_file(
        &sessions.join("session.jsonl"),
        concat!(
            r#"{"type":"session","version":5,"id":"parent","cwd":"/project"}"#,
            "\n",
            r#"{"type":"message","id":"task","timestamp":"2026-08-31T03:00:00Z","message":{"role":"toolResult","toolName":"task","details":{"usage":{"input":5,"output":1,"cacheRead":0,"cacheWrite":0,"cost":{"total":0.05}},"results":[{"id":"child","usage":{"input":5,"output":1,"cacheRead":0,"cacheWrite":0,"cost":{"total":0.05}}}]}}}"#,
            "\n"
        ),
    );
    write_file(
        &sessions.join("session/child.jsonl"),
        concat!(
            r#"{"type":"session","version":5,"id":"child","cwd":"/worktree"}"#,
            "\n"
        ),
    );

    let json = report(
        "daily",
        "gjc",
        &[
            ("GJC_CODING_AGENT_DIR", &root.join("agent")),
            ("HOME", &root),
        ],
    );

    assert_eq!(
        json.as_array().unwrap()[0]["input_tokens"].as_i64(),
        Some(5)
    );
    let _ = fs::remove_dir_all(root);
}
