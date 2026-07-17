mod common;

use common::{run_ccstats, unique_temp_dir, write_file};
use serde_json::Value;
use std::fs;
use std::path::Path;

const KIMI_SESSION_DIR: &str = "sessions/wd_kimi-proj_6c618ba503c5/session-kimi-1";

fn write_kimi_session(kimi_home: &Path) {
    let session_dir = kimi_home.join(KIMI_SESSION_DIR);
    write_file(
        &kimi_home.join("session_index.jsonl"),
        &format!(
            r#"{{"sessionId":"session-kimi-1","sessionDir":"{}","workDir":"/tmp/kimi-project"}}"#,
            session_dir.display()
        ),
    );
    write_file(
        &session_dir.join("agents/main/wire.jsonl"),
        r#"{"type":"metadata","protocol_version":1,"created_at":"2026-07-17T23:00:00Z"}
{"type":"turn.prompt","time":1784247400000,"input":"hello kimi"}
{"type":"usage.record","model":"kimi-code/k3","usage":{"inputOther":1000,"output":500,"inputCacheRead":200,"inputCacheCreation":300},"usageScope":"turn","time":1784247404495}
{"type":"usage.record","model":"kimi-code/k3","usage":{"inputOther":2000,"output":1000,"inputCacheRead":0,"inputCacheCreation":0},"usageScope":"turn","time":1784247422916}
"#,
    );
}

fn write_kimi_sub_agent_session(kimi_home: &Path) {
    let session_dir = kimi_home.join(KIMI_SESSION_DIR);
    write_file(
        &session_dir.join("agents/agent-0/wire.jsonl"),
        r#"{"type":"usage.record","model":"kimi-code/k3","usage":{"inputOther":7,"output":3,"inputCacheRead":0,"inputCacheCreation":0},"usageScope":"turn","time":1784247430000}
"#,
    );
}

fn assert_close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() < 0.000_001,
        "expected {expected}, got {actual}"
    );
}

#[test]
fn kimi_subcommand_defaults_to_daily() {
    let root = unique_temp_dir("kimi-subcommand");
    let kimi_home = root.join("kimi-home");
    write_kimi_session(&kimi_home);

    let (ok, stdout, stderr) = run_ccstats(
        &[
            "kimi",
            "-j",
            "-O",
            "--timezone",
            "UTC",
            "--since",
            "2026-07-17",
            "--until",
            "2026-07-17",
        ],
        &[("KIMI_CODE_HOME", &kimi_home), ("HOME", &root)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));

    let json: Value = serde_json::from_slice(&stdout).expect("json");
    let arr = json.as_array().expect("array output");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["date"].as_str(), Some("2026-07-17"));
    assert_eq!(arr[0]["input_tokens"].as_i64(), Some(3000));
    assert_eq!(arr[0]["output_tokens"].as_i64(), Some(1500));
    assert_eq!(arr[0]["cache_read_tokens"].as_i64(), Some(200));
    assert_eq!(arr[0]["cache_creation_tokens"].as_i64(), Some(300));
    assert_eq!(arr[0]["total_tokens"].as_i64(), Some(5000));
    assert_eq!(
        arr[0]["models"].as_array().unwrap()[0].as_str(),
        Some("kimi-code/k3")
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn kimi_daily_json_prices_fallback_model() {
    let root = unique_temp_dir("kimi-fallback-json");
    let kimi_home = root.join("kimi-home");
    write_kimi_session(&kimi_home);

    let (ok, stdout, stderr) = run_ccstats(
        &[
            "kimi",
            "daily",
            "-j",
            "-O",
            "--timezone",
            "UTC",
            "--since",
            "2026-07-17",
            "--until",
            "2026-07-17",
        ],
        &[("KIMI_CODE_HOME", &kimi_home), ("HOME", &root)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));

    let json: Value = serde_json::from_slice(&stdout).expect("json");
    let arr = json.as_array().expect("array output");
    assert_eq!(arr[0]["pricing_source"].as_str(), Some("fallback"));
    // Real (non-proxy) usage omits cost_kind/estimated_cost entirely.
    assert!(arr[0].get("cost_kind").is_none());
    assert!(arr[0].get("estimated_cost").is_none());
    // 3000*$0.95/M + 1500*$4/M + 300*$0/M + 200*$0.16/M
    assert_close(arr[0]["cost"].as_f64().unwrap(), 0.008_882);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn source_flag_can_select_kimi_without_subcommand() {
    let root = unique_temp_dir("source-flag-kimi");
    let kimi_home = root.join("kimi-home");
    write_kimi_session(&kimi_home);

    let (ok, stdout, stderr) = run_ccstats(
        &[
            "daily",
            "--source",
            "kimi",
            "-j",
            "-O",
            "--no-cost",
            "--timezone",
            "UTC",
            "--since",
            "2026-07-17",
            "--until",
            "2026-07-17",
        ],
        &[("KIMI_CODE_HOME", &kimi_home), ("HOME", &root)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));

    let json: Value = serde_json::from_slice(&stdout).expect("json");
    let arr = json.as_array().expect("array output");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["date"].as_str(), Some("2026-07-17"));
    assert_eq!(arr[0]["total_tokens"].as_i64(), Some(5000));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn kimi_subcommand_supports_project_view() {
    let root = unique_temp_dir("kimi-subcommand-project");
    let kimi_home = root.join("kimi-home");
    write_kimi_session(&kimi_home);

    let (ok, stdout, stderr) = run_ccstats(
        &[
            "kimi",
            "project",
            "-j",
            "-O",
            "--no-cost",
            "--timezone",
            "UTC",
            "--since",
            "2026-07-17",
            "--until",
            "2026-07-17",
        ],
        &[("KIMI_CODE_HOME", &kimi_home), ("HOME", &root)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));

    let json: Value = serde_json::from_slice(&stdout).expect("json");
    let arr = json.as_array().expect("array output");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["project_path"].as_str(), Some("/tmp/kimi-project"));
    assert_eq!(arr[0]["total_tokens"].as_i64(), Some(5000));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn kimi_counts_sub_agent_usage_in_same_session() {
    let root = unique_temp_dir("kimi-sub-agent");
    let kimi_home = root.join("kimi-home");
    write_kimi_session(&kimi_home);
    write_kimi_sub_agent_session(&kimi_home);

    let (ok, stdout, stderr) = run_ccstats(
        &[
            "kimi",
            "session",
            "-j",
            "-O",
            "--no-cost",
            "--timezone",
            "UTC",
            "--since",
            "2026-07-17",
            "--until",
            "2026-07-17",
        ],
        &[("KIMI_CODE_HOME", &kimi_home), ("HOME", &root)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));

    let json: Value = serde_json::from_slice(&stdout).expect("json");
    let arr = json.as_array().expect("array output");
    assert_eq!(arr.len(), 1, "sub-agent usage shares one session");
    assert_eq!(arr[0]["session_id"].as_str(), Some("session-kimi-1"));
    assert_eq!(arr[0]["total_tokens"].as_i64(), Some(5010));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn kimi_subcommand_conflicts_with_different_source_flag() {
    let root = unique_temp_dir("kimi-source-flag-conflict");
    let (ok, _stdout, stderr) = run_ccstats(
        &["kimi", "daily", "--source", "claude", "-O", "--no-cost"],
        &[("HOME", &root)],
    );
    assert!(!ok, "expected conflict failure");
    let stderr = String::from_utf8_lossy(&stderr);
    assert!(stderr.contains("conflicts with --source"));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn sources_listing_includes_kimi() {
    let root = unique_temp_dir("kimi-sources-listing");
    let (ok, stdout, stderr) = run_ccstats(&["sources", "-j"], &[("HOME", &root)]);
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));

    let json: Value = serde_json::from_slice(&stdout).expect("json");
    let arr = json.as_array().expect("array output");
    let kimi = arr
        .iter()
        .find(|entry| entry["name"].as_str() == Some("kimi"))
        .expect("kimi source row");
    assert_eq!(kimi["display_name"].as_str(), Some("Kimi Code"));
    assert_eq!(kimi["aliases"].as_array().unwrap()[0].as_str(), Some("km"));
    assert_eq!(kimi["capabilities"]["has_projects"].as_bool(), Some(true));
    assert_eq!(
        kimi["capabilities"]["has_cache_creation"].as_bool(),
        Some(true)
    );

    let _ = fs::remove_dir_all(root);
}
