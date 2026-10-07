mod common;

use common::{run_ccstats, unique_temp_dir, write_file};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

const SECRET_PROMPT: &str = "please refactor the payroll module secret-prompt-text";

struct Device {
    home: PathBuf,
    data: PathBuf,
}

fn device(prefix: &str, msg_id: &str, input: i64) -> Device {
    let root = unique_temp_dir(prefix);
    let home = root.join("home");
    // Project directory names encode absolute paths; they must not leave the device.
    write_file(
        &home.join(".claude/projects/-Users-alice-secret-client/session.jsonl"),
        &format!(
            r#"{{"timestamp":"2026-02-06T10:00:00Z","cwd":{},"message":{{"id":"{msg_id}","model":"claude-3-5-sonnet-20241022","stop_reason":"end_turn","role":"assistant","content":[{{"type":"text","text":"{SECRET_PROMPT}"}}],"usage":{{"input_tokens":{input},"output_tokens":50,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}}}}
{{"timestamp":"2026-02-06T09:59:00Z","type":"user","message":{{"role":"user","content":"{SECRET_PROMPT}"}}}}
"#,
            // Serialize so Windows backslashes stay valid JSON escapes.
            serde_json::to_string(&home.join("work/secret-client")).unwrap()
        ),
    );
    Device {
        home,
        data: root.join("data"),
    }
}

fn run(device: &Device, sync: &Path, args: &[&str]) -> (bool, String, String) {
    let mut full = args.to_vec();
    full.extend(["--sync-dir", sync.to_str().unwrap()]);
    let (ok, stdout, stderr) = run_ccstats(
        &full,
        &[("HOME", &device.home), ("XDG_DATA_HOME", &device.data)],
    );
    (
        ok,
        String::from_utf8_lossy(&stdout).into_owned(),
        String::from_utf8_lossy(&stderr).into_owned(),
    )
}

fn daily_total(device: &Device, sync: &Path, devices: &str) -> i64 {
    let (ok, stdout, stderr) = run(
        device,
        sync,
        &[
            "daily",
            "-j",
            "-O",
            "--no-cost",
            "--timezone",
            "UTC",
            "--source",
            "claude",
            "--devices",
            devices,
        ],
    );
    assert!(ok, "stderr: {stderr}");
    let json: Value = serde_json::from_str(&stdout).expect("json");
    json.as_array().expect("rows")[0]["total_tokens"]
        .as_i64()
        .unwrap()
}

fn device_files(sync: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(sync.join("ccstats/devices"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    files.sort();
    files
}

#[test]
fn two_devices_round_trip_through_sync_dir() {
    let sync = unique_temp_dir("sync-dir");
    let laptop = device("sync-laptop", "msg_laptop", 100);
    let desktop = device("sync-desktop", "msg_desktop", 1000);

    for dev in [&laptop, &desktop] {
        let (ok, stdout, stderr) = run(dev, &sync, &["sync", "push", "--timezone", "UTC"]);
        assert!(ok, "stderr: {stderr}");
        assert!(stdout.contains("Wrote"), "{stdout}");
    }
    // Pushing again rewrites the same file instead of adding one.
    let (ok, _, stderr) = run(&laptop, &sync, &["sync", "push", "--timezone", "UTC"]);
    assert!(ok, "stderr: {stderr}");
    assert_eq!(device_files(&sync).len(), 2);

    // Default stays this device only.
    let (ok, stdout, stderr) = run(
        &laptop,
        &sync,
        &[
            "daily",
            "-j",
            "-O",
            "--no-cost",
            "--timezone",
            "UTC",
            "--source",
            "claude",
        ],
    );
    assert!(ok, "stderr: {stderr}");
    let json: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json[0]["total_tokens"].as_i64(), Some(150));
    assert_eq!(daily_total(&laptop, &sync, "this"), 150);

    // all = live local + the other device; the laptop's own synced file is skipped.
    assert_eq!(daily_total(&laptop, &sync, "all"), 150 + 1050);
    assert_eq!(daily_total(&desktop, &sync, "all"), 1050 + 150);

    // --source all merges the same rows.
    let (ok, stdout, stderr) = run(
        &laptop,
        &sync,
        &[
            "daily",
            "-j",
            "-O",
            "--no-cost",
            "--timezone",
            "UTC",
            "--source",
            "all",
            "--no-source-breakdown",
            "--devices",
            "all",
        ],
    );
    assert!(ok, "stderr: {stderr}");
    let json: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json[0]["total_tokens"].as_i64(), Some(1200));

    let (ok, stdout, stderr) = run(&laptop, &sync, &["sync", "status"]);
    assert!(ok, "stderr: {stderr}");
    assert!(stdout.contains("(this device)"), "{stdout}");
    assert_eq!(stdout.matches("\n- ").count(), 2, "{stdout}");
}

#[test]
fn foreign_and_partial_files_warn_without_failing_reports() {
    let sync = unique_temp_dir("sync-broken");
    let laptop = device("sync-broken-laptop", "msg_a", 100);
    write_file(
        &sync.join("ccstats/devices/partial.json"),
        "{\"schema_version\":1,",
    );
    write_file(
        &sync.join("ccstats/devices/future.json"),
        r#"{"schema_version":2,"device_id":"0123456789abcdef"}"#,
    );

    let (ok, stdout, stderr) = run(
        &laptop,
        &sync,
        &[
            "daily",
            "-j",
            "-O",
            "--no-cost",
            "--timezone",
            "UTC",
            "--source",
            "claude",
            "--devices",
            "all",
        ],
    );
    assert!(ok, "stderr: {stderr}");
    assert!(stderr.contains("skipping partial.json"), "{stderr}");
    assert!(stderr.contains("schema version 2"), "{stderr}");
    let json: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json[0]["total_tokens"].as_i64(), Some(150));
}

#[test]
fn devices_flag_errors_are_explicit() {
    let sync = unique_temp_dir("sync-errors");
    let laptop = device("sync-errors-laptop", "msg_a", 100);

    let (ok, _, stderr) = run(
        &laptop,
        &sync,
        &["session", "--source", "claude", "--devices", "all"],
    );
    assert!(!ok);
    assert!(stderr.contains("--devices only supports"), "{stderr}");

    let (ok, _, stderr) = run(
        &laptop,
        &sync,
        &["daily", "--source", "claude", "--devices", "nas"],
    );
    assert!(!ok);
    assert!(stderr.contains("no device matches"), "{stderr}");

    let (ok, _, stderr) = run_ccstats(
        &["sync", "status"],
        &[("HOME", &laptop.home), ("XDG_DATA_HOME", &laptop.data)],
    );
    assert!(!ok);
    assert!(
        String::from_utf8_lossy(&stderr).contains("no sync directory configured"),
        "{}",
        String::from_utf8_lossy(&stderr)
    );
}

#[test]
fn exported_file_holds_no_paths_or_message_text() {
    let sync = unique_temp_dir("sync-privacy");
    let laptop = device("sync-privacy-laptop", "msg_a", 100);
    let (ok, _, stderr) = run(&laptop, &sync, &["sync", "push", "--timezone", "UTC"]);
    assert!(ok, "stderr: {stderr}");

    let files = device_files(&sync);
    assert_eq!(files.len(), 1);
    let content = fs::read_to_string(&files[0]).unwrap();
    let home = laptop.home.display().to_string();
    for forbidden in [
        home.as_str(),
        SECRET_PROMPT,
        "secret-client",
        "Users-alice",
        "session.jsonl",
        "msg_a",
    ] {
        assert!(
            !content.contains(forbidden),
            "exported file leaks {forbidden:?}:\n{content}"
        );
    }
    let json: Value = serde_json::from_str(&content).unwrap();
    assert_eq!(json["schema_version"].as_u64(), Some(1));
    assert_eq!(json["rows"][0]["source"].as_str(), Some("claude"));
    assert_eq!(json["rows"][0]["date"].as_str(), Some("2026-02-06"));
}

#[test]
fn parse_failure_keeps_the_last_complete_snapshot_until_repaired() {
    let sync = unique_temp_dir("sync-parse-failure");
    let laptop = device("sync-parse-failure-laptop", "msg_a", 100);
    let second = laptop.home.join(".claude/projects/project/second.jsonl");
    let valid = r#"{"timestamp":"2026-02-06T11:00:00Z","message":{"id":"msg_b","model":"claude-3-5-sonnet-20241022","stop_reason":"end_turn","usage":{"input_tokens":200,"output_tokens":50}}}
"#;
    write_file(&second, valid);
    let (ok, _, stderr) = run(&laptop, &sync, &["sync", "push", "--timezone", "UTC"]);
    assert!(ok, "{stderr}");
    let path = device_files(&sync).pop().unwrap();
    let complete = fs::read(&path).unwrap();

    // The other sources are absent, which is valid. A discovered damaged
    // source must not replace this complete 400-token snapshot with a subset.
    write_file(&second, "{\"timestamp\":");
    let (ok, stdout, stderr) = run(&laptop, &sync, &["sync", "push", "--timezone", "UTC"]);
    assert!(!ok);
    assert!(!stdout.contains("Wrote"), "{stdout}");
    assert!(stderr.contains("claude: 1 parse error(s)"), "{stderr}");
    assert!(stderr.contains("snapshot was not updated"), "{stderr}");
    assert_eq!(fs::read(&path).unwrap(), complete);

    write_file(&second, &valid.replace("200", "300"));
    let (ok, stdout, stderr) = run(&laptop, &sync, &["sync", "push", "--timezone", "UTC"]);
    assert!(ok, "{stderr}");
    assert!(stdout.contains("Wrote"), "{stdout}");
    let repaired: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert_eq!(repaired["rows"][0]["stats"]["input_tokens"], 400);
    assert_eq!(repaired["rows"][0]["stats"]["output_tokens"], 100);
}

#[test]
fn partial_discovery_keeps_the_complete_snapshot_and_cache_until_repaired() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let codex = root.path().join("codex");
    let data = root.path().join("data");
    let cache = root.path().join("cache");
    let sync = root.path().join("sync");
    fs::create_dir_all(&sync).unwrap();
    let transcript = |id: &str, total: i64| {
        format!(
            "{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"{id}\",\"source\":\"cli\",\"model\":\"gpt-5\"}}}}\n{{\"type\":\"event_msg\",\"timestamp\":\"2026-09-05T12:00:00Z\",\"payload\":{{\"type\":\"token_count\",\"info\":{{\"total_token_usage\":{{\"input_tokens\":{total},\"output_tokens\":0}}}}}}}}\n"
        )
    };
    write_file(
        &codex.join("sessions/active.jsonl"),
        &transcript("active", 100),
    );
    let archived = codex.join("archived_sessions/archive.jsonl");
    write_file(&archived, &transcript("archive", 20));
    let envs = [
        ("HOME", home.as_path()),
        ("CODEX_HOME", codex.as_path()),
        ("XDG_DATA_HOME", data.as_path()),
        ("XDG_CACHE_HOME", cache.as_path()),
    ];
    let push = || {
        run_ccstats(
            &[
                "sync",
                "push",
                "--timezone",
                "UTC",
                "--sync-dir",
                sync.to_str().unwrap(),
            ],
            &envs,
        )
    };
    let (ok, _, err) = push();
    assert!(ok, "{}", String::from_utf8_lossy(&err));
    let snapshot = device_files(&sync).pop().unwrap();
    let complete = fs::read(&snapshot).unwrap();
    let snapshot_input = |bytes: &[u8]| {
        let doc: Value = serde_json::from_slice(bytes).unwrap();
        doc["rows"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r["source"] == "codex")
            .map(|r| r["stats"]["input_tokens"].as_i64().unwrap())
            .sum::<i64>()
    };
    assert_eq!(snapshot_input(&complete), 120);
    let cache_file = cache.join("ccstats/usage-facts-v2.sqlite3");
    let complete_cache = fs::read(&cache_file).unwrap();
    fs::rename(codex.join("sessions"), codex.join("saved-sessions")).unwrap();
    fs::write(codex.join("sessions"), "not a directory").unwrap();
    write_file(&archived, &transcript("archive", 40));
    let (ok, out, err) = push();
    assert!(!ok, "{}", String::from_utf8_lossy(&out));
    let err = String::from_utf8_lossy(&err);
    assert!(err.contains("Cannot discover session files"), "{err}");
    assert!(err.contains("snapshot was not updated"), "{err}");
    assert_eq!(fs::read(&snapshot).unwrap(), complete);
    assert_eq!(fs::read(&cache_file).unwrap(), complete_cache);
    let (ok, report, err) = run_ccstats(
        &[
            "daily",
            "--source",
            "codex",
            "--json",
            "--offline",
            "--no-cost",
        ],
        &envs,
    );
    assert!(ok, "{}", String::from_utf8_lossy(&err));
    let report: Value = serde_json::from_slice(&report).unwrap();
    assert_eq!(report[0]["input_tokens"], 40);
    assert_eq!(report[0]["data_quality"]["parse_errors"], 1);
    assert_eq!(fs::read(&cache_file).unwrap(), complete_cache);
    // A failed discovery with no readable files must also remain incomplete.
    fs::rename(codex.join("archived_sessions"), codex.join("saved-archive")).unwrap();
    fs::write(codex.join("archived_sessions"), "not a directory").unwrap();
    let (ok, _, err) = push();
    assert!(!ok);
    assert!(String::from_utf8_lossy(&err).contains("snapshot was not updated"));
    assert_eq!(fs::read(&snapshot).unwrap(), complete);
    assert_eq!(fs::read(&cache_file).unwrap(), complete_cache);
    fs::remove_file(codex.join("archived_sessions")).unwrap();
    fs::rename(codex.join("saved-archive"), codex.join("archived_sessions")).unwrap();
    fs::remove_file(codex.join("sessions")).unwrap();
    fs::rename(codex.join("saved-sessions"), codex.join("sessions")).unwrap();
    let (ok, _, err) = push();
    assert!(ok, "{}", String::from_utf8_lossy(&err));
    assert_eq!(snapshot_input(&fs::read(&snapshot).unwrap()), 140);
}
