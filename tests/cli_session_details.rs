mod common;

use common::{run_ccstats, unique_temp_dir, write_file};
use serde_json::Value;
use std::{fs, path::Path};

fn details(root: &Path, source: &str, day: &str) -> Value {
    let (ok, out, err) = run_ccstats(
        &[
            "session",
            "--json",
            "--details",
            "--source",
            source,
            "--offline",
            "--timezone",
            "UTC",
            "--since",
            day,
            "--until",
            day,
        ],
        &[("HOME", root)],
    );
    assert!(ok, "{}", String::from_utf8_lossy(&err));
    serde_json::from_slice(&out).expect("versioned JSON even when empty")
}

#[test]
fn claude_details_preserve_prompt_exact_cwd_models_dedup_and_subagents() {
    let root = unique_temp_dir("session-details-claude");
    let file = root.join(".claude/projects/-work-with-hyphens/full-session-id.jsonl");
    write_file(
        &file,
        r#"{"type":"user","timestamp":"2026-09-23T22:00:00Z","cwd":"/work-with-hyphens","message":{"content":[{"type":"text","text":"/x-reply-eval original prompt"}]}}
{"type":"assistant","timestamp":"2026-09-24T10:00:00Z","message":{"id":"a","model":"claude-opus-4-6","content":[],"usage":{"input_tokens":10,"output_tokens":2}}}
{"type":"assistant","timestamp":"2026-09-24T10:00:00Z","message":{"id":"a","model":"claude-opus-4-6","content":[],"stop_reason":"end_turn","usage":{"input_tokens":10,"output_tokens":3}}}
{"type":"assistant","timestamp":"2026-09-24T11:00:00Z","message":{"id":"b","model":"unlisted-model-123","content":[],"stop_reason":"end_turn","usage":{"input_tokens":7,"output_tokens":1}}}
{"type":"assistant","timestamp":"2026-09-25T01:00:00Z","message":{"id":"c","model":"claude-opus-4-6","content":[],"stop_reason":"end_turn","usage":{"input_tokens":99,"output_tokens":5}}}
"#,
    );
    write_file(
        &root.join(".claude/projects/-work-with-hyphens/subagents/agent-a.jsonl"),
        r#"{"type":"user","timestamp":"2026-09-24T09:00:00Z","cwd":"/work-with-hyphens","message":{"content":"helper"}}
{"type":"assistant","timestamp":"2026-09-24T10:00:00Z","message":{"id":"sub","model":"claude-opus-4-6","content":[],"stop_reason":"end_turn","usage":{"input_tokens":1,"output_tokens":1}}}
"#,
    );
    let report = details(&root, "claude", "2026-09-24");
    assert_eq!(report["schema_version"], 1);
    assert_eq!(report["currency"], "USD");
    assert_eq!(report["parse_errors"], 0);
    let sessions = report["sessions"].as_array().unwrap();
    assert_eq!(sessions.len(), 2);
    let session = sessions
        .iter()
        .find(|s| s["session_id"] == "full-session-id")
        .unwrap();
    assert_eq!(
        session["first_user_prompt"],
        "/x-reply-eval original prompt"
    );
    assert_eq!(session["project_path"], "/work-with-hyphens");
    assert_eq!(session["is_subagent"], false);
    assert_eq!(session["requests"], 2);
    assert_eq!(session["first_timestamp"], "2026-09-24T10:00:00Z");
    assert_eq!(session["breakdown"][0]["model"], "opus-4-6");
    assert_eq!(session["breakdown"][0]["input_tokens"], 10);
    assert_eq!(session["breakdown"][0]["output_tokens"], 3);
    assert!(session["breakdown"][0]["cost"].as_f64().unwrap() > 0.0);
    assert!(session["breakdown"][1]["cost"].is_null());
    assert_eq!(session["breakdown"][1]["pricing_source"], "unknown");
    assert!(sessions.iter().any(|s| s["is_subagent"] == true));
    let (ok, plain, _) = run_ccstats(
        &["session", "--json", "--source", "claude", "--offline"],
        &[("HOME", &root)],
    );
    assert!(ok);
    let plain: Value = serde_json::from_slice(&plain).unwrap();
    assert!(plain.is_array());
    assert!(plain[0].get("first_user_prompt").is_none());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn codex_details_use_usage_day_exclusive_cache_and_reasoning() {
    let root = unique_temp_dir("session-details-codex");
    write_file(
        &root.join(".codex/sessions/rollout.jsonl"),
        r#"{"type":"session_meta","timestamp":"2026-09-23T20:00:00Z","payload":{"id":"full-codex-id","cwd":"/work","source":"exec"}}
{"type":"response_item","timestamp":"2026-09-23T20:00:01Z","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"/x-post"}]}}
{"type":"turn_context","timestamp":"2026-09-23T20:00:01Z","payload":{"model":"gpt-5.4"}}
{"type":"event_msg","timestamp":"2026-09-23T20:01:00Z","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":100,"cached_input_tokens":20,"output_tokens":30,"reasoning_output_tokens":10}}}}
{"type":"event_msg","timestamp":"2026-09-24T00:01:00Z","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":160,"cached_input_tokens":30,"output_tokens":50,"reasoning_output_tokens":15}}}}
"#,
    );
    let report = details(&root, "codex", "2026-09-24");
    assert_eq!(report["parse_errors"], 0);
    assert_eq!(report["sessions"].as_array().unwrap().len(), 1);
    let session = &report["sessions"][0];
    assert_eq!(session["session_id"], "full-codex-id");
    assert_eq!(session["project_path"], "/work");
    assert_eq!(session["first_user_prompt"], "/x-post");
    assert_eq!(session["requests"], 1);
    assert_eq!(session["breakdown"][0]["input_tokens"], 50);
    assert_eq!(session["breakdown"][0]["cache_read_tokens"], 10);
    assert_eq!(session["breakdown"][0]["output_tokens"], 15);
    assert_eq!(session["breakdown"][0]["reasoning_tokens"], 5);
    let empty = details(&root, "codex", "2026-09-25");
    assert_eq!(empty["sessions"], serde_json::json!([]));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn details_expose_malformed_records_and_validate_command() {
    let root = unique_temp_dir("session-details-errors");
    write_file(
        &root.join(".claude/projects/project/broken.jsonl"),
        "{\"private_prompt\":not-json}\n",
    );
    let report = details(&root, "claude", "2026-09-24");
    assert!(report["parse_errors"].as_u64().unwrap() > 0);
    assert!(!report.to_string().contains("private_prompt"));
    for args in [
        vec!["daily", "--json", "--details", "--source", "claude"],
        vec!["session", "--details", "--source", "claude"],
        vec!["session", "--json", "--details", "--source", "all"],
    ] {
        let (ok, _, error) = run_ccstats(&args, &[("HOME", &root)]);
        assert!(!ok);
        assert!(String::from_utf8_lossy(&error).contains("--details requires"));
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn details_reject_last_sample_without_cumulative_ledger() {
    let root = unique_temp_dir("session-details-last-only");
    write_file(
        &root.join(".codex/sessions/rollout.jsonl"),
        r#"{"type":"session_meta","timestamp":"2026-09-24T10:00:00Z","payload":{"id":"last-only","cwd":"/work","source":"exec"}}
{"type":"event_msg","timestamp":"2026-09-24T10:00:01Z","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":100,"cached_input_tokens":20,"output_tokens":5}}}}
"#,
    );
    let report = details(&root, "codex", "2026-09-24");
    assert_eq!(report["parse_errors"], 1);
    assert_eq!(report["sessions"], serde_json::json!([]));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn details_do_not_invent_a_model_when_codex_omits_it() {
    let root = unique_temp_dir("session-details-no-model");
    write_file(
        &root.join(".codex/sessions/rollout.jsonl"),
        r#"{"type":"session_meta","timestamp":"2026-09-24T10:00:00Z","payload":{"id":"no-model","cwd":"/work","source":"exec"}}
{"type":"response_item","timestamp":"2026-09-24T10:00:00Z","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"task"}]}}
{"type":"event_msg","timestamp":"2026-09-24T10:00:01Z","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":100,"cached_input_tokens":20,"output_tokens":5}}}}
"#,
    );
    let report = details(&root, "codex", "2026-09-24");
    assert_eq!(report["parse_errors"], 0);
    assert_eq!(
        report["sessions"][0]["breakdown"][0]["model"],
        "unknown-model"
    );
    assert!(report["sessions"][0]["breakdown"][0]["cost"].is_null());
    assert_eq!(
        report["sessions"][0]["breakdown"][0]["pricing_source"],
        "unknown"
    );
    fs::remove_dir_all(root).unwrap();
}
