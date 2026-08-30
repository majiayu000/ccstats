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

#[test]
fn copilot_otel_counts_chat_spans_without_agent_rollup_double_counting() {
    let root = unique_temp_dir("copilot-e2e");
    let otel = root.join("copilot-otel.jsonl");
    write_file(
        &otel,
        r#"{"type":"span","traceId":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","spanId":"bbbbbbbbbbbbbbbb","name":"invoke_agent GitHub Copilot","startTime":[1788145440,0],"endTime":[1788145450,0],"attributes":{"gen_ai.operation.name":"invoke_agent","gen_ai.request.model":"gpt-5","gen_ai.conversation.id":"copilot-session","gen_ai.usage.input_tokens":100,"gen_ai.usage.output_tokens":20}}
{"type":"span","traceId":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","spanId":"cccccccccccccccc","parentSpanId":"bbbbbbbbbbbbbbbb","name":"chat gpt-5","startTime":[1788145445,0],"endTime":[1788145446,0],"attributes":{"gen_ai.operation.name":"chat","gen_ai.request.model":"gpt-5","gen_ai.response.model":"gpt-5","gen_ai.response.id":"response-e2e","gen_ai.conversation.id":"copilot-session","gen_ai.response.finish_reasons":["stop"],"gen_ai.usage.input_tokens":100,"gen_ai.usage.output_tokens":20,"gen_ai.usage.cache_read.input_tokens":30,"gen_ai.usage.cache_creation.input_tokens":10,"gen_ai.usage.reasoning.output_tokens":5,"github.copilot.cost":0.0123}}
"#,
    );
    let copied = root.join(".copilot/otel/copied.jsonl");
    fs::create_dir_all(copied.parent().expect("Copilot OTel directory"))
        .expect("create Copilot OTel directory");
    fs::copy(&otel, &copied).expect("copy OTel fixture");

    let json = daily_json(
        "copilot",
        &[("COPILOT_OTEL_FILE_EXPORTER_PATH", &otel), ("HOME", &root)],
    );
    let rows = json.as_array().expect("array");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["input_tokens"].as_i64(), Some(60));
    assert_eq!(rows[0]["output_tokens"].as_i64(), Some(15));
    assert_eq!(rows[0]["reasoning_tokens"].as_i64(), Some(5));
    assert_eq!(rows[0]["cache_read_tokens"].as_i64(), Some(30));
    assert_eq!(rows[0]["cache_creation_tokens"].as_i64(), Some(10));
    assert_eq!(rows[0]["total_tokens"].as_i64(), Some(120));
    assert_eq!(rows[0]["data_quality"]["valid_entries"].as_i64(), Some(1));
    assert_eq!(
        rows[0]["data_quality"]["dedup_skipped_entries"].as_i64(),
        Some(1)
    );
    assert_eq!(rows[0]["data_quality"]["parse_errors"].as_u64(), Some(0));

    let _ = fs::remove_dir_all(root);
}

fn create_goose_db(path: &Path) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create Goose data dir");
    }
    let connection = Connection::open(path).expect("open fixture database");
    connection
        .execute_batch(
            "
            CREATE TABLE sessions (
                id TEXT PRIMARY KEY,
                working_dir TEXT NOT NULL,
                created_at TIMESTAMP NOT NULL,
                accumulated_input_tokens INTEGER,
                accumulated_output_tokens INTEGER,
                accumulated_total_tokens INTEGER,
                accumulated_cache_read_tokens INTEGER,
                accumulated_cache_write_tokens INTEGER,
                accumulated_cost REAL,
                provider_name TEXT,
                model_config_json TEXT
            );
            CREATE TABLE usage_ledger (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                created_timestamp INTEGER NOT NULL,
                model TEXT,
                input_tokens INTEGER,
                output_tokens INTEGER,
                total_tokens INTEGER,
                cache_read_tokens INTEGER,
                cache_write_tokens INTEGER,
                cost REAL,
                cost_source TEXT,
                is_compaction INTEGER DEFAULT 0
            );
            ",
        )
        .expect("create current Goose schema");
    connection
        .execute(
            "INSERT INTO sessions
             (id, working_dir, created_at, accumulated_input_tokens,
              accumulated_output_tokens, accumulated_total_tokens,
              accumulated_cache_read_tokens, accumulated_cache_write_tokens,
              accumulated_cost, provider_name, model_config_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                "goose-session",
                "/tmp/goose-project",
                "2026-08-31 03:00:00",
                999_i64,
                999_i64,
                1998_i64,
                0_i64,
                0_i64,
                9.99_f64,
                "anthropic",
                r#"{"model_name":"claude-sonnet-4"}"#,
            ],
        )
        .expect("insert session");
    connection
        .execute(
            "INSERT INTO usage_ledger
             (session_id, created_timestamp, model, input_tokens, output_tokens,
              total_tokens, cache_read_tokens, cache_write_tokens, cost,
              cost_source, is_compaction)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 0)",
            params![
                "goose-session",
                1_788_145_445_i64,
                "claude-sonnet-4",
                100_i64,
                20_i64,
                120_i64,
                30_i64,
                10_i64,
                0.25_f64,
                "provider_reported",
            ],
        )
        .expect("insert provider call");
    connection
        .execute(
            "INSERT INTO usage_ledger
             (session_id, created_timestamp, model, input_tokens, output_tokens,
              total_tokens, cache_read_tokens, cache_write_tokens, cost,
              cost_source, is_compaction)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 1)",
            params![
                "goose-session",
                1_788_145_505_i64,
                "claude-sonnet-4",
                40_i64,
                5_i64,
                45_i64,
                0_i64,
                0_i64,
                0.05_f64,
                "estimated",
            ],
        )
        .expect("insert compaction call");
}

#[test]
fn goose_usage_ledger_reaches_daily_cli_with_cache_normalization() {
    let root = unique_temp_dir("goose-e2e");
    let db = root.join("data/sessions/sessions.db");
    create_goose_db(&db);

    let json = daily_json("goose", &[("GOOSE_PATH_ROOT", &root), ("HOME", &root)]);
    let rows = json.as_array().expect("array");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["input_tokens"].as_i64(), Some(100));
    assert_eq!(rows[0]["output_tokens"].as_i64(), Some(25));
    assert_eq!(rows[0]["reasoning_tokens"].as_i64(), Some(0));
    assert_eq!(rows[0]["cache_read_tokens"].as_i64(), Some(30));
    assert_eq!(rows[0]["cache_creation_tokens"].as_i64(), Some(10));
    assert_eq!(rows[0]["total_tokens"].as_i64(), Some(165));
    assert_eq!(rows[0]["data_quality"]["valid_entries"].as_i64(), Some(2));
    assert_eq!(rows[0]["data_quality"]["parse_errors"].as_u64(), Some(0));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn sources_listing_exposes_copilot_and_goose_capabilities() {
    let root = unique_temp_dir("copilot-goose-sources");
    let (ok, stdout, stderr) = run_ccstats(&["sources", "--json"], &[("HOME", &root)]);
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    let rows: Value = serde_json::from_slice(&stdout).expect("sources JSON");
    let rows = rows.as_array().expect("array");

    let copilot = rows
        .iter()
        .find(|row| row["name"].as_str() == Some("copilot"))
        .expect("Copilot source");
    assert_eq!(
        copilot["capabilities"]["has_reasoning_tokens"].as_bool(),
        Some(true)
    );

    let goose = rows
        .iter()
        .find(|row| row["name"].as_str() == Some("goose"))
        .expect("Goose source");
    assert_eq!(goose["capabilities"]["has_projects"].as_bool(), Some(true));
    assert_eq!(
        goose["capabilities"]["has_cache_creation"].as_bool(),
        Some(true)
    );

    let _ = fs::remove_dir_all(root);
}
