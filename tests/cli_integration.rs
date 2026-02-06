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

fn run_ccstats(args: &[&str], envs: &[(&str, &Path)]) -> (bool, Vec<u8>, Vec<u8>) {
    let bin = std::env::var("CARGO_BIN_EXE_ccstats").unwrap_or_else(|_| {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.push("target");
        path.push("debug");
        if cfg!(windows) {
            path.push("ccstats.exe");
        } else {
            path.push("ccstats");
        }
        path.to_string_lossy().into_owned()
    });
    let mut cmd = Command::new(bin);
    cmd.args(args);
    for (k, v) in envs {
        cmd.env(k, v);
    }
    let output = cmd.output().expect("run ccstats");
    (output.status.success(), output.stdout, output.stderr)
}

#[test]
fn codex_daily_json_reads_session_data() {
    let root = unique_temp_dir("codex-daily");
    let codex_home = root.join("codex-home");
    let session_file = codex_home.join("sessions").join("test-session.jsonl");
    write_file(
        &session_file,
        r#"{"timestamp":"2026-02-06T10:00:00Z","type":"turn_context","payload":{"model":"gpt-5"}}
{"timestamp":"2026-02-06T10:00:00Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":100,"cached_input_tokens":20,"output_tokens":30,"reasoning_output_tokens":10,"total_tokens":140},"last_token_usage":{"input_tokens":100,"cached_input_tokens":20,"output_tokens":30,"reasoning_output_tokens":10,"total_tokens":140},"model":"gpt-5"}}}
"#,
    );

    let (ok, stdout, stderr) = run_ccstats(
        &[
            "codex",
            "daily",
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
        &[("CODEX_HOME", &codex_home)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));

    let json: Value = serde_json::from_slice(&stdout).expect("json");
    let arr = json.as_array().expect("array output");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["date"].as_str(), Some("2026-02-06"));
    // OpenAI output_tokens(30) includes reasoning_output_tokens(10).
    // After separating: non_cached_input=80, output=20, reasoning=10, cache_read=20 → total=130
    assert_eq!(arr[0]["total_tokens"].as_i64(), Some(130));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn codex_reasoning_tokens_not_double_counted() {
    let root = unique_temp_dir("codex-reasoning");
    let codex_home = root.join("codex-home");
    let session_file = codex_home.join("sessions").join("reasoning-session.jsonl");
    // OpenAI's output_tokens INCLUDES reasoning_output_tokens as a subset.
    // output_tokens=500 with reasoning_output_tokens=200 means 300 non-reasoning + 200 reasoning.
    write_file(
        &session_file,
        r#"{"timestamp":"2026-02-06T10:00:00Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":1000,"cached_input_tokens":0,"output_tokens":500,"reasoning_output_tokens":200,"total_tokens":1500},"last_token_usage":{"input_tokens":1000,"cached_input_tokens":0,"output_tokens":500,"reasoning_output_tokens":200,"total_tokens":1500},"model":"gpt-5.2-codex"}}}
"#,
    );

    let (ok, stdout, stderr) = run_ccstats(
        &[
            "codex",
            "daily",
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
        &[("CODEX_HOME", &codex_home)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));

    let json: Value = serde_json::from_slice(&stdout).expect("json");
    let arr = json.as_array().expect("array output");
    assert_eq!(arr.len(), 1);
    // output_tokens should be non-reasoning only: 500 - 200 = 300
    assert_eq!(arr[0]["output_tokens"].as_i64(), Some(300));
    // reasoning_tokens should be the separated reasoning portion
    assert_eq!(arr[0]["reasoning_tokens"].as_i64(), Some(200));
    // total = input(1000) + output(300) + reasoning(200) + cache(0) = 1500, no double-counting
    assert_eq!(arr[0]["total_tokens"].as_i64(), Some(1500));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn strict_pricing_sets_unknown_cost_to_null() {
    let root = unique_temp_dir("strict-pricing");
    let codex_home = root.join("codex-home");
    let session_file = codex_home.join("sessions").join("strict-session.jsonl");
    write_file(
        &session_file,
        r#"{"timestamp":"2026-02-06T11:00:00Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":50,"cached_input_tokens":0,"output_tokens":10,"reasoning_output_tokens":0,"total_tokens":60},"last_token_usage":{"input_tokens":50,"cached_input_tokens":0,"output_tokens":10,"reasoning_output_tokens":0,"total_tokens":60},"model":"mystery-model"}}}
"#,
    );

    let (ok, stdout, stderr) = run_ccstats(
        &[
            "codex",
            "daily",
            "-j",
            "-O",
            "--strict-pricing",
            "--timezone",
            "UTC",
            "--since",
            "2026-02-06",
            "--until",
            "2026-02-06",
        ],
        &[("CODEX_HOME", &codex_home)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));

    let json: Value = serde_json::from_slice(&stdout).expect("json");
    let arr = json.as_array().expect("array output");
    assert_eq!(arr.len(), 1);
    assert!(arr[0]["cost"].is_null());

    let _ = fs::remove_dir_all(root);
}

#[test]
fn claude_daily_json_reads_home_projects() {
    let root = unique_temp_dir("claude-daily");
    let claude_file = root.join(".claude/projects/myproject/session-a.jsonl");
    write_file(
        &claude_file,
        r#"{"timestamp":"2026-02-06T12:00:00Z","message":{"id":"msg_1","model":"anthropic.claude-3-5-sonnet-20241022","stop_reason":"end_turn","usage":{"input_tokens":100,"output_tokens":50,"cache_creation_input_tokens":10,"cache_read_input_tokens":20}}}
"#,
    );

    let (ok, stdout, stderr) = run_ccstats(
        &[
            "daily",
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
        &[("HOME", &root)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));

    let json: Value = serde_json::from_slice(&stdout).expect("json");
    let arr = json.as_array().expect("array output");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["date"].as_str(), Some("2026-02-06"));
    assert_eq!(arr[0]["total_tokens"].as_i64(), Some(180));

    let _ = fs::remove_dir_all(root);
}
