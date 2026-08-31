mod common;

use common::{run_ccstats, unique_temp_dir, write_file};
use rusqlite::{Connection, params};
use serde_json::Value;
use std::fs;
use std::path::Path;

fn daily_json(source: &str, envs: &[(&str, &Path)]) -> Value {
    daily_json_with_cost(source, envs, false)
}

fn daily_json_with_cost(source: &str, envs: &[(&str, &Path)], include_cost: bool) -> Value {
    let mut args = vec![
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
    ];
    if !include_cost {
        args.push("--no-cost");
    }
    let (ok, stdout, stderr) = run_ccstats(&args, envs);
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    serde_json::from_slice(&stdout).expect("daily JSON")
}

fn insert_single_message(connection: &Connection, session: &str, message: &str) {
    connection
        .execute(
            "INSERT INTO session (id, directory, parent_id, time_created)
             VALUES (?1, '/tmp/discovery-project', NULL, 1788145400000)",
            params![session],
        )
        .expect("insert discovery session");
    connection
        .execute(
            "INSERT INTO message (id, session_id, data) VALUES ('discovery-message', ?1, ?2)",
            params![session, message],
        )
        .expect("insert discovery message");
}

fn create_schema(connection: &Connection, include_v2: bool) {
    connection
        .execute_batch(
            "CREATE TABLE session (
                id TEXT PRIMARY KEY,
                directory TEXT NOT NULL,
                parent_id TEXT,
                time_created INTEGER NOT NULL
            );
            CREATE TABLE message (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                data TEXT NOT NULL
            );",
        )
        .expect("create OpenCode-family schema");
    if include_v2 {
        connection
            .execute_batch(
                "CREATE TABLE session_message (
                    id TEXT PRIMARY KEY,
                    session_id TEXT NOT NULL,
                    type TEXT NOT NULL,
                    data TEXT NOT NULL
                );",
            )
            .expect("create v2 message schema");
    }
}

fn assert_one_normalized_call(json: &Value, expected_dedup: i64) {
    let rows = json.as_array().expect("array");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["input_tokens"].as_i64(), Some(100));
    assert_eq!(rows[0]["output_tokens"].as_i64(), Some(20));
    assert_eq!(rows[0]["reasoning_tokens"].as_i64(), Some(5));
    assert_eq!(rows[0]["cache_read_tokens"].as_i64(), Some(30));
    assert_eq!(rows[0]["cache_creation_tokens"].as_i64(), Some(10));
    assert_eq!(rows[0]["total_tokens"].as_i64(), Some(165));
    assert_eq!(rows[0]["data_quality"]["valid_entries"].as_i64(), Some(1));
    assert_eq!(
        rows[0]["data_quality"]["dedup_skipped_entries"].as_i64(),
        Some(expected_dedup)
    );
    assert_eq!(rows[0]["data_quality"]["parse_errors"].as_u64(), Some(0));
}

#[test]
fn mimocode_database_deduplicates_fork_copied_history() {
    let root = unique_temp_dir("mimocode-e2e");
    let db = root.join("mimocode.db");
    write_file(&db, "");
    let connection = Connection::open(&db).expect("open MiMo fixture database");
    create_schema(&connection, false);
    connection
        .execute(
            "INSERT INTO session (id, directory, parent_id, time_created) VALUES
             ('parent', '/tmp/mimo-project', NULL, 1788145400000),
             ('fork', '/tmp/mimo-fork-project', NULL, 1788145500000)",
            [],
        )
        .expect("insert sessions");
    let data = r#"{"role":"assistant","modelID":"mimo-v2.5-pro","providerID":"mimo","finish":"stop","cost":0.25,"tokens":{"input":100,"output":20,"reasoning":5,"cache":{"read":30,"write":10}},"time":{"created":1788145445000,"completed":1788145446000}}"#;
    connection
        .execute(
            "INSERT INTO message (id, session_id, data) VALUES (?1, ?2, ?3)",
            params!["original-message", "parent", data],
        )
        .expect("insert original message");
    connection
        .execute(
            "INSERT INTO message (id, session_id, data) VALUES (?1, ?2, ?3)",
            params!["copied-message", "fork", data],
        )
        .expect("insert copied message");
    drop(connection);

    let json = daily_json("mimocode", &[("MIMOCODE_DB", &db), ("HOME", &root)]);
    assert_one_normalized_call(&json, 1);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn kilo_database_reconciles_dual_schema_and_fork_copy() {
    let root = unique_temp_dir("kilo-cli-e2e");
    let db = root.join("kilo.db");
    write_file(&db, "");
    let connection = Connection::open(&db).expect("open Kilo fixture database");
    create_schema(&connection, true);
    connection
        .execute(
            "INSERT INTO session (id, directory, parent_id, time_created) VALUES
             ('parent', '/tmp/kilo-project', NULL, 1788145400000),
             ('fork', '/tmp/kilo-fork-project', NULL, 1788145500000),
             ('fork-2', '/tmp/kilo-second-fork', NULL, 1788145600000)",
            [],
        )
        .expect("insert sessions");
    let legacy = r#"{"role":"assistant","modelID":"claude-sonnet-4","providerID":"anthropic","finish":"stop","cost":0.25,"tokens":{"input":100,"output":20,"reasoning":5,"cache":{"read":30,"write":10}},"time":{"created":1788145445000,"completed":1788145446000}}"#;
    let current = r#"{"agent":"build","model":{"id":"claude-sonnet-4","providerID":"anthropic"},"finish":"stop","cost":0.25,"tokens":{"input":100,"output":20,"reasoning":5,"cache":{"read":30,"write":10}},"time":{"created":1788145445000,"completed":1788145446000}}"#;
    let copied = r#"{"agent":"build","model":{"id":"claude-sonnet-4","providerID":"anthropic"},"finish":"stop","cost":0.0,"tokens":{"input":100,"output":20,"reasoning":5,"cache":{"read":30,"write":10}},"time":{"created":1788145445000,"completed":1788145446000}}"#;
    connection
        .execute(
            "INSERT INTO message (id, session_id, data) VALUES (?1, 'parent', ?2)",
            params!["shared-message", legacy],
        )
        .expect("insert legacy projection");
    connection
        .execute(
            "INSERT INTO session_message (id, session_id, type, data) VALUES (?1, 'parent', 'assistant', ?2)",
            params!["shared-message", current],
        )
        .expect("insert current projection");
    connection
        .execute(
            "INSERT INTO session_message (id, session_id, type, data) VALUES ('fork-copy', 'fork', 'assistant', ?1)",
            params![copied],
        )
        .expect("insert fork copy");
    connection
        .execute(
            "INSERT INTO session_message (id, session_id, type, data) VALUES ('fork-copy-2', 'fork-2', 'assistant', ?1)",
            params![copied],
        )
        .expect("insert second fork copy");
    drop(connection);

    let json = daily_json("kilo", &[("KILO_DB", &db), ("HOME", &root)]);
    assert_one_normalized_call(&json, 3);

    let connection = Connection::open(&db).expect("reopen Kilo fixture database");
    connection
        .execute("DELETE FROM message WHERE session_id = 'parent'", [])
        .expect("delete original legacy message");
    connection
        .execute(
            "DELETE FROM session_message WHERE session_id = 'parent'",
            [],
        )
        .expect("delete original current message");
    connection
        .execute("DELETE FROM session WHERE id = 'parent'", [])
        .expect("delete original session");
    drop(connection);
    let copies_only = daily_json("kilo", &[("KILO_DB", &db), ("HOME", &root)]);
    assert_one_normalized_call(&copies_only, 1);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn sources_listing_exposes_mimocode_and_kilo_cli() {
    let root = unique_temp_dir("mimocode-kilo-sources");
    let (ok, stdout, stderr) = run_ccstats(&["sources", "--json"], &[("HOME", &root)]);
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    let rows: Value = serde_json::from_slice(&stdout).expect("sources JSON");
    let rows = rows.as_array().expect("array");
    assert!(rows.iter().any(|row| row["name"] == "mimocode"));
    assert!(rows.iter().any(|row| row["name"] == "kilo"));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn mimocode_home_and_kilo_xdg_channel_databases_are_discovered() {
    let root = unique_temp_dir("opencode-family-discovery");
    let message = r#"{"role":"assistant","modelID":"verified-model","providerID":"verified","finish":"stop","cost":0.0,"tokens":{"input":100,"output":20,"reasoning":5,"cache":{"read":30,"write":10}},"time":{"created":1788145445000,"completed":1788145446000}}"#;

    let mimocode_home = root.join("mimo-home");
    let mimocode_db = mimocode_home.join("data/mimocode.db");
    write_file(&mimocode_db, "");
    let connection = Connection::open(&mimocode_db).expect("open discovered MiMo database");
    create_schema(&connection, false);
    insert_single_message(&connection, "mimo-discovery", message);
    drop(connection);
    assert_one_normalized_call(
        &daily_json(
            "mimocode",
            &[("MIMOCODE_HOME", &mimocode_home), ("HOME", &root)],
        ),
        0,
    );

    let xdg = root.join("share");
    let kilo_db = xdg.join("kilo/kilo-nightly.db");
    write_file(&kilo_db, "");
    let connection = Connection::open(&kilo_db).expect("open discovered Kilo database");
    create_schema(&connection, false);
    insert_single_message(&connection, "kilo-discovery", message);
    drop(connection);
    assert_one_normalized_call(
        &daily_json("kilo", &[("XDG_DATA_HOME", &xdg), ("HOME", &root)]),
        0,
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn mimocode_recorded_zero_cost_wins_and_invalid_cost_is_reported() {
    let root = unique_temp_dir("mimocode-cost-e2e");
    let db = root.join("mimocode.db");
    write_file(&db, "");
    let connection = Connection::open(&db).expect("open MiMo cost fixture database");
    create_schema(&connection, false);
    connection
        .execute(
            "INSERT INTO session (id, directory, parent_id, time_created)
             VALUES ('cost-session', '/tmp/cost-project', NULL, 1788145400000)",
            [],
        )
        .expect("insert cost session");
    let zero = r#"{"role":"assistant","modelID":"gpt-5","providerID":"openai","finish":"stop","cost":0.0,"tokens":{"input":10,"output":2,"reasoning":0,"cache":{"read":0,"write":0}},"time":{"created":1788145445000,"completed":1788145446000}}"#;
    let invalid = r#"{"role":"assistant","modelID":"gpt-5","providerID":"openai","finish":"stop","cost":-1.0,"tokens":{"input":999,"output":99,"reasoning":0,"cache":{"read":0,"write":0}},"time":{"created":1788145447000,"completed":1788145448000}}"#;
    connection
        .execute(
            "INSERT INTO message (id, session_id, data) VALUES
             ('zero-cost', 'cost-session', ?1),
             ('invalid-cost', 'cost-session', ?2)",
            params![zero, invalid],
        )
        .expect("insert cost messages");
    drop(connection);

    let json = daily_json_with_cost("mimocode", &[("MIMOCODE_DB", &db), ("HOME", &root)], true);
    let row = &json.as_array().expect("array")[0];
    assert_eq!(row["input_tokens"].as_i64(), Some(10));
    assert_eq!(row["cost"].as_f64(), Some(0.0));
    assert_eq!(row["data_quality"]["valid_entries"].as_i64(), Some(1));
    assert_eq!(row["data_quality"]["parse_errors"].as_u64(), Some(1));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn relative_mimocode_home_never_falls_back_to_xdg() {
    let root = unique_temp_dir("mimocode-invalid-home");
    let xdg = root.join("share");
    let fallback_db = xdg.join("mimocode/mimocode.db");
    write_file(&fallback_db, "");
    let connection = Connection::open(&fallback_db).expect("open fallback database");
    create_schema(&connection, false);
    insert_single_message(
        &connection,
        "must-not-load",
        r#"{"role":"assistant","modelID":"wrong-db","providerID":"test","finish":"stop","cost":0.0,"tokens":{"input":100,"output":20,"reasoning":5,"cache":{"read":30,"write":10}},"time":{"created":1788145445000,"completed":1788145446000}}"#,
    );
    drop(connection);
    let relative_home = Path::new("invalid-relative-mimocode-home");
    let relative_db = root.join(relative_home).join("data/mimocode.db");
    write_file(&relative_db, "");
    let connection = Connection::open(&relative_db).expect("open relative MiMo database");
    create_schema(&connection, false);
    insert_single_message(
        &connection,
        "relative-must-not-load",
        r#"{"role":"assistant","modelID":"also-wrong","providerID":"test","finish":"stop","cost":0.0,"tokens":{"input":200,"output":40,"reasoning":10,"cache":{"read":60,"write":20}},"time":{"created":1788145445000,"completed":1788145446000}}"#,
    );
    drop(connection);
    let (ok, stdout, stderr) = run_ccstats(
        &[
            "daily",
            "--source",
            "mimocode",
            "--json",
            "--offline",
            "--no-cost",
            "--timezone",
            "UTC",
        ],
        &[
            ("MIMOCODE_HOME", relative_home),
            ("XDG_DATA_HOME", &xdg),
            ("HOME", &root),
            ("CCSTATS_TEST_CWD", &root),
        ],
    );
    assert!(ok);
    let rows = serde_json::from_slice::<Value>(&stdout).unwrap();
    let row = &rows.as_array().expect("array")[0];
    assert_eq!(row["data_quality"]["valid_entries"].as_i64(), Some(0));
    assert_eq!(row["data_quality"]["parse_errors"].as_u64(), Some(1));
    assert!(String::from_utf8_lossy(&stderr).contains("malformed"));
    let _ = fs::remove_dir_all(root);
}
