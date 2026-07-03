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
    for (key, value) in envs {
        cmd.env(key, value);
    }
    let output = cmd.output().expect("run ccstats");
    (output.status.success(), output.stdout, output.stderr)
}

#[test]
fn tools_command_reads_claude_config_dir_tool_calls() {
    let root = unique_temp_dir("tools-claude-config-dir");
    let claude_config_dir = root.join("custom-claude");
    let home_file = root.join(".claude/projects/home-project/session-a.jsonl");
    let config_file = claude_config_dir.join("projects/config-project/session-a.jsonl");
    write_file(
        &home_file,
        r#"{"type":"assistant","timestamp":"2026-02-06T12:00:00Z","message":{"id":"msg_home","content":[{"type":"tool_use","name":"HomeOnly","id":"tool_home","input":{}}]}}
"#,
    );
    write_file(
        &config_file,
        r#"{"type":"assistant","timestamp":"2026-02-06T12:00:00Z","message":{"id":"msg_config","content":[{"type":"tool_use","name":"Read","id":"tool_config","input":{}}]}}
"#,
    );

    let (ok, stdout, stderr) = run_ccstats(
        &[
            "tools",
            "-j",
            "--timezone",
            "UTC",
            "--since",
            "2026-02-06",
            "--until",
            "2026-02-06",
        ],
        &[("HOME", &root), ("CLAUDE_CONFIG_DIR", &claude_config_dir)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));

    let json: Value = serde_json::from_slice(&stdout).expect("json");
    assert_eq!(json["total"].as_u64(), Some(1));
    let tools = json["tools"].as_array().expect("tools");
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0]["name"].as_str(), Some("Read"));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn tools_command_rejects_source_without_tool_call_capability() {
    let root = unique_temp_dir("tools-capability-gate");
    let codex_home = root.join("codex-home");

    let (ok, stdout, stderr) = run_ccstats(
        &["tools", "--source", "codex"],
        &[("HOME", &root), ("CODEX_HOME", &codex_home)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    assert!(
        String::from_utf8_lossy(&stdout)
            .contains("Tool usage analysis is only supported for Claude source."),
        "stdout: {}",
        String::from_utf8_lossy(&stdout)
    );

    let _ = fs::remove_dir_all(root);
}
