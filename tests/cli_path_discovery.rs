mod common;

#[cfg(windows)]
use common::{RestoredFile, lock_appdata, run_ccstats_with_isolation};
use common::{run_ccstats, unique_temp_dir, write_file};
use serde_json::{Value, json};
use std::path::Path;
#[cfg(windows)]
use std::path::PathBuf;

fn write_claude_session(home: &Path) {
    write_file(
        &home.join(".claude/projects/project/session.jsonl"),
        &json!({"timestamp":"2026-09-07T00:00:00Z","message":{
            "id":"claude-path-test","model":"claude-3-5-sonnet-20241022",
            "stop_reason":"end_turn","usage":{"input_tokens":100,"output_tokens":5}
        }})
        .to_string(),
    );
}

#[cfg(windows)]
fn platform_config_file(name: &str) -> PathBuf {
    dirs::config_dir()
        .expect("platform config directory")
        .join("ccstats")
        .join(name)
}

fn assert_hidden_cost_row(stdout: &[u8], source: &str) {
    let rows: Value = serde_json::from_slice(stdout).expect("JSON daily report");
    assert_eq!(rows.as_array().unwrap().len(), 1, "{source}: {rows}");
    assert_eq!(rows[0]["total_tokens"], 105, "{source}: {rows}");
    assert_eq!(rows[0]["data_quality"]["parse_errors"], 0);
    assert!(
        rows[0].get("cost").is_none(),
        "config must hide costs: {rows}"
    );
}

#[test]
fn home_override_discovers_logs_and_config_in_literal_unicode_directory() {
    let temp = unique_temp_dir("literal-home");
    let home = temp.join("中文 空格 [demo]");
    let xdg_config = home.join(".config");
    let xdg_data = home.join(".local/share");
    let xdg_cache = home.join(".cache");
    write_file(
        &xdg_config.join("ccstats/config.toml"),
        "no_cost = true\noffline = true\n",
    );
    write_claude_session(&home);
    let rows = [
        json!({"type":"session_meta","payload":{"id":"codex-path-test","cwd":r"C:\work\项目","source":"cli"}}),
        json!({"type":"turn_context","payload":{"cwd":r"C:\work\项目","model":"gpt-5"}}),
        json!({"timestamp":"2026-09-07T00:00:00Z","type":"event_msg","payload":{
            "type":"token_count","info":{"total_token_usage":{"input_tokens":100,"output_tokens":5}}
        }}),
    ];
    write_file(
        &home.join(".codex/sessions/session.jsonl"),
        &rows.map(|row| row.to_string()).join("\r\n"),
    );
    for home in [&home, &home.canonicalize().unwrap()] {
        for source in ["claude", "codex"] {
            let (ok, stdout, stderr) = run_ccstats(
                &["daily", "--source", source, "--json", "--timezone", "UTC"],
                &[
                    ("HOME", home),
                    ("XDG_CONFIG_HOME", &xdg_config),
                    ("XDG_DATA_HOME", &xdg_data),
                    ("XDG_CACHE_HOME", &xdg_cache),
                ],
            );
            assert!(ok, "{}", String::from_utf8_lossy(&stderr));
            assert_hidden_cost_row(&stdout, source);
        }
    }
    std::fs::remove_dir_all(temp).unwrap();
}

#[cfg(windows)]
#[test]
fn home_without_xdg_loads_existing_appdata_config() {
    let _lock = lock_appdata();
    let home = unique_temp_dir("appdata-config-home");
    write_claude_session(&home);
    let appdata_config = platform_config_file("config.toml");
    assert_ne!(
        home.join(".config"),
        appdata_config.parent().unwrap().parent().unwrap(),
        "isolated HOME must not be the native config root"
    );
    let _guard = RestoredFile::overwrite(&appdata_config, "no_cost = true\noffline = true\n");
    let (ok, stdout, stderr) = run_ccstats_with_isolation(
        &["daily", "--source", "claude", "--json", "--timezone", "UTC"],
        &[("HOME", home.as_path())],
        false,
    );
    assert!(ok, "{}", String::from_utf8_lossy(&stderr));
    assert_hidden_cost_row(&stdout, "claude");
    assert!(
        !home.join(".config/ccstats/config.toml").exists(),
        "AppData config must be used without copying into HOME"
    );
    let relative_xdg = Path::new(".relative-xdg");
    let (ok, stdout, stderr) = run_ccstats_with_isolation(
        &["daily", "--source", "claude", "--json", "--timezone", "UTC"],
        &[("HOME", home.as_path()), ("XDG_CONFIG_HOME", relative_xdg)],
        false,
    );
    assert!(ok, "{}", String::from_utf8_lossy(&stderr));
    assert_hidden_cost_row(&stdout, "claude");
    std::fs::remove_dir_all(home).unwrap();
}
