mod common;

use std::fs;
use std::path::Path;

use chrono::{SecondsFormat, Timelike, Utc};
use common::{run_ccstats, run_ccstats_with_stdin, unique_temp_dir, write_file};
use serde_json::{Value, json};

fn write_claude_entry(home: &Path) {
    let timestamp = Utc::now()
        .with_nanosecond(0)
        .unwrap()
        .to_rfc3339_opts(SecondsFormat::Secs, true);
    write_file(
        &home.join(".claude/projects/mcp-app/session.jsonl"),
        &format!(
            r#"{{"timestamp":"{timestamp}","message":{{"id":"msg_mcp_1","model":"claude-3-5-sonnet-20241022","stop_reason":"end_turn","usage":{{"input_tokens":100,"output_tokens":50,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}}}}
"#
        ),
    );
}

fn request(id: i64, method: &str, params: &Value) -> String {
    json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }).to_string()
}

/// Run one stdio session and return every stdout line parsed as JSON-RPC.
fn run_session(home: &Path, messages: &[String]) -> Vec<Value> {
    let mut stdin = messages.join("\n");
    stdin.push('\n');
    let (ok, stdout, stderr) = run_ccstats_with_stdin(
        &["mcp", "--offline", "--timezone", "UTC"],
        &[("HOME", home)],
        &stdin,
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    String::from_utf8(stdout)
        .expect("utf-8 stdout")
        .lines()
        .map(|line| {
            let value: Value = serde_json::from_str(line)
                .unwrap_or_else(|error| panic!("non-protocol stdout line {line:?}: {error}"));
            assert_eq!(value["jsonrpc"], "2.0", "{line}");
            value
        })
        .collect()
}

fn tool_payload(response: &Value) -> Value {
    let result = &response["result"];
    assert_eq!(result["isError"], false, "{response}");
    let text = result["content"][0]["text"].as_str().expect("text content");
    serde_json::from_str(text).expect("tool text is JSON")
}

#[test]
fn mcp_stdio_session_lists_and_calls_tools() {
    let home = unique_temp_dir("mcp-session");
    write_claude_entry(&home);

    let responses = run_session(
        &home,
        &[
            request(
                1,
                "initialize",
                &json!({
                    "protocolVersion": "2025-06-18",
                    "capabilities": {},
                    "clientInfo": { "name": "test", "version": "0" },
                }),
            ),
            json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }).to_string(),
            request(2, "tools/list", &json!({})),
            request(
                3,
                "tools/call",
                &json!({ "name": "get_usage_summary", "arguments": { "source": "claude", "period": "today" } }),
            ),
            request(
                4,
                "tools/call",
                &json!({ "name": "get_limits", "arguments": { "source": "claude" } }),
            ),
            request(5, "tools/call", &json!({ "name": "doctor" })),
            request(
                6,
                "tools/call",
                &json!({ "name": "get_usage_summary", "arguments": { "source": "claude", "period": "year" } }),
            ),
            request(7, "tools/call", &json!({ "name": "nope" })),
            request(8, "resources/list", &json!({})),
            "not json".to_string(),
        ],
    );

    // The notification gets no reply; every request plus the parse error does.
    let ids: Vec<Value> = responses.iter().map(|r| r["id"].clone()).collect();
    assert_eq!(
        ids,
        [
            json!(1),
            json!(2),
            json!(3),
            json!(4),
            json!(5),
            json!(6),
            json!(7),
            json!(8),
            Value::Null
        ]
    );

    let init = &responses[0]["result"];
    assert_eq!(init["protocolVersion"], "2025-06-18");
    assert_eq!(init["serverInfo"]["name"], "ccstats");
    assert!(init["capabilities"]["tools"].is_object());

    let tools: Vec<&str> = responses[1]["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| {
            assert_eq!(tool["inputSchema"]["type"], "object");
            tool["name"].as_str().unwrap()
        })
        .collect();
    assert_eq!(
        tools,
        ["get_limits", "get_usage_summary", "diagnose", "doctor"]
    );

    let summary = tool_payload(&responses[2]);
    assert_eq!(summary["source_name"], "claude");
    assert_eq!(summary["tokens"]["input_tokens"], 100);
    assert_eq!(summary["tokens"]["output_tokens"], 50);
    assert_eq!(responses[2]["result"]["structuredContent"], summary);

    let limits = tool_payload(&responses[3]);
    assert!(limits["codex"].is_null(), "{limits}");
    assert_eq!(limits["claude_blocks"]["total_tokens"], 150);
    assert!(limits["windows"].is_array());

    let doctor = tool_payload(&responses[4]);
    assert!(
        doctor
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["name"] == "claude" && row["status"] == "detected"),
        "{doctor}"
    );

    assert_eq!(responses[5]["result"]["isError"], true);
    assert_eq!(responses[6]["error"]["code"], -32602);
    assert_eq!(responses[7]["error"]["code"], -32601);
    assert_eq!(responses[8]["error"]["code"], -32700);

    let _ = fs::remove_dir_all(home);
}

#[test]
fn mcp_exits_cleanly_on_stdin_eof_without_stdout() {
    let home = unique_temp_dir("mcp-eof");
    let (ok, stdout, stderr) = run_ccstats(&["mcp", "--offline"], &[("HOME", &home)]);
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    assert!(stdout.is_empty(), "{}", String::from_utf8_lossy(&stdout));
    let _ = fs::remove_dir_all(home);
}

#[test]
fn diagnose_mcp_returns_cli_shape_and_preserves_tool_error_contract() {
    let home = unique_temp_dir("mcp-diagnose");
    write_claude_entry(&home);
    let responses = run_session(
        &home,
        &[
            request(
                1,
                "tools/call",
                &json!({"name":"diagnose","arguments":{"window":"5h"}}),
            ),
            request(
                2,
                "tools/call",
                &json!({"name":"diagnose","arguments":{"window":"yesterday"}}),
            ),
            request(
                3,
                "tools/call",
                &json!({"name":"diagnose","arguments":{"session":"missing"}}),
            ),
            request(
                4,
                "tools/call",
                &json!({"name":"diagnose","arguments":{"window":5}}),
            ),
            request(
                5,
                "tools/call",
                &json!({"name":"diagnose","arguments":{"extra":true}}),
            ),
        ],
    );
    let payload = tool_payload(&responses[0]);
    assert_eq!(payload["source"], "claude");
    assert_eq!(payload["usage"]["total_tokens"], 150);
    assert_eq!(responses[0]["result"]["structuredContent"], payload);
    let (ok, stdout, stderr) = run_ccstats(
        &["diagnose", "--json", "--offline", "--timezone", "UTC"],
        &[("HOME", &home)],
    );
    assert!(ok, "{}", String::from_utf8_lossy(&stderr));
    let cli: Value = serde_json::from_slice(&stdout).unwrap();
    assert_eq!(payload["usage"], cli["usage"]);
    assert_eq!(payload["versions"], cli["versions"]);
    for response in &responses[1..] {
        assert_eq!(response["result"]["isError"], true, "{response}");
        assert!(response.get("error").is_none());
    }
    fs::remove_dir_all(home).unwrap();
}
