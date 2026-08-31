mod common;

use std::{fs, path::Path};

use common::{run_ccstats, unique_temp_dir, write_file};
use serde_json::Value;

#[test]
fn doctor_reports_detected_and_configured_sources_without_exposing_credentials() {
    let root = unique_temp_dir("doctor-diagnostics");
    let claude_root = root.join("claude-home");
    write_file(&claude_root.join("projects/example/session.jsonl"), "{}\n");
    let cursor_key = Path::new("cursor-secret-must-not-appear");

    let (ok, stdout, stderr) = run_ccstats(
        &["doctor", "--json"],
        &[
            ("HOME", &root),
            ("CLAUDE_CONFIG_DIR", &claude_root),
            ("CURSOR_API_KEY", cursor_key),
        ],
    );

    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    assert!(
        stderr.is_empty(),
        "stderr: {}",
        String::from_utf8_lossy(&stderr)
    );
    let output = String::from_utf8(stdout).expect("utf8 output");
    assert!(!output.contains("cursor-secret-must-not-appear"));

    let rows: Value = serde_json::from_str(&output).expect("doctor json");
    let rows = rows.as_array().expect("array output");

    let claude = rows
        .iter()
        .find(|row| row["name"] == "claude")
        .expect("Claude diagnostic");
    assert_eq!(claude["status"], "detected");
    assert_eq!(claude["files"], 1);

    let cursor = rows
        .iter()
        .find(|row| row["name"] == "cursor")
        .expect("Cursor diagnostic");
    assert_eq!(cursor["status"], "configured");
    assert_eq!(cursor["files"], 0);
    assert!(
        cursor["detail"]
            .as_str()
            .is_some_and(|detail| detail.contains("not contacted"))
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn doctor_table_gives_configured_sources_a_runnable_next_step() {
    let root = unique_temp_dir("doctor-configured");
    let cursor_key = Path::new("configured-cursor-key");
    let (ok, stdout, stderr) = run_ccstats(
        &["doctor"],
        &[("HOME", &root), ("CURSOR_API_KEY", cursor_key)],
    );

    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    let output = String::from_utf8(stdout).expect("utf8 output");
    assert!(output.contains("Next: run `ccstats daily --source all`"));
    assert!(!output.contains("No source data detected"));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn doctor_json_does_not_expose_invalid_cursor_replay_path() {
    let root = unique_temp_dir("doctor-cursor-path");
    let replay = root.join("private/account/replay.json");
    let (ok, stdout, stderr) = run_ccstats(
        &["doctor", "--json"],
        &[("HOME", &root), ("CURSOR_USAGE_FILE", &replay)],
    );

    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    let output = String::from_utf8(stdout).expect("utf8 output");
    assert!(!output.contains(&root.display().to_string()));
    let rows: Value = serde_json::from_str(&output).expect("doctor json");
    let cursor = rows
        .as_array()
        .expect("array output")
        .iter()
        .find(|row| row["name"] == "cursor")
        .expect("Cursor diagnostic");
    assert_eq!(cursor["status"], "missing");
    assert_eq!(
        cursor["detail"],
        "CURSOR_USAGE_FILE is set but does not point to a file"
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn doctor_reports_openclaw_config_errors_instead_of_detecting_the_sentinel() {
    let root = unique_temp_dir("doctor-openclaw-config");
    let config = root.join("broken-openclaw.json");
    write_file(&config, "{broken");
    write_file(&root.join("agents/main/sessions/valid.jsonl"), "{}\n");
    let (ok, stdout, stderr) = run_ccstats(
        &["doctor", "--json"],
        &[
            ("HOME", &root),
            ("OPENCLAW_STATE_DIR", &root),
            ("OPENCLAW_CONFIG_PATH", &config),
        ],
    );

    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    let rows: Value = serde_json::from_slice(&stdout).expect("doctor json");
    let openclaw = rows
        .as_array()
        .expect("array output")
        .iter()
        .find(|row| row["name"] == "openclaw")
        .expect("OpenClaw diagnostic");
    assert_eq!(openclaw["status"], "missing");
    assert_eq!(
        openclaw["detail"],
        "OpenClaw configuration could not be read or parsed"
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn doctor_reports_senpi_config_errors_instead_of_detecting_the_sentinel() {
    let root = unique_temp_dir("doctor-senpi-config");
    let project = root.join("project");
    let workspace = project.join("workspace");
    write_file(&project.join(".senpi/settings.jsonc"), "{broken");
    fs::create_dir_all(&workspace).expect("create workspace");
    let (ok, stdout, stderr) = run_ccstats(
        &["doctor", "--json"],
        &[("HOME", &root), ("CCSTATS_TEST_CWD", &workspace)],
    );

    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    let rows: Value = serde_json::from_slice(&stdout).expect("doctor json");
    let senpi = rows
        .as_array()
        .expect("array output")
        .iter()
        .find(|row| row["name"] == "senpi")
        .expect("Senpi diagnostic");
    assert_eq!(senpi["status"], "missing");
    assert_eq!(senpi["files"], 0);
    assert_eq!(
        senpi["detail"],
        "Senpi configuration could not be read or parsed"
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn doctor_reports_prime_config_errors_instead_of_detecting_the_sentinel() {
    let root = unique_temp_dir("doctor-prime-config");
    write_file(&root.join(".prime/agent/settings.json"), "{broken");
    let (ok, stdout, stderr) = run_ccstats(
        &["doctor", "--json"],
        &[("HOME", &root), ("CCSTATS_TEST_CWD", &root)],
    );

    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    let rows: Value = serde_json::from_slice(&stdout).expect("doctor json");
    let prime = rows
        .as_array()
        .expect("array output")
        .iter()
        .find(|row| row["name"] == "prime")
        .expect("Prime diagnostic");
    assert_eq!(prime["status"], "missing");
    assert_eq!(prime["files"], 0);
    assert_eq!(
        prime["detail"],
        "Prime Agent configuration could not be read or parsed"
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn doctor_reports_invalid_omp_profile_instead_of_detecting_the_sentinel() {
    let root = unique_temp_dir("doctor-omp-profile");
    let invalid_profile = Path::new("../escape");
    let (ok, stdout, stderr) = run_ccstats(
        &["doctor", "--json"],
        &[("HOME", &root), ("OMP_PROFILE", invalid_profile)],
    );

    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    let rows: Value = serde_json::from_slice(&stdout).expect("doctor json");
    let omp = rows
        .as_array()
        .expect("array output")
        .iter()
        .find(|row| row["name"] == "omp")
        .expect("Oh My Pi diagnostic");
    assert_eq!(omp["status"], "missing");
    assert_eq!(omp["files"], 0);
    assert_eq!(omp["detail"], "OMP_PROFILE is invalid");

    let _ = fs::remove_dir_all(root);
}

#[test]
fn doctor_table_gives_a_next_step_when_no_sources_are_detected() {
    let root = unique_temp_dir("doctor-empty");
    let (ok, stdout, stderr) = run_ccstats(&["doctor"], &[("HOME", &root)]);

    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    assert!(
        stderr.is_empty(),
        "stderr: {}",
        String::from_utf8_lossy(&stderr)
    );
    let output = String::from_utf8(stdout).expect("utf8 output");
    assert!(output.contains("No source data detected"));
    assert!(output.contains("ccstats doctor --json"));

    let _ = fs::remove_dir_all(root);
}
