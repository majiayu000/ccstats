mod common;

use common::{run_ccstats, unique_temp_dir, write_file};
use serde_json::{Value, json};

#[test]
fn home_override_discovers_logs_and_config_in_literal_unicode_directory() {
    let temp = unique_temp_dir("literal-home");
    let home = temp.join("中文 空格 [demo]");
    write_file(
        &home.join(".config/ccstats/config.toml"),
        "no_cost = true\noffline = true\n",
    );
    write_file(
        &home.join(".claude/projects/project/session.jsonl"),
        &json!({"timestamp":"2026-09-07T00:00:00Z","message":{
            "id":"claude-path-test","model":"claude-3-5-sonnet-20241022",
            "stop_reason":"end_turn","usage":{"input_tokens":100,"output_tokens":5}
        }})
        .to_string(),
    );
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
                &[("HOME", home)],
            );
            assert!(ok, "{}", String::from_utf8_lossy(&stderr));
            let rows: Value = serde_json::from_slice(&stdout).expect("JSON daily report");
            assert_eq!(rows.as_array().unwrap().len(), 1, "{source}: {rows}");
            assert_eq!(rows[0]["total_tokens"], 105, "{source}: {rows}");
            assert_eq!(rows[0]["data_quality"]["parse_errors"], 0);
            assert!(
                rows[0].get("cost").is_none(),
                "HOME config must hide costs: {rows}"
            );
        }
    }
    std::fs::remove_dir_all(temp).unwrap();
}
