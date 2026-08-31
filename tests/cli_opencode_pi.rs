mod common;

use common::{run_ccstats, unique_temp_dir, write_file};
use rusqlite::{Connection, params};
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

fn create_opencode_db(path: &Path) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create OpenCode data dir");
    }
    let connection = Connection::open(path).expect("open fixture database");
    connection
        .execute_batch(
            "
            CREATE TABLE session (
                id TEXT PRIMARY KEY,
                directory TEXT NOT NULL,
                parent_id TEXT,
                title TEXT NOT NULL,
                time_created INTEGER NOT NULL,
                time_updated INTEGER NOT NULL
            );
            CREATE TABLE session_message (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                type TEXT NOT NULL,
                seq INTEGER NOT NULL,
                time_created INTEGER NOT NULL,
                time_updated INTEGER NOT NULL,
                data TEXT NOT NULL
            );
            CREATE TABLE message (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                time_created INTEGER NOT NULL,
                time_updated INTEGER NOT NULL,
                data TEXT NOT NULL
            );
            ",
        )
        .expect("create current OpenCode schema");
    connection
        .execute(
            "INSERT INTO session
             (id, directory, parent_id, title, time_created, time_updated)
             VALUES (?1, ?2, NULL, ?3, ?4, ?5)",
            params![
                "ses_e2e",
                "/tmp/opencode-project",
                "OpenCode E2E",
                1_788_145_445_000_i64,
                1_788_145_446_000_i64,
            ],
        )
        .expect("insert session");
    connection
        .execute(
            "INSERT INTO session_message
             (id, session_id, type, seq, time_created, time_updated, data)
             VALUES (?1, ?2, 'assistant', 1, ?3, ?4, ?5)",
            params![
                "msg_e2e",
                "ses_e2e",
                1_788_145_445_000_i64,
                1_788_145_446_000_i64,
                r#"{"agent":"build","model":{"id":"claude-sonnet-4","providerID":"anthropic"},"finish":"stop","cost":0.0123,"tokens":{"input":100,"output":20,"reasoning":5,"cache":{"read":30,"write":10}},"time":{"created":1788145445000,"completed":1788145446000}}"#,
            ],
        )
        .expect("insert assistant message");
    connection
        .execute(
            "INSERT INTO message
             (id, session_id, time_created, time_updated, data)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                "msg_e2e",
                "ses_e2e",
                1_788_145_445_000_i64,
                1_788_145_446_000_i64,
                r#"{"role":"assistant","agent":"build","modelID":"claude-sonnet-4","providerID":"anthropic","finish":"stop","cost":0.0123,"tokens":{"input":100,"output":20,"reasoning":5,"cache":{"read":30,"write":10}},"time":{"created":1788145445000,"completed":1788145446000}}"#,
            ],
        )
        .expect("insert mirrored v1 assistant message");
}

#[test]
fn opencode_sqlite_reaches_daily_cli_with_all_token_buckets() {
    let root = unique_temp_dir("opencode-e2e");
    let db = root.join("opencode.db");
    create_opencode_db(&db);

    let json = daily_json("opencode", &[("OPENCODE_DB", &db), ("HOME", &root)]);
    let rows = json.as_array().expect("array");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["date"].as_str(), Some("2026-08-31"));
    assert_eq!(rows[0]["input_tokens"].as_i64(), Some(100));
    assert_eq!(rows[0]["output_tokens"].as_i64(), Some(20));
    assert_eq!(rows[0]["reasoning_tokens"].as_i64(), Some(5));
    assert_eq!(rows[0]["cache_read_tokens"].as_i64(), Some(30));
    assert_eq!(rows[0]["cache_creation_tokens"].as_i64(), Some(10));
    assert_eq!(rows[0]["total_tokens"].as_i64(), Some(165));
    assert_eq!(rows[0]["data_quality"]["valid_entries"].as_i64(), Some(1));
    assert_eq!(
        rows[0]["data_quality"]["dedup_skipped_entries"].as_i64(),
        Some(1)
    );
    assert_eq!(rows[0]["data_quality"]["parse_errors"].as_u64(), Some(0));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn pi_jsonl_counts_assistant_and_summary_llm_calls_end_to_end() {
    let root = unique_temp_dir("pi-e2e");
    let sessions = root.join("pi-sessions");
    let file = sessions.join("--tmp-pi-project--/2026-08-31_session.jsonl");
    write_file(
        &file,
        r#"{"type":"session","version":3,"id":"pi-session","timestamp":"2026-08-31T03:00:00.000Z","cwd":"/tmp/pi-project"}
{"type":"model_change","id":"model-1","parentId":null,"timestamp":"2026-08-31T03:00:01.000Z","provider":"openai","modelId":"gpt-5"}
{"type":"message","id":"assistant-1","parentId":"model-1","timestamp":"2026-08-31T03:00:02.000Z","message":{"role":"assistant","provider":"anthropic","model":"claude-opus-4","usage":{"input":100,"output":20,"reasoning":7,"cacheRead":30,"cacheWrite":10,"totalTokens":160,"cost":{"input":0.1,"output":0.1,"cacheRead":0.02,"cacheWrite":0.03,"total":0.25}},"stopReason":"stop","timestamp":1788145202000}}
{"type":"compaction","id":"compact-1","parentId":"assistant-1","timestamp":"2026-08-31T03:01:00.000Z","summary":"summary","firstKeptEntryId":"assistant-1","tokensBefore":160,"usage":{"input":40,"output":5,"cacheRead":2,"cacheWrite":3,"totalTokens":50,"cost":{"input":0.02,"output":0.02,"cacheRead":0.005,"cacheWrite":0.005,"total":0.05}}}
{"type":"branch_summary","id":"branch-1","parentId":"compact-1","timestamp":"2026-08-31T03:02:00.000Z","fromId":"assistant-1","summary":"branch","usage":{"input":20,"output":4,"cacheRead":1,"cacheWrite":0,"totalTokens":25,"cost":{"input":0.01,"output":0.01,"cacheRead":0.005,"cacheWrite":0.0,"total":0.025}}}
"#,
    );
    let branched = sessions.join("--tmp-pi-project--/2026-08-31_branched.jsonl");
    fs::copy(&file, &branched).expect("copy Pi branch fixture");

    let json = daily_json(
        "pi",
        &[("PI_CODING_AGENT_SESSION_DIR", &sessions), ("HOME", &root)],
    );
    let rows = json.as_array().expect("array");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["input_tokens"].as_i64(), Some(160));
    assert_eq!(rows[0]["output_tokens"].as_i64(), Some(29));
    assert_eq!(rows[0]["reasoning_tokens"].as_i64(), Some(0));
    assert_eq!(rows[0]["cache_read_tokens"].as_i64(), Some(33));
    assert_eq!(rows[0]["cache_creation_tokens"].as_i64(), Some(13));
    assert_eq!(rows[0]["total_tokens"].as_i64(), Some(235));
    assert_eq!(rows[0]["data_quality"]["valid_entries"].as_i64(), Some(3));
    assert_eq!(
        rows[0]["data_quality"]["dedup_skipped_entries"].as_i64(),
        Some(3)
    );
    assert_eq!(rows[0]["data_quality"]["parse_errors"].as_u64(), Some(0));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn sources_listing_exposes_opencode_and_pi_capabilities() {
    let root = unique_temp_dir("opencode-pi-sources");
    let (ok, stdout, stderr) = run_ccstats(&["sources", "--json"], &[("HOME", &root)]);
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    let rows: Value = serde_json::from_slice(&stdout).expect("sources JSON");
    let rows = rows.as_array().expect("array");

    let opencode = rows
        .iter()
        .find(|row| row["name"].as_str() == Some("opencode"))
        .expect("OpenCode source");
    assert_eq!(
        opencode["capabilities"]["has_projects"].as_bool(),
        Some(true)
    );
    assert_eq!(
        opencode["capabilities"]["has_reasoning_tokens"].as_bool(),
        Some(true)
    );

    let pi = rows
        .iter()
        .find(|row| row["name"].as_str() == Some("pi"))
        .expect("Pi source");
    assert_eq!(pi["capabilities"]["has_projects"].as_bool(), Some(true));
    assert_eq!(
        pi["capabilities"]["has_reasoning_tokens"].as_bool(),
        Some(false)
    );

    let _ = fs::remove_dir_all(root);
}
