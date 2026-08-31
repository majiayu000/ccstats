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

fn project_json(source: &str, envs: &[(&str, &Path)]) -> Value {
    let (ok, stdout, stderr) = run_ccstats(
        &[
            "project",
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
    serde_json::from_slice(&stdout).expect("project JSON")
}

#[test]
fn gjc_uses_assistant_usage_message_time_and_fork_identity() {
    let root = unique_temp_dir("gjc-e2e");
    let sessions = root.join("gjc-agent/sessions/project");
    let original = sessions.join("original.jsonl");
    let fork = sessions.join("fork.jsonl");
    let other = sessions.join("other.jsonl");
    let child = sessions.join("original/child-one.jsonl");
    let grandchild = sessions.join("original/child-one/grandchild.jsonl");
    let invalid_version = sessions.join("invalid-version.jsonl");
    let assistant = r#"{"type":"message","id":"same-id","parentId":null,"timestamp":"2026-08-30T23:59:00Z","message":{"role":"assistant","provider":"openai","model":"gpt-5","timestamp":1788145200000,"usage":{"input":10,"output":7,"cacheRead":3,"cacheWrite":4,"totalTokens":24,"reasoningTokens":2,"cttl":{"ephemeral1h":1},"cost":{"total":0.25}},"stopReason":"stop"}}"#;
    let task_rollup = r#"{"type":"message","id":"tool-rollup","parentId":"same-id","timestamp":"2026-08-31T03:01:00Z","message":{"role":"toolResult","toolName":"task","timestamp":1788145260000,"details":{"usage":{"input":50,"output":10,"cacheRead":5,"cacheWrite":1,"totalTokens":66,"cost":{"total":0.1}},"results":[{"id":"child-one","usage":{"input":20,"output":4,"cacheRead":2,"cacheWrite":0,"cost":{"total":0.04}}},{"id":"missing-child","usage":{"input":30,"output":6,"cacheRead":3,"cacheWrite":1,"cost":{"total":0.06}}}]}}}"#;
    let ignored_rollups = r#"
{task_rollup}
{"type":"compaction","id":"summary-rollup","parentId":"tool-rollup","timestamp":"2026-08-31T03:02:00Z","usage":{"input":80,"output":8,"cacheRead":0,"cacheWrite":0,"totalTokens":88}}
{"type":"header_patch","patch":{"cwd":"/tmp/gjc-lineage-patched"}}
"#
    .replacen("{task_rollup}", task_rollup, 1);
    write_file(
        &original,
        &format!(
            "{}\n{assistant}{ignored_rollups}",
            r#"{"type":"session","version":5,"id":"gjc-original","timestamp":"2026-08-31T03:00:00Z","cwd":"/tmp/gjc"}"#
        ),
    );
    write_file(
        &fork,
        &format!(
            "{}\n{assistant}\n{task_rollup}\n{}\n",
            r#"{"type":"session","version":5,"id":"gjc-fork","timestamp":"2026-08-31T03:03:00Z","cwd":"/tmp/gjc-fork","parentSession":"gjc-original"}"#,
            r#"{"type":"message","id":"fork-only","timestamp":"2026-08-31T03:03:01Z","message":{"role":"assistant","provider":"openai","model":"gpt-5","timestamp":1788145381000,"usage":{"input":3,"output":1,"cacheRead":0,"cacheWrite":0,"cost":{"total":0.03}},"stopReason":"stop"}}"#
        ),
    );
    write_file(
        &other,
        &format!(
            "{}\n{}\n{}\n{}\n",
            r#"{"type":"session","version":5,"id":"gjc-other","timestamp":"2026-08-31T04:00:00Z","cwd":"/tmp/other"}"#,
            r#"{"type":"message","id":"same-id","parentId":null,"timestamp":"2026-08-31T04:00:01Z","message":{"role":"assistant","provider":"openai","model":"gpt-5","timestamp":1788145200000,"usage":{"input":1,"output":1,"cacheRead":0,"cacheWrite":0,"totalTokens":2,"cost":{"total":0.01}},"stopReason":"stop"}}"#,
            r#"{"type":"entry_patch","entryId":"same-id","patch":{"message":{"role":"assistant","provider":"openai","model":"gpt-5","timestamp":1788145200000,"usage":{"input":2,"output":2,"cacheRead":0,"cacheWrite":0,"totalTokens":4,"cost":{"total":0.02}},"stopReason":"stop"}}}"#,
            r#"{"type":"header_patch","patch":{"cwd":"/tmp/gjc-patched"}}"#
        ),
    );
    write_file(
        &child,
        &format!(
            "{}\n{}\n{}\n",
            r#"{"type":"session","version":5,"id":"gjc-child","timestamp":"2026-08-31T03:00:30Z","cwd":"/tmp/gjc-worktree"}"#,
            r#"{"type":"message","id":"child-call","parentId":null,"timestamp":"2026-08-31T03:00:31Z","message":{"role":"assistant","provider":"anthropic","model":"claude-sonnet-4","timestamp":1788145231000,"usage":{"input":20,"output":4,"cacheRead":2,"cacheWrite":0,"totalTokens":26,"cost":{"total":0.04}},"stopReason":"stop"}}"#,
            r#"{"type":"message","id":"nested-task","timestamp":"2026-08-31T03:00:32Z","message":{"role":"toolResult","toolName":"task","timestamp":1788145232000,"details":{"usage":{"input":5,"output":1,"cacheRead":0,"cacheWrite":0,"cost":{"total":0.01}},"results":[{"id":"grandchild"}]}}}"#
        ),
    );
    write_file(
        &grandchild,
        &format!(
            "{}\n{}\n",
            r#"{"type":"session","version":5,"id":"gjc-grandchild","timestamp":"2026-08-31T03:00:33Z","cwd":"/tmp/gjc-grand-worktree"}"#,
            r#"{"type":"message","id":"grandchild-call","timestamp":"2026-08-31T03:00:33Z","message":{"role":"assistant","provider":"openai","model":"gpt-5","timestamp":1788145233000,"usage":{"input":5,"output":1,"cacheRead":0,"cacheWrite":0,"cost":{"total":0.01}},"stopReason":"stop"}}"#
        ),
    );
    write_file(
        &invalid_version,
        &format!(
            "{}\n{assistant}\n",
            r#"{"type":"session","version":4,"id":"old-gjc","timestamp":"2026-08-31T03:00:00Z","cwd":"/tmp/gjc"}"#
        ),
    );

    let json = daily_json(
        "gjc",
        &[
            ("GJC_CODING_AGENT_DIR", &root.join("gjc-agent")),
            ("HOME", &root),
        ],
    );
    let row = &json.as_array().expect("array")[0];
    assert_eq!(row["input_tokens"].as_i64(), Some(70));
    assert_eq!(row["output_tokens"].as_i64(), Some(19));
    assert_eq!(row["reasoning_tokens"].as_i64(), Some(2));
    assert_eq!(row["cache_read_tokens"].as_i64(), Some(8));
    assert_eq!(row["cache_creation_tokens"].as_i64(), Some(5));
    assert_eq!(row["data_quality"]["valid_entries"].as_i64(), Some(6));
    assert_eq!(
        row["data_quality"]["dedup_skipped_entries"].as_i64(),
        Some(2)
    );
    assert_eq!(row["data_quality"]["parse_errors"].as_u64(), Some(1));
    let projects = project_json(
        "gjc",
        &[
            ("GJC_CODING_AGENT_DIR", &root.join("gjc-agent")),
            ("HOME", &root),
        ],
    );
    assert!(projects.as_array().expect("array").iter().any(|row| {
        row["project_path"].as_str() == Some("/tmp/gjc-patched")
            && row["input_tokens"].as_i64() == Some(2)
    }));
    assert!(
        projects
            .as_array()
            .expect("array")
            .iter()
            .any(|row| row["project_path"].as_str() == Some("/tmp/gjc-fork")),
        "projects: {projects}"
    );
    assert!(!projects.as_array().expect("array").iter().any(|row| {
        matches!(
            row["project_path"].as_str(),
            Some("/tmp/gjc-worktree" | "/tmp/gjc-grand-worktree")
        )
    }));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn prime_counts_parent_and_child_transcripts_without_aggregate_rollup() {
    let root = unique_temp_dir("prime-e2e");
    let sessions = root.join("prime-sessions");
    let parent = sessions.join("parent.jsonl");
    let fork = sessions.join("fork.jsonl");
    let invalid_version = sessions.join("invalid-version.jsonl");
    let child = root.join("session-artifacts/prime-parent/sub-aaaa/child.jsonl");
    let second_child = root.join("session-artifacts/prime-parent/sub-bbbb/child.jsonl");
    let parent_assistant = r#"{"type":"message","id":"parent-call","parentId":null,"timestamp":"2026-08-31T05:00:01Z","message":{"role":"assistant","provider":"anthropic","model":"claude-sonnet-4","timestamp":1788152401000,"usage":{"input":40,"output":8,"cacheRead":4,"cacheWrite":1,"totalTokens":53,"cost":{"total":0.08}},"stopReason":"stop"}}"#;
    let attributions = concat!(
        r#"{"type":"child_usage_attributed","id":"rollup-one","parentId":"parent-call","timestamp":"2026-08-31T05:00:03Z","targetId":"parent-call","childUsage":{"input":20,"output":4,"cacheRead":2,"cacheWrite":0,"totalTokens":26,"cost":{"total":0.04}},"aggregateUsage":{"input":30,"output":6,"cacheRead":3,"cacheWrite":1,"totalTokens":40,"cost":{"total":0.06}}}"#,
        "\n",
        r#"{"type":"child_usage_attributed","id":"rollup-two","parentId":"parent-call","timestamp":"2026-08-31T05:00:04Z","targetId":"parent-call","childUsage":{"input":10,"output":2,"cacheRead":1,"cacheWrite":0,"totalTokens":13,"cost":{"total":0.02}},"aggregateUsage":{"input":40,"output":8,"cacheRead":4,"cacheWrite":1,"totalTokens":53,"cost":{"total":0.08}}}"#
    );
    write_file(
        &parent,
        &format!(
            "{}\n{parent_assistant}\n{attributions}\n{}\n{}\n",
            r#"{"type":"session","version":3,"id":"prime-parent","timestamp":"2026-08-31T05:00:00Z","cwd":"/tmp/prime"}"#,
            r#"{"type":"message","id":"bad-parent","timestamp":"2026-08-31T05:00:06Z","message":{"role":"assistant","provider":"openai","model":"gpt-5","timestamp":1788152406000,"usage":{"input":1,"output":1,"cacheRead":0,"cacheWrite":0,"cost":{"total":0.01}},"stopReason":"stop"}}"#,
            r#"{"type":"child_usage_attributed","id":"bad-rollup","timestamp":"2026-08-31T05:00:07Z","targetId":"bad-parent","childUsage":{"input":2,"output":1,"cacheRead":0,"cacheWrite":0,"cost":{"total":0.02}},"aggregateUsage":{"input":1,"output":1,"cacheRead":0,"cacheWrite":0,"cost":{"total":0.01}}}"#
        ),
    );
    write_file(
        &fork,
        &format!(
            "{}\n{parent_assistant}\n",
            r#"{"type":"session","version":3,"id":"prime-fork","timestamp":"2026-08-31T05:00:04Z","cwd":"/tmp/prime","parentSession":"parent.jsonl"}"#,
        ),
    );
    write_file(
        &child,
        &format!(
            "{}\n{}\n",
            r#"{"type":"session","version":3,"id":"prime-child","timestamp":"2026-08-31T05:00:02Z","cwd":"/tmp/prime","parentSession":"parent.jsonl","rlmDepth":1}"#,
            r#"{"type":"message","id":"child-call","parentId":null,"timestamp":"2026-08-31T05:00:02Z","message":{"role":"assistant","provider":"anthropic","model":"claude-sonnet-4","timestamp":1788152402000,"usage":{"input":20,"output":4,"cacheRead":2,"cacheWrite":0,"totalTokens":26,"cost":{"total":0.04}},"stopReason":"stop"}}"#
        ),
    );
    write_file(
        &second_child,
        &format!(
            "{}\n{}\n",
            r#"{"type":"session","version":3,"id":"prime-child-two","timestamp":"2026-08-31T05:00:05Z","cwd":"/tmp/prime","parentSession":"parent.jsonl","rlmDepth":1}"#,
            r#"{"type":"message","id":"child-call-two","parentId":null,"timestamp":"2026-08-31T05:00:05Z","message":{"role":"assistant","provider":"anthropic","model":"claude-sonnet-4","timestamp":1788152405000,"usage":{"input":10,"output":2,"cacheRead":1,"cacheWrite":0,"totalTokens":13,"cost":{"total":0.02}},"stopReason":"stop"}}"#
        ),
    );
    write_file(
        &invalid_version,
        &format!(
            "{}\n{parent_assistant}\n",
            r#"{"type":"session","version":4,"id":"prime-invalid","timestamp":"2026-08-31T05:00:00Z","cwd":"/tmp/prime"}"#
        ),
    );

    let json = daily_json(
        "prime",
        &[("PRIME_AGENT_SESSION_DIR", &sessions), ("HOME", &root)],
    );
    let row = &json.as_array().expect("array")[0];
    assert_eq!(row["input_tokens"].as_i64(), Some(40));
    assert_eq!(row["output_tokens"].as_i64(), Some(8));
    assert_eq!(row["cache_read_tokens"].as_i64(), Some(4));
    assert_eq!(row["cache_creation_tokens"].as_i64(), Some(1));
    assert_eq!(row["data_quality"]["valid_entries"].as_i64(), Some(3));
    assert_eq!(
        row["data_quality"]["dedup_skipped_entries"].as_i64(),
        Some(1)
    );
    assert_eq!(row["data_quality"]["parse_errors"].as_u64(), Some(2));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn omp_profile_counts_nested_transcripts_without_task_rollup() {
    let root = unique_temp_dir("omp-e2e");
    let xdg = root.join("xdg-data");
    let sessions = xdg.join("omp/profiles/work/sessions/project");
    let parent = sessions.join("parent.jsonl");
    let fork = sessions.join("fork.jsonl");
    let invalid_version = sessions.join("invalid-version.jsonl");
    let child = sessions.join("parent/task-child.jsonl");
    let grandchild = sessions.join("parent/task-child/grandchild.jsonl");
    let header_only = sessions.join("parent/header-only.jsonl");
    let parent_assistant = r#"{"type":"message","id":"omp-parent-call","parentId":null,"timestamp":"2026-08-31T06:00:01Z","message":{"role":"assistant","provider":"openai","model":"gpt-5","timestamp":1788156001000,"usage":{"input":10,"output":5,"cacheRead":1,"cacheWrite":2,"totalTokens":18,"reasoningTokens":2,"cttl":{"ephemeral1h":1},"orchestration":{"input":2,"output":1,"cacheRead":2},"cost":{"total":0.03}},"stopReason":"stop"}}"#;
    let task_rollup = r#"{"type":"message","id":"task-rollup","parentId":"omp-parent-call","timestamp":"2026-08-31T06:00:03Z","message":{"role":"toolResult","toolName":"task","timestamp":1788156003000,"details":{"usage":{"input":20,"output":4,"cacheRead":3,"cacheWrite":0,"totalTokens":27,"cost":{"total":0.04}},"results":[{"id":"task-child","usage":{"input":20,"output":4,"cacheRead":3,"cacheWrite":0,"totalTokens":27,"cost":{"total":0.04}}},{"id":"cancelled"}]}}}"#;
    write_file(
        &parent,
        &format!(
            "{}\n{parent_assistant}\n{task_rollup}\n{}\n{}\n",
            r#"{"type":"session","version":3,"id":"omp-parent","timestamp":"2026-08-31T06:00:00Z","cwd":"/tmp/omp"}"#,
            r#"{"type":"message","id":"missing-task","timestamp":"2026-08-31T06:00:07Z","message":{"role":"toolResult","toolName":"task","details":{"usage":{"input":1,"output":1,"cacheRead":0,"cacheWrite":0,"cost":{"total":0.01}},"results":[{"id":"missing-child","usage":{"input":1,"output":1,"cacheRead":0,"cacheWrite":0,"cost":{"total":0.01}}}]}}}"#,
            r#"{"type":"message","id":"empty-task","timestamp":"2026-08-31T06:00:08Z","message":{"role":"toolResult","toolName":"task","details":{"usage":{"input":1,"output":1,"cacheRead":0,"cacheWrite":0,"cost":{"total":0.01}},"results":[{"id":"header-only","usage":{"input":1,"output":1,"cacheRead":0,"cacheWrite":0,"cost":{"total":0.01}}}]}}}"#
        ),
    );
    write_file(
        &fork,
        &format!(
            "{}\n{parent_assistant}\n{task_rollup}\n{}\n",
            r#"{"type":"session","version":3,"id":"omp-fork","timestamp":"2026-08-31T06:00:04Z","cwd":"/tmp/omp","parentSession":"omp-parent"}"#,
            r#"{"type":"message","id":"invalid-orchestration","timestamp":"2026-08-31T06:00:06Z","message":{"role":"assistant","provider":"openai","model":"gpt-5","timestamp":1788156006000,"usage":{"input":-1,"output":1,"cacheRead":0,"cacheWrite":0,"reasoningTokens":0,"orchestration":{"input":2,"output":0,"cacheRead":0},"cost":{"total":0.01}},"stopReason":"stop"}}"#
        ),
    );
    write_file(
        &child,
        &format!(
            "{}\n{}\n{}\n",
            r#"{"type":"session","version":3,"id":"omp-child","timestamp":"2026-08-31T06:00:02Z","cwd":"/tmp/omp"}"#,
            r#"{"type":"message","id":"omp-child-call","parentId":null,"timestamp":"2026-08-31T06:00:02Z","message":{"role":"assistant","provider":"anthropic","model":"claude-sonnet-4","timestamp":1788156002000,"usage":{"input":20,"output":4,"cacheRead":3,"cacheWrite":0,"totalTokens":27,"cost":{"total":0.04}},"stopReason":"stop"}}"#,
            r#"{"type":"message","id":"nested-task-rollup","parentId":"omp-child-call","timestamp":"2026-08-31T06:00:04Z","message":{"role":"toolResult","toolName":"task","details":{"usage":{"input":5,"output":1,"cacheRead":0,"cacheWrite":0,"totalTokens":6,"cost":{"total":0.01}},"results":[{"id":"grandchild","usage":{"input":5,"output":1,"cacheRead":0,"cacheWrite":0,"totalTokens":6,"cost":{"total":0.01}}}]}}}"#
        ),
    );
    write_file(
        &grandchild,
        &format!(
            "{}\n{}\n",
            r#"{"type":"session","version":3,"id":"omp-grandchild","timestamp":"2026-08-31T06:00:05Z","cwd":"/tmp/omp"}"#,
            r#"{"type":"message","id":"omp-grandchild-call","parentId":null,"timestamp":"2026-08-31T06:00:05Z","message":{"role":"assistant","provider":"openai","model":"gpt-5","timestamp":1788156005000,"usage":{"input":5,"output":1,"cacheRead":0,"cacheWrite":0,"totalTokens":6,"cost":{"total":0.01}},"stopReason":"stop"}}"#
        ),
    );
    write_file(
        &header_only,
        r#"{"type":"session","version":3,"id":"omp-empty-child","timestamp":"2026-08-31T06:00:08Z","cwd":"/tmp/omp"}
"#,
    );
    write_file(
        &invalid_version,
        &format!(
            "{}\n{parent_assistant}\n",
            r#"{"type":"session","version":2,"id":"omp-invalid","timestamp":"2026-08-31T06:00:00Z","cwd":"/tmp/omp"}"#
        ),
    );

    let json = daily_json(
        "omp",
        &[
            ("HOME", &root),
            ("XDG_DATA_HOME", &xdg),
            ("OMP_PROFILE", Path::new("work")),
        ],
    );
    let row = &json.as_array().expect("array")[0];
    assert_eq!(row["input_tokens"].as_i64(), Some(37));
    assert_eq!(row["output_tokens"].as_i64(), Some(9));
    assert_eq!(row["reasoning_tokens"].as_i64(), Some(2));
    assert_eq!(row["cache_read_tokens"].as_i64(), Some(6));
    assert_eq!(row["cache_creation_tokens"].as_i64(), Some(2));
    assert_eq!(row["data_quality"]["valid_entries"].as_i64(), Some(3));
    assert_eq!(
        row["data_quality"]["dedup_skipped_entries"].as_i64(),
        Some(1)
    );
    assert_eq!(row["data_quality"]["parse_errors"].as_u64(), Some(4));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn gjc_preserves_an_explicit_zero_recorded_cost() {
    let root = unique_temp_dir("gjc-zero-cost");
    let sessions = root.join("gjc-agent/sessions/project");
    write_file(
        &sessions.join("zero.jsonl"),
        concat!(
            r#"{"type":"session","version":5,"id":"gjc-zero","timestamp":"2026-08-31T07:00:00Z","cwd":"/tmp/gjc"}"#,
            "\n",
            r#"{"type":"message","id":"zero-call","timestamp":"2026-08-31T07:00:01Z","message":{"role":"assistant","provider":"openai","model":"gpt-5","timestamp":1788159601000,"usage":{"input":1000,"output":100,"cacheRead":0,"cacheWrite":0,"cost":{"total":0}},"stopReason":"stop"}}"#,
            "\n"
        ),
    );
    write_file(
        &sessions.join("priced.jsonl"),
        concat!(
            r#"{"type":"session","version":5,"id":"gjc-priced","timestamp":"2026-08-31T07:00:00Z","cwd":"/tmp/gjc"}"#,
            "\n",
            r#"{"type":"message","id":"zero-call","timestamp":"2026-08-31T07:00:01Z","message":{"role":"assistant","provider":"openai","model":"gpt-5","timestamp":1788159601000,"usage":{"input":1000,"output":100,"cacheRead":0,"cacheWrite":0,"cost":{"total":0.25}},"stopReason":"stop"}}"#,
            "\n"
        ),
    );
    let (ok, stdout, stderr) = run_ccstats(
        &[
            "daily",
            "--source",
            "gjc",
            "--json",
            "--offline",
            "--timezone",
            "UTC",
            "--since",
            "2026-08-31",
            "--until",
            "2026-08-31",
        ],
        &[
            ("GJC_CODING_AGENT_DIR", &root.join("gjc-agent")),
            ("HOME", &root),
        ],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    let rows: Value = serde_json::from_slice(&stdout).expect("daily JSON");
    let row = &rows.as_array().expect("array")[0];
    assert_eq!(row["input_tokens"].as_i64(), Some(2000));
    assert_eq!(row["cost"].as_f64(), Some(0.25));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn sources_listing_exposes_gjc_prime_and_omp() {
    let root = unique_temp_dir("gjc-prime-omp-sources");
    let (ok, stdout, stderr) = run_ccstats(&["sources", "--json"], &[("HOME", &root)]);
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    let rows: Value = serde_json::from_slice(&stdout).expect("sources JSON");
    let rows = rows.as_array().expect("array");
    assert!(rows.iter().any(|row| row["name"] == "gjc"));
    assert!(rows.iter().any(|row| row["name"] == "prime"));
    assert!(rows.iter().any(|row| row["name"] == "omp"));
    let _ = fs::remove_dir_all(root);
}
