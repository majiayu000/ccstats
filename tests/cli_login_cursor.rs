#[allow(dead_code)]
mod common;

use std::fs;
use std::path::{Path, PathBuf};

use common::{run_ccstats, unique_temp_dir};
use serde_json::Value;

const SESSION_TOKEN: &str = "login-cursor-session-secret-167";
const FILE_TOKEN: &str = "file-token-must-not-leak-167";
const ENV_TOKEN: &str = "env-token-must-not-leak-167";

fn credentials_path(root: &Path) -> PathBuf {
    root.join(".config/ccstats/credentials.toml")
}

fn run_isolated(root: &Path, args: &[&str]) -> (bool, Vec<u8>, Vec<u8>) {
    run_isolated_with(root, args, &[])
}

fn run_isolated_with(
    root: &Path,
    args: &[&str],
    extra: &[(&str, &Path)],
) -> (bool, Vec<u8>, Vec<u8>) {
    let xdg = root.join("xdg-config");
    fs::create_dir_all(&xdg).expect("create xdg config home");
    let mut envs = vec![("HOME", root), ("XDG_CONFIG_HOME", xdg.as_path())];
    envs.extend_from_slice(extra);
    run_ccstats(args, &envs)
}

fn cursor_row(stdout: &[u8]) -> Value {
    let rows: Value = serde_json::from_slice(stdout).expect("doctor json");
    rows.as_array()
        .expect("array output")
        .iter()
        .find(|row| row["name"] == "cursor")
        .cloned()
        .expect("Cursor diagnostic")
}

fn assert_no_secret(stdout: &[u8], stderr: &[u8], secret: &str) {
    let stdout = String::from_utf8_lossy(stdout);
    let stderr = String::from_utf8_lossy(stderr);
    assert!(!stdout.contains(secret), "stdout leaked secret: {stdout}");
    assert!(!stderr.contains(secret), "stderr leaked secret: {stderr}");
}

#[test]
fn login_session_token_configures_doctor_without_leaking_secret() {
    let root = unique_temp_dir("login-cursor-session");
    let (ok, stdout, stderr) = run_isolated(
        &root,
        &["login", "cursor", "--session-token", SESSION_TOKEN],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    assert_no_secret(&stdout, &stderr, SESSION_TOKEN);

    let written = fs::read_to_string(credentials_path(&root)).expect("credentials file");
    assert!(written.contains("session_token"));
    assert!(!written.contains("api_key"));

    let (ok, stdout, stderr) = run_isolated(&root, &["doctor", "--json"]);
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    assert_no_secret(&stdout, &stderr, SESSION_TOKEN);
    let cursor = cursor_row(&stdout);
    assert_eq!(cursor["status"], "configured");

    let _ = fs::remove_dir_all(root);
}

#[test]
fn login_writes_unix_0600_credentials_file() {
    let root = unique_temp_dir("login-cursor-mode");
    let (ok, stdout, stderr) = run_isolated(
        &root,
        &["login", "cursor", "--session-token", SESSION_TOKEN],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    assert_no_secret(&stdout, &stderr, SESSION_TOKEN);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(credentials_path(&root))
            .expect("credentials metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }

    let _ = fs::remove_dir_all(root);
}

#[test]
fn env_overrides_file_and_check_reports_env() {
    let root = unique_temp_dir("login-cursor-env");
    let (ok, _, stderr) = run_isolated(&root, &["login", "cursor", "--session-token", FILE_TOKEN]);
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));

    let (ok, stdout, stderr) = run_isolated(&root, &["login", "cursor", "--check"]);
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    let check = format!(
        "{}{}",
        String::from_utf8_lossy(&stdout),
        String::from_utf8_lossy(&stderr)
    );
    assert!(check.contains("file"), "expected file origin: {check}");
    assert!(
        !check.contains("env"),
        "file check should not report env: {check}"
    );
    assert_no_secret(&stdout, &stderr, FILE_TOKEN);

    let env_token = Path::new(ENV_TOKEN);
    let (ok, stdout, stderr) = run_isolated_with(
        &root,
        &["login", "cursor", "--check"],
        &[("CURSOR_SESSION_TOKEN", env_token)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    let check = format!(
        "{}{}",
        String::from_utf8_lossy(&stdout),
        String::from_utf8_lossy(&stderr)
    );
    assert!(check.contains("env"), "expected env origin: {check}");
    assert_no_secret(&stdout, &stderr, FILE_TOKEN);
    assert_no_secret(&stdout, &stderr, ENV_TOKEN);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn both_credential_flags_exit_1() {
    let root = unique_temp_dir("login-cursor-both");
    let (ok, stdout, stderr) = run_isolated(
        &root,
        &[
            "login",
            "cursor",
            "--api-key",
            "api-secret",
            "--session-token",
            "session-secret",
        ],
    );
    assert!(!ok);
    let output = format!(
        "{}{}",
        String::from_utf8_lossy(&stdout),
        String::from_utf8_lossy(&stderr)
    );
    assert!(output.contains("--api-key") && output.contains("--session-token"));
    assert!(!credentials_path(&root).exists());

    let _ = fs::remove_dir_all(root);
}

#[test]
fn clear_removes_file_credentials_and_doctor_is_missing() {
    let root = unique_temp_dir("login-cursor-clear");
    let (ok, _, stderr) = run_isolated(
        &root,
        &["login", "cursor", "--session-token", SESSION_TOKEN],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));

    let (ok, stdout, stderr) = run_isolated(&root, &["login", "cursor", "--clear"]);
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    assert_no_secret(&stdout, &stderr, SESSION_TOKEN);
    assert!(!credentials_path(&root).exists());

    let (ok, stdout, stderr) = run_isolated(&root, &["doctor", "--json"]);
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    assert_no_secret(&stdout, &stderr, SESSION_TOKEN);
    let cursor = cursor_row(&stdout);
    assert_eq!(cursor["status"], "missing");

    let _ = fs::remove_dir_all(root);
}

#[test]
fn no_flags_non_tty_exit_1() {
    let root = unique_temp_dir("login-cursor-nontty");
    let (ok, stdout, stderr) = run_isolated(&root, &["login", "cursor"]);
    assert!(!ok);
    let output = format!(
        "{}{}",
        String::from_utf8_lossy(&stdout),
        String::from_utf8_lossy(&stderr)
    );
    assert!(
        output.contains("--api-key") || output.contains("--session-token"),
        "expected flag guidance: {output}"
    );
    assert!(!credentials_path(&root).exists());

    let _ = fs::remove_dir_all(root);
}

#[test]
fn corrupt_credentials_are_errors_without_secret_disclosure() {
    let root = unique_temp_dir("login-corrupt-file");
    fs::create_dir_all(credentials_path(&root).parent().unwrap()).unwrap();
    fs::write(
        credentials_path(&root),
        format!("[cursor]\nsession_token = \"{FILE_TOKEN}"),
    )
    .unwrap();
    for args in [
        vec!["login", "cursor", "--check"],
        vec!["doctor", "--json"],
        vec!["daily", "-O", "--no-cost"],
    ] {
        let (ok, stdout, stderr) = run_isolated(&root, &args);
        assert!(!ok, "corrupt credentials should fail: {args:?}");
        assert_no_secret(&stdout, &stderr, FILE_TOKEN);
        let output = format!(
            "{}{}",
            String::from_utf8_lossy(&stdout),
            String::from_utf8_lossy(&stderr)
        );
        assert!(output.contains("failed to parse credentials"), "{output}");
    }
    let (ok, stdout, stderr) = run_isolated_with(
        &root,
        &["login", "cursor", "--check"],
        &[("CURSOR_API_KEY", Path::new(ENV_TOKEN))],
    );
    assert!(ok, "environment credentials must override the broken file");
    assert_no_secret(&stdout, &stderr, ENV_TOKEN);
    fs::remove_dir_all(root).unwrap();
}
