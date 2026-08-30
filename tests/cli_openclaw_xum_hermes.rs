mod common;

use common::{run_ccstats, unique_temp_dir, write_file};
use rusqlite::Connection;
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

fn top_models_json(source: &str, envs: &[(&str, &Path)]) -> Value {
    let (ok, stdout, stderr) = run_ccstats(
        &[
            "top",
            "--source",
            source,
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
        envs,
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    serde_json::from_slice(&stdout).expect("top models JSON")
}

fn daily_cost_json(source: &str, envs: &[(&str, &Path)]) -> Value {
    let (ok, stdout, stderr) = run_ccstats(
        &[
            "daily",
            "--source",
            source,
            "--json",
            "--offline",
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
    serde_json::from_slice(&stdout).expect("daily cost JSON")
}

fn write_openclaw_event_store(path: &Path, session: &str, call: &str, input: i64) {
    fs::create_dir_all(path.parent().expect("store parent")).expect("create store parent");
    let connection = Connection::open(path).expect("open OpenClaw event store");
    connection
        .execute_batch(
            r#"
            CREATE TABLE transcript_events (
                session_id TEXT NOT NULL, seq INTEGER NOT NULL,
                event_json TEXT NOT NULL, created_at INTEGER NOT NULL,
                PRIMARY KEY (session_id, seq)
            );
            CREATE TABLE session_transcript_archives (
                session_id TEXT NOT NULL, generation INTEGER NOT NULL,
                encoding TEXT NOT NULL, archive_blob BLOB NOT NULL,
                PRIMARY KEY (session_id, generation)
            );
            "#,
        )
        .expect("create OpenClaw event schema");
    let header = format!(
        r#"{{"type":"session","version":3,"id":"{session}","timestamp":"2026-08-31T01:04:00Z","cwd":"/tmp/configured"}}"#
    );
    let message = format!(
        r#"{{"type":"message","id":"{call}","timestamp":"2026-08-31T01:04:01Z","message":{{"role":"assistant","provider":"openai","model":"gpt-5","timestamp":1788138241000,"usage":{{"input":{input},"output":1,"cacheRead":0,"cacheWrite":0}}}}}}"#
    );
    connection
        .execute(
            "INSERT INTO transcript_events VALUES (?1, 1, ?2, 1788138240000), (?1, 2, ?3, 1788138241000)",
            rusqlite::params![session, header, message],
        )
        .expect("insert OpenClaw events");
}

#[test]
fn openclaw_reads_exact_calls_and_deduplicates_copied_entries() {
    let root = unique_temp_dir("openclaw-e2e");
    let sessions = root.join("agents/main/sessions");
    let header = r#"{"type":"session","version":3,"id":"oc-session","timestamp":"2026-08-31T01:00:00Z","cwd":"/tmp/openclaw"}"#;
    let model = r#"{"type":"model_change","id":"model-one","parentId":null,"timestamp":"2026-08-31T01:00:01Z","provider":"openai","modelId":"gpt-5"}"#;
    let billed = r#"{"type":"message","id":"call-one","parentId":"model-one","timestamp":"2026-08-31T01:00:02Z","message":{"role":"assistant","provider":"openai","model":"gpt-5","timestamp":1788138002000,"usage":{"input":10,"output":8,"cacheRead":3,"cacheWrite":2,"totalTokens":23,"cost":{"input":0.01,"output":0.02,"cacheRead":0.001,"cacheWrite":0.002,"total":0.033,"totalOrigin":"provider-billed"}},"stopReason":"stop"}}"#;
    let estimated = r#"{"type":"message","id":"call-two","parentId":"call-one","timestamp":"2026-08-31T01:00:03Z","message":{"role":"assistant","timestamp":1788138003000,"usage":{"input":5,"output":4,"cacheRead":1,"cacheWrite":0,"totalTokens":10,"cost":{"input":0.01,"output":0.02,"cacheRead":0,"cacheWrite":0,"total":9.99}},"stopReason":"stop"}}"#;
    write_file(
        &sessions.join("oc-session.jsonl"),
        &format!("{header}\n{model}\n{billed}\n{estimated}\n"),
    );
    write_file(
        &sessions.join("oc-fork.jsonl"),
        &format!(
            "{}\n{billed}\n",
            r#"{"type":"session","version":3,"id":"oc-fork","timestamp":"2026-08-31T01:01:00Z","cwd":"/tmp/openclaw-fork"}"#
        ),
    );
    write_file(
        &sessions.join("bad.jsonl"),
        concat!(
            r#"{"type":"session","version":3,"id":"oc-bad","timestamp":"2026-08-31T01:02:00Z","cwd":"/tmp/openclaw"}"#,
            "\n",
            r#"{"type":"message","id":"bad-call","timestamp":"2026-08-31T01:02:01Z","message":{"role":"assistant","provider":"openai","model":"gpt-5","timestamp":1788138121000,"usage":{"input":-1,"output":2,"cacheRead":0,"cacheWrite":0,"cost":{"total":0}},"stopReason":"stop"}}"#,
            "\n"
        ),
    );

    let json = daily_json(
        "openclaw",
        &[("OPENCLAW_STATE_DIR", &root), ("HOME", &root)],
    );
    let row = &json.as_array().expect("array")[0];
    assert_eq!(row["input_tokens"].as_i64(), Some(15));
    assert_eq!(row["output_tokens"].as_i64(), Some(12));
    assert_eq!(row["cache_read_tokens"].as_i64(), Some(4));
    assert_eq!(row["cache_creation_tokens"].as_i64(), Some(2));
    assert_eq!(row["data_quality"]["valid_entries"].as_i64(), Some(2));
    assert_eq!(
        row["data_quality"]["dedup_skipped_entries"].as_i64(),
        Some(1)
    );
    assert_eq!(row["data_quality"]["parse_errors"].as_u64(), Some(1));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn xum_suppresses_children_already_rolled_into_parent() {
    let root = unique_temp_dir("xum-e2e");
    let sessions = root.join("sessions");
    write_file(
        &sessions.join("parent/session-usage.json"),
        r#"{
          "version": 1,
          "byModel": {
            "anthropic:claude-sonnet-4": {
              "input": {"tokens": 30, "cost_usd": 0.03},
              "cached": {"tokens": 6, "cost_usd": 0.006},
              "cacheCreate": {"tokens": 2, "cost_usd": 0.002},
              "output": {"tokens": 12, "cost_usd": 0.12},
              "reasoning": {"tokens": 4, "cost_usd": 0.04}
            }
          },
          "lastRequest": {"model":"anthropic:claude-sonnet-4","usage":{},"timestamp":1788141600000},
          "rolledUpFrom": {"child":{"totalTokens":24,"rolledUpAtMs":1788141500000}}
        }"#,
    );
    write_file(
        &sessions.join("child/session-usage.json"),
        r#"{"version":1,"byModel":{"anthropic:claude-sonnet-4":{"input":{"tokens":20},"cached":{"tokens":2},"cacheCreate":{"tokens":0},"output":{"tokens":6},"reasoning":{"tokens":2}}},"lastRequest":{"model":"anthropic:claude-sonnet-4","usage":{},"timestamp":1788141500000}}"#,
    );
    write_file(
        &sessions.join("independent/session-usage.json"),
        r#"{"version":1,"byModel":{"openai:gpt-5":{"input":{"tokens":5},"cached":{"tokens":1},"cacheCreate":{"tokens":0},"output":{"tokens":3},"reasoning":{"tokens":1}}},"lastRequest":{"model":"openai:gpt-5","usage":{},"timestamp":1788141700000}}"#,
    );

    let json = daily_json("xum", &[("XUM_ROOT", &root), ("HOME", &root)]);
    let row = &json.as_array().expect("array")[0];
    assert_eq!(row["input_tokens"].as_i64(), Some(35));
    assert_eq!(row["output_tokens"].as_i64(), Some(15));
    assert_eq!(row["reasoning_tokens"].as_i64(), Some(5));
    assert_eq!(row["cache_read_tokens"].as_i64(), Some(7));
    assert_eq!(row["cache_creation_tokens"].as_i64(), Some(2));
    assert_eq!(row["data_quality"]["valid_entries"].as_i64(), Some(2));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn xum_costs_included_is_an_authoritative_zero() {
    let root = unique_temp_dir("xum-included-cost");
    write_file(
        &root.join("sessions/included/session-usage.json"),
        r#"{"version":1,"byModel":{"custom:unpriced-model":{"input":{"tokens":100},"cached":{"tokens":0},"cacheCreate":{"tokens":0},"output":{"tokens":10},"reasoning":{"tokens":0},"costsIncluded":true}},"lastRequest":{"model":"custom:unpriced-model","usage":{},"timestamp":1788141600000}}"#,
    );
    let json = daily_cost_json("xum", &[("XUM_ROOT", &root), ("HOME", &root)]);
    assert_eq!(
        json.as_array().expect("array")[0]["cost"].as_f64(),
        Some(0.0)
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn openclaw_uses_only_provider_billed_recorded_cost() {
    let root = unique_temp_dir("openclaw-billed-cost");
    write_file(
        &root.join("agents/main/sessions/cost.jsonl"),
        concat!(
            r#"{"type":"session","version":3,"id":"oc-cost","timestamp":"2026-08-31T01:00:00Z","cwd":"/tmp/openclaw"}"#,
            "\n",
            r#"{"type":"message","id":"cost-call","timestamp":"2026-08-31T01:00:01Z","message":{"role":"assistant","provider":"custom","model":"unpriced-model","timestamp":1788138001000,"usage":{"input":100,"output":10,"cacheRead":0,"cacheWrite":0,"totalTokens":110,"cost":{"input":0,"output":0,"cacheRead":0,"cacheWrite":0,"total":0.123,"totalOrigin":"provider-billed"}},"stopReason":"stop"}}"#,
            "\n"
        ),
    );
    let json = daily_cost_json(
        "openclaw",
        &[("OPENCLAW_STATE_DIR", &root), ("HOME", &root)],
    );
    assert_eq!(
        json.as_array().expect("array")[0]["cost"].as_f64(),
        Some(0.123)
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn openclaw_reads_current_sqlite_and_counted_zstd_archives() {
    let root = unique_temp_dir("openclaw-store-e2e");
    let state = root.join("state");
    let database = root.join("custom-store.sqlite");
    fs::create_dir_all(database.parent().expect("database parent")).expect("create database dir");
    let connection = Connection::open(&database).expect("open OpenClaw fixture");
    connection
        .execute_batch(
            r#"
            CREATE TABLE transcript_events (
                session_id TEXT NOT NULL, seq INTEGER NOT NULL,
                event_json TEXT NOT NULL, created_at INTEGER NOT NULL,
                PRIMARY KEY (session_id, seq)
            );
            CREATE TABLE session_transcript_archives (
                session_id TEXT NOT NULL, generation INTEGER NOT NULL,
                encoding TEXT NOT NULL, archive_blob BLOB NOT NULL,
                PRIMARY KEY (session_id, generation)
            );
            INSERT INTO transcript_events VALUES
            ('active', 1, '{"type":"session","version":3,"id":"active","timestamp":"2026-08-31T01:00:00Z","cwd":"/tmp/db"}', 1788138000000),
            ('active', 2, '{"type":"message","id":"db-call","timestamp":"2026-08-31T01:00:01Z","message":{"role":"assistant","provider":"openai","model":"gpt-5","timestamp":1788138001000,"usage":{"input":10,"output":4,"cacheRead":2,"cacheWrite":3,"cacheWrite1h":1}}}', 1788138001000);
            "#,
        )
        .expect("create OpenClaw schema");
    let archived = concat!(
        r#"{"type":"session","version":3,"id":"archived","timestamp":"2026-08-31T01:01:00Z","cwd":"/tmp/archive"}"#,
        "\n",
        r#"{"type":"message","id":"db-call","timestamp":"2026-08-31T01:01:01Z","message":{"role":"assistant","provider":"openai","model":"gpt-5","timestamp":1788138061000,"usage":{"input":10,"output":4,"cacheRead":2,"cacheWrite":3,"cacheWrite1h":1}}}"#,
        "\n",
        r#"{"type":"message","id":"archive-call","timestamp":"2026-08-31T01:01:02Z","message":{"role":"assistant","provider":"openai","model":"gpt-5","timestamp":1788138062000,"usage":{"input":5,"output":2,"cacheRead":1,"cacheWrite":0}}}"#,
        "\n"
    );
    let blob = zstd::stream::encode_all(archived.as_bytes(), 1).expect("compress DB archive");
    connection
        .execute(
            "INSERT INTO session_transcript_archives VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params!["archived", 1_i64, "zstd", blob],
        )
        .expect("insert archive");
    connection
        .execute(
            "INSERT INTO session_transcript_archives VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params!["broken", 2_i64, "unsupported", vec![1_u8, 2, 3]],
        )
        .expect("insert malformed archive");
    drop(connection);
    write_openclaw_event_store(
        &root.join("custom-store.worker.sqlite"),
        "stale-suffix",
        "stale-suffix-call",
        99,
    );

    let worker_dir = root.join("worker-agent");
    fs::create_dir_all(&worker_dir).expect("create worker agent dir");
    let worker_db = worker_dir.join("openclaw-agent.sqlite");
    let worker = Connection::open(&worker_db).expect("open worker store");
    worker
        .execute_batch(
            r#"
            CREATE TABLE transcript_events (
                session_id TEXT NOT NULL, seq INTEGER NOT NULL,
                event_json TEXT NOT NULL, created_at INTEGER NOT NULL,
                PRIMARY KEY (session_id, seq)
            );
            CREATE TABLE session_transcript_archives (
                session_id TEXT NOT NULL, generation INTEGER NOT NULL,
                encoding TEXT NOT NULL, archive_blob BLOB NOT NULL,
                PRIMARY KEY (session_id, generation)
            );
            INSERT INTO transcript_events VALUES
            ('worker', 1, '{"type":"session","version":3,"id":"worker","timestamp":"2026-08-31T01:03:00Z","cwd":"/tmp/worker"}', 1788138180000),
            ('worker', 2, '{"type":"message","id":"worker-call","timestamp":"2026-08-31T01:03:01Z","message":{"role":"assistant","provider":"openai","model":"gpt-5","timestamp":1788138181000,"usage":{"input":3,"output":1,"cacheRead":0,"cacheWrite":0}}}', 1788138181000);
            "#,
        )
        .expect("create worker store");
    drop(worker);
    write_file(
        &state.join("openclaw.json"),
        &format!(
            "{{ session: {{ store: '{}' }}, agents: {{ list: [{{ id: 'worker', agentDir: '{}' }}] }} }}",
            database.display(),
            worker_dir.display()
        ),
    );

    let sessions = state.join("agents/main/sessions");
    let deleted = concat!(
        r#"{"type":"session","version":3,"id":"deleted","timestamp":"2026-08-31T01:02:00Z","cwd":"/tmp/deleted"}"#,
        "\n",
        r#"{"type":"message","id":"deleted-call","timestamp":"2026-08-31T01:02:01Z","message":{"role":"assistant","provider":"openai","model":"gpt-5","timestamp":1788138121000,"usage":{"input":2,"output":1,"cacheRead":0,"cacheWrite":0}}}"#,
        "\n"
    );
    fs::create_dir_all(&sessions).expect("create session dir");
    fs::write(
        sessions.join("deleted.jsonl.deleted.2026-08-31T01-02-01Z.zst"),
        zstd::stream::encode_all(deleted.as_bytes(), 1).expect("compress deleted transcript"),
    )
    .expect("write deleted transcript");
    write_file(
        &sessions.join("ignored.checkpoint.550e8400-e29b-41d4-a716-446655440000.jsonl"),
        deleted,
    );
    write_file(&sessions.join("ignored.trajectory.jsonl"), deleted);

    let json = daily_json(
        "openclaw",
        &[
            ("OPENCLAW_HOME", &root),
            ("OPENCLAW_STATE_DIR", Path::new("~/state")),
            ("HOME", &root),
        ],
    );
    let row = &json.as_array().expect("array")[0];
    assert_eq!(row["input_tokens"].as_i64(), Some(20));
    assert_eq!(row["output_tokens"].as_i64(), Some(8));
    assert_eq!(row["cache_read_tokens"].as_i64(), Some(3));
    assert_eq!(row["cache_creation_tokens"].as_i64(), Some(3));
    assert_eq!(row["data_quality"]["valid_entries"].as_i64(), Some(4));
    assert_eq!(
        row["data_quality"]["dedup_skipped_entries"].as_i64(),
        Some(1)
    );
    assert_eq!(row["data_quality"]["parse_errors"].as_i64(), Some(1));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn openclaw_resolves_logical_and_templated_config_stores() {
    let root = unique_temp_dir("openclaw-config-store-e2e");
    let state = root.join("state");
    let logical = root.join("custom/session-state.json");
    write_openclaw_event_store(
        &logical.with_extension("sqlite"),
        "logical",
        "logical-call",
        2,
    );
    write_file(
        &state.join("openclaw.json"),
        &format!("{{ session: {{ store: '{}' }} }}", logical.display()),
    );
    let envs = [
        ("OPENCLAW_STATE_DIR", state.as_path()),
        ("HOME", root.as_path()),
    ];
    let logical_json = daily_json("openclaw", &envs);
    assert_eq!(
        logical_json.as_array().expect("array")[0]["input_tokens"].as_i64(),
        Some(2)
    );

    let template = root.join("stores/{agentId}/sessions.json");
    write_openclaw_event_store(
        &root.join("stores/main/openclaw-agent.sqlite"),
        "main-template",
        "main-template-call",
        3,
    );
    write_openclaw_event_store(
        &root.join("stores/worker/openclaw-agent.sqlite"),
        "worker-template",
        "worker-template-call",
        4,
    );
    write_file(
        &state.join("openclaw.json"),
        &format!(
            "{{ session: {{ store: '{}' }}, agents: {{ list: [{{ id: 'worker' }}] }} }}",
            template.display()
        ),
    );
    let template_json = daily_json("openclaw", &envs);
    assert_eq!(
        template_json.as_array().expect("array")[0]["input_tokens"].as_i64(),
        Some(7)
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn hermes_aggregates_current_task_dimensions_and_api_call_counts() {
    let root = unique_temp_dir("hermes-e2e");
    let db = root.join("state.db");
    let connection = Connection::open(&db).expect("open Hermes fixture");
    connection
        .execute_batch(
            r#"
            CREATE TABLE sessions (
                id TEXT PRIMARY KEY, started_at REAL NOT NULL, cwd TEXT,
                model TEXT, billing_provider TEXT, billing_base_url TEXT,
                billing_mode TEXT, input_tokens INTEGER DEFAULT 0,
                output_tokens INTEGER DEFAULT 0, cache_read_tokens INTEGER DEFAULT 0,
                cache_write_tokens INTEGER DEFAULT 0, reasoning_tokens INTEGER DEFAULT 0,
                api_call_count INTEGER DEFAULT 0, estimated_cost_usd REAL,
                actual_cost_usd REAL, cost_status TEXT, cost_source TEXT
            );
            CREATE TABLE session_model_usage (
                session_id TEXT NOT NULL, model TEXT NOT NULL,
                billing_provider TEXT NOT NULL DEFAULT '',
                billing_base_url TEXT NOT NULL DEFAULT '',
                billing_mode TEXT NOT NULL DEFAULT '', task TEXT NOT NULL DEFAULT '',
                api_call_count INTEGER NOT NULL DEFAULT 0,
                input_tokens INTEGER NOT NULL DEFAULT 0,
                output_tokens INTEGER NOT NULL DEFAULT 0,
                cache_read_tokens INTEGER NOT NULL DEFAULT 0,
                cache_write_tokens INTEGER NOT NULL DEFAULT 0,
                reasoning_tokens INTEGER NOT NULL DEFAULT 0,
                estimated_cost_usd REAL NOT NULL DEFAULT 0,
                actual_cost_usd REAL NOT NULL DEFAULT 0,
                cost_status TEXT, cost_source TEXT, first_seen REAL, last_seen REAL,
                PRIMARY KEY (session_id, model, billing_provider, billing_base_url, billing_mode, task)
            );
            INSERT INTO sessions (id, started_at, cwd, model, billing_provider)
            VALUES ('h-one', 1788145200, '/tmp/hermes', 'gpt-5', 'openai');
            INSERT INTO session_model_usage VALUES
            ('h-one','gpt-5','openai','https://api.openai.com','api','main',2,10,8,3,2,4,0.20,0.18,'actual','provider_cost_api',1788145200,1788145210),
            ('h-one','gpt-5','openai','https://api.openai.com','api','review',1,5,4,1,0,2,0.09,0.00,'estimated','official_docs_snapshot',1788145220,1788145230),
            ('h-one','gpt-5','openai','https://api.openai.com','subscription_included','title',1,2,1,0,0,0,0.00,0.00,'included','none',1788145240,1788145240);
            "#,
        )
        .expect("create Hermes fixture");
    drop(connection);

    let json = daily_json("hermes", &[("HERMES_HOME", &root), ("HOME", &root)]);
    let row = &json.as_array().expect("array")[0];
    assert_eq!(row["input_tokens"].as_i64(), Some(17));
    assert_eq!(row["output_tokens"].as_i64(), Some(7));
    assert_eq!(row["reasoning_tokens"].as_i64(), Some(6));
    assert_eq!(row["cache_read_tokens"].as_i64(), Some(4));
    assert_eq!(row["cache_creation_tokens"].as_i64(), Some(2));
    assert_eq!(row["data_quality"]["valid_entries"].as_i64(), Some(3));
    let top = top_models_json("hermes", &[("HERMES_HOME", &root), ("HOME", &root)]);
    assert_eq!(top["entries"][0]["count"].as_i64(), Some(4));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn hermes_preserves_usage_only_present_in_session_aggregate() {
    let root = unique_temp_dir("hermes-residual-e2e");
    let db = root.join("state.db");
    let connection = Connection::open(&db).expect("open Hermes fixture");
    connection
        .execute_batch(
            r#"
            CREATE TABLE sessions (
                id TEXT PRIMARY KEY, started_at REAL NOT NULL, cwd TEXT,
                model TEXT, billing_provider TEXT, billing_base_url TEXT,
                billing_mode TEXT, input_tokens INTEGER DEFAULT 0,
                output_tokens INTEGER DEFAULT 0, cache_read_tokens INTEGER DEFAULT 0,
                cache_write_tokens INTEGER DEFAULT 0, reasoning_tokens INTEGER DEFAULT 0,
                api_call_count INTEGER DEFAULT 0, estimated_cost_usd REAL,
                actual_cost_usd REAL, cost_status TEXT, cost_source TEXT
            );
            CREATE TABLE session_model_usage (
                session_id TEXT NOT NULL, model TEXT NOT NULL,
                billing_provider TEXT NOT NULL DEFAULT '', billing_base_url TEXT NOT NULL DEFAULT '',
                billing_mode TEXT NOT NULL DEFAULT '', task TEXT NOT NULL DEFAULT '',
                api_call_count INTEGER NOT NULL DEFAULT 0, input_tokens INTEGER NOT NULL DEFAULT 0,
                output_tokens INTEGER NOT NULL DEFAULT 0, cache_read_tokens INTEGER NOT NULL DEFAULT 0,
                cache_write_tokens INTEGER NOT NULL DEFAULT 0, reasoning_tokens INTEGER NOT NULL DEFAULT 0,
                estimated_cost_usd REAL NOT NULL DEFAULT 0, actual_cost_usd REAL NOT NULL DEFAULT 0,
                cost_status TEXT, cost_source TEXT, first_seen REAL, last_seen REAL,
                PRIMARY KEY (session_id, model, billing_provider, billing_base_url, billing_mode, task)
            );
            INSERT INTO sessions VALUES
            ('residual',1788145200,'/tmp/hermes',NULL,'custom','','api',30,20,5,4,8,6,0.50,0.30,'actual','provider_cost_api');
            INSERT INTO session_model_usage VALUES
            ('residual','unknown','custom','','api','main',4,20,19,3,2,2,0.30,0.18,'actual','provider_cost_api',1788145200,1788145210),
            ('residual','unknown','custom','','api','bad',1,5,2,0,0,3,0.10,0.10,'actual','provider_cost_api',1788145211,1788145212);
            "#,
        )
        .expect("create Hermes residual fixture");
    drop(connection);

    let usage = daily_json("hermes", &[("HERMES_HOME", &root), ("HOME", &root)]);
    let row = &usage.as_array().expect("array")[0];
    assert_eq!(row["input_tokens"].as_i64(), Some(30));
    assert_eq!(row["output_tokens"].as_i64(), Some(17));
    assert_eq!(row["reasoning_tokens"].as_i64(), Some(8));
    assert_eq!(row["cache_read_tokens"].as_i64(), Some(5));
    assert_eq!(row["cache_creation_tokens"].as_i64(), Some(4));
    assert_eq!(row["data_quality"]["valid_entries"].as_i64(), Some(2));
    assert_eq!(row["data_quality"]["parse_errors"].as_i64(), Some(1));
    let top = top_models_json("hermes", &[("HERMES_HOME", &root), ("HOME", &root)]);
    assert_eq!(top["entries"][0]["count"].as_i64(), Some(6));
    let cost = daily_cost_json("hermes", &[("HERMES_HOME", &root), ("HOME", &root)]);
    assert_eq!(
        cost.as_array().expect("array")[0]["cost"].as_f64(),
        Some(0.3)
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn xum_uses_complete_bucket_costs_and_preserves_children_of_invalid_rollups() {
    let root = unique_temp_dir("xum-cost-rollup-e2e");
    write_file(
        &root.join("sessions/child/session-usage.json"),
        r#"{"version":1,"byModel":{"custom:unpriced-model":{"input":{"tokens":10,"cost_usd":0.01},"cached":{"tokens":2,"cost_usd":0.002},"cacheCreate":{"tokens":1,"cost_usd":0.003},"output":{"tokens":4,"cost_usd":0.04},"reasoning":{"tokens":1,"cost_usd":0.005},"costsIncluded":true}},"lastRequest":{"timestamp":1788141600000}}"#,
    );
    write_file(
        &root.join("sessions/invalid-parent/session-usage.json"),
        r#"{"version":1,"byModel":{"custom:":{"input":{"tokens":99},"cached":{"tokens":0},"cacheCreate":{"tokens":0},"output":{"tokens":1},"reasoning":{"tokens":0}}},"lastRequest":{"timestamp":1788141600000},"rolledUpFrom":{"child":{}}}"#,
    );
    write_file(
        &root.join("sessions/zero-time-parent/session-usage.json"),
        r#"{"version":1,"byModel":{"custom:model":{"input":{"tokens":99},"cached":{"tokens":0},"cacheCreate":{"tokens":0},"output":{"tokens":1},"reasoning":{"tokens":0}}},"lastRequest":{"timestamp":0},"rolledUpFrom":{"child":{}}}"#,
    );
    let json = daily_cost_json("xum", &[("XUM_ROOT", &root), ("HOME", &root)]);
    let row = &json.as_array().expect("array")[0];
    assert_eq!(row["input_tokens"].as_i64(), Some(10));
    assert_eq!(row["cost"].as_f64(), Some(0.06));
    assert_eq!(row["data_quality"]["parse_errors"].as_i64(), Some(2));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn xum_rollup_cycles_keep_one_canonical_ledger_and_report_error() {
    let root = unique_temp_dir("xum-cycle-e2e");
    for (id, child, timestamp) in [("a", "b", 1788141600000_i64), ("b", "a", 1788141601000)] {
        write_file(
            &root.join(format!("sessions/{id}/session-usage.json")),
            &format!(
                r#"{{"version":1,"byModel":{{"custom:model":{{"input":{{"tokens":5}},"cached":{{"tokens":0}},"cacheCreate":{{"tokens":0}},"output":{{"tokens":1}},"reasoning":{{"tokens":0}}}}}},"lastRequest":{{"timestamp":{timestamp}}},"rolledUpFrom":{{"{child}":{{}}}}}}"#
            ),
        );
    }
    write_file(
        &root.join("sessions/parent/session-usage.json"),
        r#"{"version":1,"byModel":{"custom:model":{"input":{"tokens":5},"cached":{"tokens":0},"cacheCreate":{"tokens":0},"output":{"tokens":1},"reasoning":{"tokens":0}}},"lastRequest":{"timestamp":1788141602000},"rolledUpFrom":{"a":{}}}"#,
    );
    let json = daily_json("xum", &[("XUM_ROOT", &root), ("HOME", &root)]);
    let row = &json.as_array().expect("array")[0];
    assert_eq!(row["input_tokens"].as_i64(), Some(5));
    assert_eq!(row["data_quality"]["valid_entries"].as_i64(), Some(1));
    assert_eq!(row["data_quality"]["parse_errors"].as_i64(), Some(1));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn sources_listing_exposes_openclaw_xum_and_hermes() {
    let root = unique_temp_dir("batch5-sources");
    let (ok, stdout, stderr) = run_ccstats(&["sources", "--json"], &[("HOME", &root)]);
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    let rows: Value = serde_json::from_slice(&stdout).expect("sources JSON");
    let rows = rows.as_array().expect("array");
    assert!(rows.iter().any(|row| row["name"] == "openclaw"));
    assert!(rows.iter().any(|row| row["name"] == "xum"));
    assert!(rows.iter().any(|row| row["name"] == "hermes"));
    let _ = fs::remove_dir_all(root);
}
