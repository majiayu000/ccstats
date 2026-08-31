mod common;

use common::{run_ccstats, unique_temp_dir, write_file};
use rusqlite::{Connection, params};
use serde_json::Value;
use std::fs;
use std::path::Path;

fn create_database(path: &Path) -> Connection {
    write_file(
        &path
            .parent()
            .expect("database parent")
            .join(".fixture-root"),
        "",
    );
    let connection = Connection::open(path).expect("open database");
    connection
        .execute_batch(
            r#"
            CREATE TABLE chat_threads (
                id TEXT PRIMARY KEY,
                model_id TEXT,
                pair_id TEXT,
                project_id TEXT,
                created_at INTEGER NOT NULL,
                forked_from_thread_id TEXT
            );
            CREATE TABLE chat_projects (
                id TEXT PRIMARY KEY,
                root_path TEXT
            );
            CREATE TABLE chat_messages (
                id TEXT PRIMARY KEY,
                thread_id TEXT NOT NULL,
                role TEXT NOT NULL,
                metadata_json TEXT,
                created_at INTEGER NOT NULL
            );
            CREATE TABLE api_usage_events (
                id TEXT PRIMARY KEY,
                subject TEXT NOT NULL,
                endpoint TEXT NOT NULL,
                model TEXT NOT NULL,
                status TEXT NOT NULL,
                prompt_tokens INTEGER NOT NULL,
                completion_tokens INTEGER NOT NULL,
                total_tokens INTEGER NOT NULL,
                created_at INTEGER NOT NULL
            );
            CREATE TABLE training_metrics (
                id INTEGER PRIMARY KEY,
                num_tokens INTEGER
            );
            "#,
        )
        .expect("create schema");
    connection
}

fn daily_json(studio_home: &Path) -> Value {
    let (ok, stdout, stderr) = run_ccstats(
        &[
            "daily",
            "--source",
            "unsloth",
            "--json",
            "--offline",
            "--timezone",
            "UTC",
            "--since",
            "2026-08-31",
            "--until",
            "2026-08-31",
        ],
        &[("STUDIO_HOME", studio_home)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    serde_json::from_slice(&stdout).expect("daily JSON")
}

#[test]
fn unsloth_reads_chat_and_api_without_fork_double_counting() {
    let root = unique_temp_dir("unsloth-e2e");
    let studio = root.join("studio");
    let connection = create_database(&studio.join("studio.db"));
    for (id, created_at, source) in [
        ("original", 1_788_145_000_000_i64, None),
        ("fork", 1_788_145_500_000_i64, Some("original")),
        ("local", 1_788_145_200_000_i64, None),
    ] {
        connection
            .execute(
                "INSERT INTO chat_threads (id, model_id, pair_id, project_id, created_at, forked_from_thread_id) VALUES (?1, 'stale/thread-model', NULL, NULL, ?2, ?3)",
                params![id, created_at, source],
            )
            .expect("insert thread");
    }
    let routed = r#"{"contextUsage":{"promptTokens":100,"completionTokens":40,"totalTokens":140,"cachedTokens":30,"cacheWriteTokens":10,"modelId":"openrouter/auto"},"responseDetails":{"responseModelId":"anthropic/claude-sonnet-4"}}"#;
    for (id, thread) in [("message-original", "original"), ("message-copy", "fork")] {
        connection
            .execute(
                "INSERT INTO chat_messages (id, thread_id, role, metadata_json, created_at) VALUES (?1, ?2, 'assistant', ?3, 1788145200000)",
                params![id, thread, routed],
            )
            .expect("insert routed message");
    }
    connection
        .execute(
            "INSERT INTO chat_messages (id, thread_id, role, metadata_json, created_at) VALUES ('local-response', 'local', 'assistant', ?1, 1788145260000)",
            [r#"{"serverTimings":{"prompt_n":50,"predicted_n":15,"cache_n":10},"timing":{"tokenCount":99},"responseDetails":{"responseModelId":"unsloth/local-model"}}"#],
        )
        .expect("insert local response");
    connection
        .execute(
            "INSERT INTO api_usage_events (id, subject, endpoint, model, status, prompt_tokens, completion_tokens, total_tokens, created_at) VALUES ('request-1', 'private-user', '/v1/chat/completions', 'unsloth/api-model', 'completed', 20, 7, 30, 1788145320000)",
            [],
        )
        .expect("insert API receipt");
    connection
        .execute(
            "INSERT INTO training_metrics (id, num_tokens) VALUES (1, 999999)",
            [],
        )
        .expect("insert excluded training metric");
    drop(connection);

    let json = daily_json(&studio);
    let row = &json.as_array().expect("array")[0];
    assert_eq!(row["input_tokens"].as_i64(), Some(120));
    assert_eq!(row["output_tokens"].as_i64(), Some(62));
    assert_eq!(row["reasoning_tokens"].as_i64(), Some(0));
    assert_eq!(row["cache_read_tokens"].as_i64(), Some(40));
    assert_eq!(row["cache_creation_tokens"].as_i64(), Some(10));
    assert_eq!(row["total_tokens"].as_i64(), Some(235));
    assert!(row["cost"].as_f64().is_some_and(|cost| cost > 0.0));
    assert_eq!(row["data_quality"]["valid_entries"].as_i64(), Some(3));
    assert_eq!(row["data_quality"]["parse_errors"].as_u64(), Some(0));

    let (ok, stdout, stderr) = run_ccstats(
        &[
            "top",
            "--source",
            "unsloth",
            "--dim",
            "model",
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
        &[("STUDIO_HOME", &studio)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    let top: Value = serde_json::from_slice(&stdout).expect("top JSON");
    let models = top["entries"].as_array().expect("models");
    assert!(
        models
            .iter()
            .any(|model| model["name"] == "anthropic/claude-sonnet-4")
    );
    assert!(
        models
            .iter()
            .all(|model| model["name"] != "stale/thread-model")
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn unsloth_elects_one_fork_keeper_after_original_is_deleted() {
    let root = unique_temp_dir("unsloth-fork-keeper");
    let studio = root.join("studio");
    let connection = create_database(&studio.join("studio.db"));
    for id in ["fork-b", "fork-a"] {
        connection
            .execute(
                "INSERT INTO chat_threads (id, model_id, pair_id, project_id, created_at, forked_from_thread_id) VALUES (?1, NULL, NULL, NULL, 1788145300000, 'deleted-source')",
                [id],
            )
            .expect("insert fork");
        connection
            .execute(
                "INSERT INTO chat_messages (id, thread_id, role, metadata_json, created_at) VALUES (?1, ?2, 'assistant', ?3, 1788145200000)",
                params![format!("message-{id}"), id, r#"{"contextUsage":{"promptTokens":8,"completionTokens":2,"totalTokens":10,"modelId":"unsloth/fork-model"}}"#],
            )
            .expect("insert clone");
    }
    drop(connection);

    let json = daily_json(&studio);
    let row = &json.as_array().expect("array")[0];
    assert_eq!(row["total_tokens"].as_i64(), Some(10));
    assert_eq!(row["data_quality"]["valid_entries"].as_i64(), Some(1));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn unsloth_reports_bad_rows_and_keeps_later_receipts() {
    let root = unique_temp_dir("unsloth-errors");
    let studio = root.join("studio");
    let connection = create_database(&studio.join("studio.db"));
    connection
        .execute(
            "INSERT INTO chat_threads (id, model_id, pair_id, project_id, created_at, forked_from_thread_id) VALUES ('thread', NULL, NULL, NULL, 1788145000000, NULL)",
            [],
        )
        .expect("insert thread");
    for (id, metadata, created_at) in [
        ("bad-json", "not-json", 1_788_145_200_000_i64),
        (
            "bad-cache",
            r#"{"contextUsage":{"promptTokens":5,"completionTokens":2,"totalTokens":7,"cachedTokens":4,"cacheWriteTokens":2,"modelId":"bad/cache"}}"#,
            1_788_145_210_000_i64,
        ),
        (
            "bad-negative",
            r#"{"contextUsage":{"promptTokens":-1,"completionTokens":2,"totalTokens":7,"modelId":"bad/negative"}}"#,
            1_788_145_220_000_i64,
        ),
    ] {
        connection
            .execute(
                "INSERT INTO chat_messages (id, thread_id, role, metadata_json, created_at) VALUES (?1, 'thread', 'assistant', ?2, ?3)",
                params![id, metadata, created_at],
            )
            .expect("insert bad row");
    }
    connection
        .execute(
            "INSERT INTO chat_messages (id, thread_id, role, metadata_json, created_at) VALUES ('bad-sql-type', 'thread', 'assistant', ?1, 'not-an-integer')",
            [r#"{"contextUsage":{"promptTokens":999,"completionTokens":0,"totalTokens":999,"modelId":"bad/sql-type"}}"#],
        )
        .expect("insert wrong SQLite type");
    connection
        .execute(
            "INSERT INTO chat_messages (id, thread_id, role, metadata_json, created_at) VALUES ('healthy-chat', 'thread', 'assistant', ?1, 1788145250000)",
            [r#"{"contextUsage":{"promptTokens":2,"completionTokens":1,"totalTokens":3,"modelId":"unsloth/healthy"}}"#],
        )
        .expect("insert healthy chat row");
    connection
        .execute(
            "INSERT INTO api_usage_events (id, subject, endpoint, model, status, prompt_tokens, completion_tokens, total_tokens, created_at) VALUES ('good-request', 'subject', '/v1/responses', 'unsloth/good', 'error', 4, 1, 5, 1788145260000)",
            [],
        )
        .expect("insert good receipt");
    drop(connection);

    let json = daily_json(&studio);
    let row = &json.as_array().expect("array")[0];
    assert_eq!(row["total_tokens"].as_i64(), Some(8));
    assert_eq!(row["data_quality"]["valid_entries"].as_i64(), Some(2));
    assert_eq!(row["data_quality"]["parse_errors"].as_u64(), Some(4));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn unsloth_specific_home_wins_and_compare_pair_is_one_project_session() {
    let root = unique_temp_dir("unsloth-path-pair");
    let primary = root.join("primary");
    let alias = root.join("alias");
    let primary_connection = create_database(&primary.join("studio.db"));
    primary_connection
        .execute(
            "INSERT INTO chat_projects (id, root_path) VALUES ('project', '/tmp/pair-project')",
            [],
        )
        .expect("insert project");
    for (thread, total, created_at) in [
        ("left", 10_i64, 1_788_145_200_000_i64),
        ("right", 20_i64, 1_788_145_210_000_i64),
    ] {
        primary_connection
            .execute(
                "INSERT INTO chat_threads (id, model_id, pair_id, project_id, created_at, forked_from_thread_id) VALUES (?1, NULL, 'compare-pair', 'project', ?2, NULL)",
                params![thread, created_at - 1_000],
            )
            .expect("insert compare thread");
        primary_connection
            .execute(
                "INSERT INTO chat_messages (id, thread_id, role, metadata_json, created_at) VALUES (?1, ?2, 'assistant', ?3, ?4)",
                params![
                    format!("message-{thread}"),
                    thread,
                    format!(r#"{{"contextUsage":{{"promptTokens":{},"completionTokens":1,"totalTokens":{total},"modelId":"unsloth/compare"}}}}"#, total - 1),
                    created_at,
                ],
            )
            .expect("insert compare response");
    }
    drop(primary_connection);

    let alias_connection = create_database(&alias.join("studio.db"));
    alias_connection
        .execute(
            "INSERT INTO api_usage_events (id, subject, endpoint, model, status, prompt_tokens, completion_tokens, total_tokens, created_at) VALUES ('decoy', 'subject', '/v1/responses', 'unsloth/decoy', 'completed', 999, 0, 999, 1788145200000)",
            [],
        )
        .expect("insert alias decoy");
    drop(alias_connection);

    let (ok, stdout, stderr) = run_ccstats(
        &[
            "top",
            "--source",
            "unsloth",
            "--dim",
            "project",
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
        &[("UNSLOTH_STUDIO_HOME", &primary), ("STUDIO_HOME", &alias)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    let top: Value = serde_json::from_slice(&stdout).expect("project top JSON");
    let project = top["entries"]
        .as_array()
        .expect("project entries")
        .iter()
        .find(|entry| entry["name"] == "pair-project")
        .expect("pair project");
    assert_eq!(project["count"].as_i64(), Some(1));
    assert_eq!(project["total_tokens"].as_i64(), Some(30));
    assert!(
        top["entries"]
            .as_array()
            .expect("project entries")
            .iter()
            .all(|entry| entry["total_tokens"].as_i64() != Some(999))
    );
    let _ = fs::remove_dir_all(root);
}
