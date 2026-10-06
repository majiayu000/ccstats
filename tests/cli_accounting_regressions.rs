mod common;

use common::{run_ccstats, write_file};
use rusqlite::{Connection, params};
use serde_json::{Value, json};
use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime};

fn json_report(args: &[&str], envs: &[(&str, &Path)]) -> Value {
    let (ok, stdout, stderr) = run_ccstats(args, envs);
    assert!(ok, "{}", String::from_utf8_lossy(&stderr));
    serde_json::from_slice(&stdout).expect("JSON report")
}

fn insert_usage(writer: &Connection, id: &str, input: i64) {
    let at = chrono::Utc::now().timestamp_millis();
    let data = json!({
        "role": "assistant", "modelID": "gpt-5", "finish": "stop",
        "cost": input as f64 / 1000.0,
        "tokens": {"input": input, "output": 0, "reasoning": 0,
            "cache": {"read": 0, "write": 0}},
        "time": {"created": at, "completed": at}
    });
    writer
        .execute(
            "INSERT INTO message (id, session_id, data) VALUES (?1, 'session', ?2)",
            params![id, data.to_string()],
        )
        .unwrap();
}

#[test]
fn sqlite_wal_updates_reach_cached_reports_today_and_sync() {
    for (source, db_env) in [
        ("opencode", "OPENCODE_DB"),
        ("mimocode", "MIMOCODE_DB"),
        ("kilo", "KILO_DB"),
    ] {
        let root = tempfile::tempdir().unwrap();
        let db = root.path().join("usage.db");
        let wal = root.path().join("usage.db-wal");
        let source_path = db.clone();
        #[cfg(unix)]
        let source_path = if source == "opencode" {
            let alias = root.path().join("source.db");
            std::os::unix::fs::symlink(&db, &alias).unwrap();
            alias
        } else {
            source_path
        };
        let data = root.path().join("data");
        let cache = root.path().join("cache");
        let sync = root.path().join("sync");
        fs::create_dir(&sync).unwrap();
        let envs = [
            ("HOME", root.path()),
            ("XDG_DATA_HOME", data.as_path()),
            ("XDG_CACHE_HOME", cache.as_path()),
            (db_env, source_path.as_path()),
        ];
        let writer = Connection::open(&db).unwrap();
        writer
            .execute_batch(
                "PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;
             CREATE TABLE session (id TEXT PRIMARY KEY, directory TEXT, time_created INTEGER);
             INSERT INTO session VALUES ('session', '/work', 0);
             CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT, data TEXT);",
            )
            .unwrap();
        insert_usage(&writer, "one", 10);
        writer
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
            .unwrap();
        assert_eq!(fs::metadata(&wal).unwrap().len(), 0);
        fs::OpenOptions::new()
            .write(true)
            .open(&db)
            .unwrap()
            .set_times(
                fs::FileTimes::new().set_modified(SystemTime::now() - Duration::from_secs(172_800)),
            )
            .unwrap();
        let stamp = fs::metadata(&db).unwrap();
        let daily = [
            "daily",
            "--source",
            source,
            "--json",
            "--offline",
            "--timezone",
            "UTC",
        ];
        // Repeated reads warm the old main-file-only cache while the empty WAL
        // stays open. Production must keep asking SQLite for its live snapshot.
        for _ in 0..2 {
            assert_eq!(json_report(&daily, &envs)[0]["total_tokens"], 10);
        }
        insert_usage(&writer, "two", 20);
        assert!(fs::metadata(&wal).unwrap().len() > 0);
        let after = fs::metadata(&db).unwrap();
        assert_eq!(stamp.len(), after.len());
        assert_eq!(stamp.modified().unwrap(), after.modified().unwrap());
        let report = json_report(&daily, &envs);
        assert_eq!(report[0]["total_tokens"], 30, "{source}: {report}");
        assert!((report[0]["cost"].as_f64().unwrap() - 0.03).abs() < 1e-10);
        let mut no_cache = daily.to_vec();
        no_cache.push("--no-cache");
        assert_eq!(json_report(&no_cache, &envs)[0]["total_tokens"], 30);
        let today = [
            "today",
            "--source",
            source,
            "--json",
            "--offline",
            "--timezone",
            "UTC",
        ];
        assert_eq!(json_report(&today, &envs)[0]["total_tokens"], 30);

        // Truncation, WAL removal, and a new WAL generation all stay fresh.
        writer
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
            .unwrap();
        assert_eq!(json_report(&daily, &envs)[0]["total_tokens"], 30);
        insert_usage(&writer, "three", 30);
        assert_eq!(json_report(&daily, &envs)[0]["total_tokens"], 60);
        drop(writer);
        assert!(!wal.exists());
        for _ in 0..2 {
            assert_eq!(json_report(&daily, &envs)[0]["total_tokens"], 60);
        }
        let writer = Connection::open(&db).unwrap();
        writer.execute_batch("PRAGMA wal_autocheckpoint=0").unwrap();
        insert_usage(&writer, "four", 40);
        assert!(fs::metadata(&wal).unwrap().len() > 0);
        assert_eq!(json_report(&daily, &envs)[0]["total_tokens"], 100);

        let (ok, _, stderr) = run_ccstats(
            &[
                "sync",
                "push",
                "--sync-dir",
                sync.to_str().unwrap(),
                "--timezone",
                "UTC",
            ],
            &envs,
        );
        assert!(ok, "{}", String::from_utf8_lossy(&stderr));
        let snapshot = fs::read_dir(sync.join("ccstats/devices"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let snapshot: Value = serde_json::from_slice(&fs::read(snapshot).unwrap()).unwrap();
        assert_eq!(snapshot["rows"].as_array().unwrap().len(), 1);
        assert_eq!(snapshot["rows"][0]["source"], source);
        assert_eq!(snapshot["rows"][0]["stats"]["input_tokens"], 100);
    }
}

fn codex_count(total: i64, timestamp: &str) -> Value {
    json!({"type":"event_msg", "timestamp":timestamp,
    "payload":{"type":"token_count", "info":{
        "total_token_usage":{"input_tokens":total,"output_tokens":0}
    }}})
}

#[test]
fn codex_reset_usage_survives_loader_cache_and_archived_copy_dedup() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("codex");
    let cache = root.path().join("cache");
    let file = home.join("sessions/reset.jsonl");
    let mut rows = vec![json!({"type":"session_meta", "payload":{
        "id":"reset-session", "source":"cli", "model":"gpt-5"
    }})];
    for (index, total) in [100, 120, 20, 100, 120, 120].into_iter().enumerate() {
        // Keep the initial cumulative checkpoint on a different day: its
        // existing file-local scope is deliberately distinct from replay IDs.
        let day = if index == 0 {
            "2026-09-04"
        } else {
            "2026-09-05"
        };
        rows.push(codex_count(total, &format!("{day}T12:00:00Z")));
    }
    write_file(
        &file,
        &rows
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n"),
    );
    let envs = [
        ("CODEX_HOME", home.as_path()),
        ("XDG_CACHE_HOME", cache.as_path()),
    ];
    let args = [
        "daily",
        "--source",
        "codex",
        "--json",
        "--offline",
        "--no-cost",
        "--timezone",
        "UTC",
    ];
    for _ in 0..2 {
        let report = json_report(&args, &envs);
        let total: i64 = report
            .as_array()
            .unwrap()
            .iter()
            .map(|day| day["total_tokens"].as_i64().unwrap())
            .sum();
        assert_eq!(total, 220);
    }
    write_file(
        &home.join("archived_sessions/copy.jsonl"),
        &fs::read_to_string(&file).unwrap(),
    );
    let mut selected = args.to_vec();
    selected.extend(["--since", "2026-09-05", "--until", "2026-09-05"]);
    for _ in 0..2 {
        let report = json_report(&selected, &envs);
        assert_eq!(report[0]["total_tokens"], 120);
        assert_eq!(report[0]["data_quality"]["valid_entries"], 3);
        assert_eq!(report[0]["data_quality"]["dedup_skipped_entries"], 3);
    }
}

#[test]
fn codex_missing_model_is_unknown_in_reports_and_synced_facts() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("codex");
    let data = root.path().join("data");
    let cache = root.path().join("cache");
    let sync = root.path().join("sync");
    fs::create_dir(&sync).unwrap();
    let file = home.join("sessions/unknown.jsonl");
    write_file(&file, &codex_count(100, "2026-09-05T12:00:00Z").to_string());
    let envs = [
        ("HOME", root.path()),
        ("CODEX_HOME", home.as_path()),
        ("XDG_DATA_HOME", data.as_path()),
        ("XDG_CACHE_HOME", cache.as_path()),
    ];
    let args = [
        "daily",
        "--source",
        "codex",
        "--json",
        "--offline",
        "--breakdown",
        "--timezone",
        "UTC",
    ];
    for strict in [false, true] {
        let mut args = args.to_vec();
        if strict {
            args.push("--strict-pricing");
        }
        let report = json_report(&args, &envs);
        assert_eq!(report[0]["total_tokens"], 100);
        assert_eq!(report[0]["cost"], Value::Null);
        assert_eq!(report[0]["breakdown"][0]["model"], "unknown-model");
        assert_eq!(report[0]["breakdown"][0]["cost"], Value::Null);
    }
    let (ok, stdout, stderr) = run_ccstats(
        &[
            "daily",
            "--source",
            "codex",
            "--csv",
            "--offline",
            "--breakdown",
            "--timezone",
            "UTC",
        ],
        &envs,
    );
    assert!(ok, "{}", String::from_utf8_lossy(&stderr));
    let csv = String::from_utf8(stdout).unwrap();
    assert!(csv.contains("unknown-model"), "{csv}");
    assert!(!csv.contains("gpt-5"), "{csv}");
    let report = json_report(
        &[
            "session",
            "--source",
            "codex",
            "--details",
            "--json",
            "--offline",
            "--timezone",
            "UTC",
        ],
        &envs,
    );
    assert_eq!(
        report["sessions"][0]["breakdown"][0]["model"],
        "unknown-model"
    );
    assert_eq!(report["sessions"][0]["breakdown"][0]["cost"], Value::Null);

    let (ok, _, stderr) = run_ccstats(
        &[
            "sync",
            "push",
            "--sync-dir",
            sync.to_str().unwrap(),
            "--timezone",
            "UTC",
        ],
        &envs,
    );
    assert!(ok, "{}", String::from_utf8_lossy(&stderr));
    let snapshot = fs::read_dir(sync.join("ccstats/devices"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let snapshot: Value = serde_json::from_slice(&fs::read(snapshot).unwrap()).unwrap();
    assert_eq!(snapshot["rows"][0]["model"], "unknown-model");
    assert_eq!(snapshot["rows"][0]["stats"]["input_tokens"], 100);

    // An explicit model remains priceable without changing recorded tokens.
    let mut known = codex_count(100, "2026-09-05T12:00:00Z");
    known["payload"]["info"]["model"] = json!("gpt-5");
    write_file(&file, &known.to_string());
    let report = json_report(&args, &envs);
    assert_eq!(report[0]["total_tokens"], 100);
    assert!(report[0]["cost"].as_f64().is_some_and(|cost| cost > 0.0));
}
