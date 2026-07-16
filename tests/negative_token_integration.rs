use rusqlite::Connection;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn unique_temp_dir(prefix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("ccstats-{prefix}-{}-{nanos}", std::process::id()));
    fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn write_file(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent dirs");
    }
    fs::write(path, content).expect("write test file");
}

fn resolve_ccstats_binary() -> PathBuf {
    if let Some(bin) = std::env::var_os("CARGO_BIN_EXE_ccstats") {
        return PathBuf::from(bin);
    }

    let bin_name = if cfg!(windows) {
        "ccstats.exe"
    } else {
        "ccstats"
    };
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let candidates = [
        manifest_dir
            .join("target")
            .join("llvm-cov-target")
            .join("debug")
            .join(bin_name),
        manifest_dir.join("target").join("debug").join(bin_name),
    ];

    candidates
        .into_iter()
        .find(|path| path.is_file())
        .unwrap_or_else(|| panic!("unable to locate ccstats binary"))
}

fn run_ccstats(args: &[&str], envs: &[(&str, &Path)]) -> (bool, Vec<u8>, Vec<u8>) {
    let mut cmd = Command::new(resolve_ccstats_binary());
    cmd.args(args);
    for (k, v) in envs {
        cmd.env(k, v);
    }
    let output = cmd.output().expect("run ccstats");
    (output.status.success(), output.stdout, output.stderr)
}

fn write_cursor_negative_state_db(path: &Path) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create cursor db parent");
    }
    let conn = Connection::open(path).expect("open cursor db");
    conn.execute(
        "CREATE TABLE cursorDiskKV (key TEXT PRIMARY KEY, value BLOB)",
        [],
    )
    .expect("create cursorDiskKV");
    conn.execute(
        "CREATE TABLE ItemTable (key TEXT PRIMARY KEY, value BLOB)",
        [],
    )
    .expect("create ItemTable");
    conn.execute(
        "INSERT INTO cursorDiskKV (key, value) VALUES (?1, ?2)",
        (
            "composerData:composer-1",
            r#"{"composerId":"composer-1","modelConfig":{"modelName":"gpt-4o-mini"},"workspaceIdentifier":{"uri":{"fsPath":"/tmp/cursor-project"}}}"#,
        ),
    )
    .expect("insert composer");
    conn.execute(
        "INSERT INTO cursorDiskKV (key, value) VALUES (?1, ?2)",
        (
            "bubbleId:composer-1:bubble-1",
            r#"{"createdAt":"2026-02-06T10:30:00Z","tokenCount":{"inputTokens":-25,"outputTokens":40}}"#,
        ),
    )
    .expect("insert mixed bubble");
    conn.execute(
        "INSERT INTO cursorDiskKV (key, value) VALUES (?1, ?2)",
        (
            "bubbleId:composer-1:bubble-negative",
            r#"{"createdAt":"2026-02-06T10:31:00Z","tokenCount":{"inputTokens":-7,"outputTokens":-3}}"#,
        ),
    )
    .expect("insert all-negative bubble");
    conn.execute(
        "INSERT INTO ItemTable (key, value) VALUES (?1, ?2)",
        (
            "aiService.generations",
            r#"[{"createdAt":"2026-02-06T11:00:00Z","generationUUID":"generation-1","model":"gpt-4o-mini","inputTokens":25,"outputTokens":-5},{"createdAt":"2026-02-06T11:05:00Z","generationUUID":"generation-negative","model":"gpt-4o-mini","inputTokens":-11,"outputTokens":-2}]"#,
        ),
    )
    .expect("insert generations");
}

#[test]
fn all_sources_daily_json_clamps_negative_claude_and_cursor_tokens() {
    let root = unique_temp_dir("negative-token-json");
    let codex_home = root.join("codex-home");
    let grok_home = root.join("grok-home");
    let cursor_home = root.join("cursor-user");
    let claude_file = root.join(".claude/projects/myapp/session-negative.jsonl");

    write_file(
        &claude_file,
        r#"{"timestamp":"2026-02-06T10:00:00Z","message":{"id":"msg_mixed","model":"claude-3-5-sonnet-20241022","stop_reason":"end_turn","usage":{"input_tokens":-100,"output_tokens":50,"cache_creation_input_tokens":-30,"cache_read_input_tokens":20}}}
{"timestamp":"2026-02-06T10:01:00Z","message":{"id":"msg_negative","model":"claude-3-5-sonnet-20241022","stop_reason":"end_turn","usage":{"input_tokens":-10,"output_tokens":-5,"cache_creation_input_tokens":-3,"cache_read_input_tokens":-2}}}
"#,
    );
    write_cursor_negative_state_db(&cursor_home.join("globalStorage").join("state.vscdb"));

    let (ok, stdout, stderr) = run_ccstats(
        &[
            "daily",
            "--source",
            "all",
            "-j",
            "-O",
            "--no-cost",
            "--timezone",
            "UTC",
            "--since",
            "2026-02-06",
            "--until",
            "2026-02-06",
        ],
        &[
            ("HOME", &root),
            ("CODEX_HOME", &codex_home),
            ("CURSOR_HOME", &cursor_home),
            ("GROK_HOME", &grok_home),
        ],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));

    let json: Value = serde_json::from_slice(&stdout).expect("json");
    let arr = json.as_array().expect("array output");
    assert_eq!(arr.len(), 1);
    let row = &arr[0];
    assert_eq!(row["date"].as_str(), Some("2026-02-06"));
    assert_eq!(row["input_tokens"].as_i64(), Some(25));
    assert_eq!(row["output_tokens"].as_i64(), Some(90));
    assert_eq!(row["cache_creation_tokens"].as_i64(), Some(0));
    assert_eq!(row["cache_read_tokens"].as_i64(), Some(20));
    assert!(row["cache_hit_rate"].is_null());
    assert_eq!(row["total_tokens"].as_i64(), Some(135));

    for key in [
        "input_tokens",
        "output_tokens",
        "cache_creation_tokens",
        "cache_read_tokens",
        "total_tokens",
    ] {
        assert!(
            row[key].as_i64().expect("numeric token field") >= 0,
            "{key} should be non-negative"
        );
    }

    let _ = fs::remove_dir_all(root);
}
