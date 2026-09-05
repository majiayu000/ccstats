mod common;

use chrono::Utc;
use common::{run_ccstats, unique_temp_dir, write_file};
use serde_json::Value;
use std::fs;
use std::path::Path;

fn write_codex_session(codex_home: &Path, timestamp: &str, input: i64) {
    write_file(
        &codex_home.join("sessions").join("session.jsonl"),
        &format!(
            r#"{{"timestamp":"{timestamp}","type":"event_msg","payload":{{"type":"token_count","info":{{"total_token_usage":{{"input_tokens":{input},"cached_input_tokens":0,"output_tokens":5,"reasoning_output_tokens":0,"total_tokens":{total}}},"last_token_usage":{{"input_tokens":{input},"cached_input_tokens":0,"output_tokens":5,"reasoning_output_tokens":0,"total_tokens":{total}}},"model":"gpt-5"}}}}}}
"#,
            total = input + 5
        ),
    );
}

fn write_claude_session(root: &Path, timestamp: &str) {
    write_file(
        &root.join(".claude/projects/myapp/claude-session.jsonl"),
        &format!(
            r#"{{"timestamp":"{timestamp}","message":{{"id":"msg_1","model":"claude-3-5-sonnet-20241022","stop_reason":"end_turn","usage":{{"input_tokens":100,"output_tokens":50,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}}}}
"#
        ),
    );
}

fn json_total_tokens(stdout: &[u8]) -> i64 {
    let json: Value = serde_json::from_slice(stdout).expect("json");
    json.as_array().expect("array output")[0]["total_tokens"]
        .as_i64()
        .expect("total_tokens")
}

fn assert_doctor_diagnostics(stdout: &[u8]) {
    let output = String::from_utf8_lossy(stdout);
    assert!(
        output.contains("Source diagnostics"),
        "expected doctor table, got: {output}"
    );
    assert!(
        output.contains("No source data detected"),
        "expected empty-source hint, got: {output}"
    );
    assert!(
        !output.contains("Token Usage"),
        "Claude period table leaked into empty auto-detect: {output}"
    );
    assert!(
        !output.contains("No Claude Code usage data found"),
        "default Claude empty table leaked: {output}"
    );
}

#[test]
fn empty_home_daily_prints_doctor_not_claude_table() {
    let root = unique_temp_dir("auto-detect-empty-daily");
    let (ok, stdout, stderr) = run_ccstats(&["daily", "-O", "--no-cost"], &[("HOME", &root)]);
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    assert_doctor_diagnostics(&stdout);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn empty_home_default_command_prints_doctor_not_claude_table() {
    let root = unique_temp_dir("auto-detect-empty-default");
    let (ok, stdout, stderr) = run_ccstats(&["-O", "--no-cost"], &[("HOME", &root)]);
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    assert_doctor_diagnostics(&stdout);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn empty_home_daily_json_uses_doctor_schema() {
    let root = unique_temp_dir("auto-detect-empty-json");
    let (ok, stdout, stderr) = run_ccstats(&["daily", "-j", "-O", "--no-cost"], &[("HOME", &root)]);
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    let json: Value = serde_json::from_slice(&stdout).expect("doctor json");
    let rows = json.as_array().expect("doctor array");
    assert!(rows.iter().any(|row| row["name"] == "claude"));
    assert!(rows.iter().any(|row| row["name"] == "codex"));
    for row in rows {
        assert!(
            row.get("status").is_some(),
            "doctor row missing status: {row}"
        );
        assert!(
            row.get("setup").is_some(),
            "doctor row missing setup: {row}"
        );
        assert!(
            row.get("date").is_none(),
            "usage schema mixed into doctor json: {row}"
        );
        assert!(
            row.get("total_tokens").is_none(),
            "usage totals mixed into doctor json: {row}"
        );
    }
    let _ = fs::remove_dir_all(root);
}

#[test]
fn empty_home_statusline_is_quiet_single_line() {
    let root = unique_temp_dir("auto-detect-empty-statusline");
    let (ok, stdout, stderr) = run_ccstats(&["statusline"], &[("HOME", &root)]);
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    let output = String::from_utf8(stdout).expect("utf8 stdout");
    assert!(
        !output.contains("Source diagnostics"),
        "statusline dumped doctor: {output}"
    );
    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(
        lines.len(),
        1,
        "expected a single quiet line, got: {output:?}"
    );
    assert!(
        lines[0].len() < 80,
        "statusline line should stay short: {output:?}"
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn codex_only_today_matches_explicit_codex_source() {
    let root = unique_temp_dir("auto-detect-codex-only");
    let codex_home = root.join("codex-home");
    let timestamp = Utc::now().format("%Y-%m-%dT12:00:00Z").to_string();
    write_codex_session(&codex_home, &timestamp, 10);

    let args_auto = ["today", "-j", "-O", "--no-cost", "--timezone", "UTC"];
    let args_explicit = [
        "today",
        "--source",
        "codex",
        "-j",
        "-O",
        "--no-cost",
        "--timezone",
        "UTC",
    ];
    let envs = [
        ("HOME", root.as_path()),
        ("CODEX_HOME", codex_home.as_path()),
    ];

    let (ok, auto_stdout, stderr) = run_ccstats(&args_auto, &envs);
    assert!(ok, "auto stderr: {}", String::from_utf8_lossy(&stderr));
    let (ok, explicit_stdout, stderr) = run_ccstats(&args_explicit, &envs);
    assert!(ok, "explicit stderr: {}", String::from_utf8_lossy(&stderr));

    assert_eq!(
        json_total_tokens(&auto_stdout),
        json_total_tokens(&explicit_stdout)
    );
    assert_eq!(json_total_tokens(&auto_stdout), 15);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn claude_and_codex_daily_uses_all_sources_path() {
    let root = unique_temp_dir("auto-detect-claude-codex");
    let codex_home = root.join("codex-home");
    write_claude_session(&root, "2026-02-06T10:00:00Z");
    write_codex_session(&codex_home, "2026-02-06T11:00:00Z", 10);

    let period_args = [
        "-j",
        "-O",
        "--no-cost",
        "--timezone",
        "UTC",
        "--since",
        "2026-02-06",
        "--until",
        "2026-02-06",
    ];
    let envs = [
        ("HOME", root.as_path()),
        ("CODEX_HOME", codex_home.as_path()),
    ];

    let mut auto_args = vec!["daily"];
    auto_args.extend_from_slice(&period_args);
    let (ok, auto_stdout, stderr) = run_ccstats(&auto_args, &envs);
    assert!(ok, "auto stderr: {}", String::from_utf8_lossy(&stderr));

    let mut all_args = vec!["daily", "--source", "all"];
    all_args.extend_from_slice(&period_args);
    let (ok, all_stdout, stderr) = run_ccstats(&all_args, &envs);
    assert!(ok, "all stderr: {}", String::from_utf8_lossy(&stderr));

    assert_eq!(
        json_total_tokens(&auto_stdout),
        json_total_tokens(&all_stdout)
    );
    assert_eq!(json_total_tokens(&auto_stdout), 165);

    let mut breakdown_args = vec!["daily", "--source-breakdown"];
    breakdown_args.extend_from_slice(&period_args);
    let (ok, breakdown_stdout, stderr) = run_ccstats(&breakdown_args, &envs);
    assert!(
        ok,
        "auto-detect two sources should allow --source-breakdown: {}",
        String::from_utf8_lossy(&stderr)
    );
    let json: Value = serde_json::from_slice(&breakdown_stdout).expect("breakdown json");
    assert!(json.is_object(), "all-sources breakdown wraps an object");
    let names: Vec<&str> = json["sources"]
        .as_array()
        .expect("sources")
        .iter()
        .filter_map(|entry| entry["source"].as_str())
        .collect();
    assert_eq!(names, vec!["claude", "codex"]);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn explicit_claude_source_does_not_switch_to_codex() {
    let root = unique_temp_dir("auto-detect-claude-override");
    let codex_home = root.join("codex-home");
    write_codex_session(&codex_home, "2026-02-06T11:00:00Z", 10);

    let (ok, stdout, stderr) = run_ccstats(
        &[
            "daily",
            "--source",
            "claude",
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
    let output = String::from_utf8_lossy(&stdout);
    assert!(
        output.contains("No Claude Code usage data found"),
        "expected Claude empty hint, got: {output}"
    );
    assert!(
        !output.contains("\"total_tokens\":15"),
        "explicit Claude switched to Codex: {output}"
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn config_source_pins_codex_on_a_multi_source_machine() {
    let root = unique_temp_dir("auto-detect-config-source");
    let codex_home = root.join("codex-home");
    write_claude_session(&root, "2026-02-06T10:00:00Z");
    write_codex_session(&codex_home, "2026-02-06T11:00:00Z", 10);
    write_file(
        &root.join(".config/ccstats/config.toml"),
        "source = \"codex\"\n",
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
        &[("HOME", &root), ("CODEX_HOME", &codex_home)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    assert_eq!(json_total_tokens(&stdout), 15);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn explicit_source_all_uses_all_path_with_one_ready_source() {
    let root = unique_temp_dir("auto-detect-explicit-all");
    let codex_home = root.join("codex-home");
    write_codex_session(&codex_home, "2026-02-06T11:00:00Z", 10);

    let (ok, stdout, stderr) = run_ccstats(
        &[
            "daily",
            "--source",
            "all",
            "--source-breakdown",
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
    let json: Value = serde_json::from_slice(&stdout).expect("breakdown json");
    assert!(json.is_object());
    let names: Vec<&str> = json["sources"]
        .as_array()
        .expect("sources")
        .iter()
        .filter_map(|entry| entry["source"].as_str())
        .collect();
    assert_eq!(names, vec!["codex"]);

    let (ok, _stdout, stderr) = run_ccstats(
        &[
            "daily",
            "--source-breakdown",
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
    assert!(
        !ok,
        "one ready source must not auto-select all for --source-breakdown"
    );
    assert!(
        String::from_utf8_lossy(&stderr).contains("--source-breakdown requires --source all"),
        "stderr: {}",
        String::from_utf8_lossy(&stderr)
    );

    let _ = fs::remove_dir_all(root);
}
