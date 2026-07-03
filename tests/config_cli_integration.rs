use ccstats::{
    MultiSummaryOptions, SummaryOptions, UsageRange, summarize_cost_ranges_with_cli_config,
    summarize_cost_with_cli_config,
};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

static ENV_LOCK: Mutex<()> = Mutex::new(());

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

fn write_config(home: &Path, content: &str) {
    write_file(&home.join(".config/ccstats/config.toml"), content);
}

fn write_codex_session(codex_home: &Path) {
    write_file(
        &codex_home.join("sessions/config-test.jsonl"),
        r#"{"timestamp":"2026-02-06T10:00:00Z","type":"turn_context","payload":{"model":"gpt-5"}}
{"timestamp":"2026-02-06T10:00:00Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":100,"cached_input_tokens":20,"output_tokens":30,"reasoning_output_tokens":10,"total_tokens":130},"last_token_usage":{"input_tokens":100,"cached_input_tokens":20,"output_tokens":30,"reasoning_output_tokens":10,"total_tokens":130},"model":"gpt-5"}}}
"#,
    );
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
    for (key, value) in envs {
        cmd.env(key, value);
    }
    let output = cmd.output().expect("run ccstats");
    (output.status.success(), output.stdout, output.stderr)
}

#[test]
fn invalid_config_toml_fails_daily_command() {
    let root = unique_temp_dir("invalid-config-toml");
    write_config(&root, "offline = true\nthis is not valid toml [[[");

    let (ok, stdout, stderr) = run_ccstats(&["daily", "--no-cost"], &[("HOME", &root)]);

    assert!(!ok);
    assert!(stdout.is_empty());
    let stderr = String::from_utf8_lossy(&stderr);
    assert!(stderr.contains("Error: failed to parse config"));
    assert!(stderr.contains("config.toml"));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn wrong_config_type_fails_statusline_command() {
    let root = unique_temp_dir("wrong-config-type");
    write_config(&root, "strict_pricing = \"yes\"");

    let (ok, stdout, stderr) = run_ccstats(&["statusline"], &[("HOME", &root)]);

    assert!(!ok);
    assert!(stdout.is_empty());
    let stderr = String::from_utf8_lossy(&stderr);
    assert!(stderr.contains("Error: failed to parse config"));
    assert!(stderr.contains("strict_pricing"));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn missing_config_uses_defaults_for_codex_daily() {
    let root = unique_temp_dir("missing-config-defaults");
    let codex_home = root.join("codex-home");
    write_codex_session(&codex_home);

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
        &[("HOME", &root), ("CODEX_HOME", &codex_home)],
    );

    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    let json: Value = serde_json::from_slice(&stdout).expect("json");
    let rows = json.as_array().expect("array output");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["date"].as_str(), Some("2026-02-06"));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn sdk_cli_config_helpers_fail_on_invalid_config() {
    let _guard = ENV_LOCK.lock().expect("env lock");
    let root = unique_temp_dir("sdk-invalid-config");
    write_config(&root, "timezone = 123");

    let previous_home = std::env::var_os("HOME");
    unsafe {
        std::env::set_var("HOME", &root);
    }

    let single = summarize_cost_with_cli_config(SummaryOptions::default())
        .expect_err("single-range SDK helper should fail");
    assert!(single.to_string().contains("failed to parse config"));

    let multi = summarize_cost_ranges_with_cli_config(MultiSummaryOptions {
        ranges: vec![UsageRange::Today],
        ..MultiSummaryOptions::default()
    })
    .expect_err("multi-range SDK helper should fail");
    assert!(multi.to_string().contains("failed to parse config"));

    match previous_home {
        Some(value) => unsafe {
            std::env::set_var("HOME", value);
        },
        None => unsafe {
            std::env::remove_var("HOME");
        },
    }
    let _ = fs::remove_dir_all(root);
}
