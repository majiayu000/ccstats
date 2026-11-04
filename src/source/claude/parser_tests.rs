use super::*;
use serde_json::{Value, json};
fn parse(row: &Value) -> ParseOutput {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session.jsonl");
    std::fs::write(&path, row.to_string()).unwrap();
    parse_claude_file_with_debug(&path, Timezone::Named(chrono_tz::UTC), false)
}
fn row() -> Value {
    json!({"timestamp":"2026-02-06T10:00:00.000Z","message":{"id":"msg","model":"claude-fable-5",
    "usage":{"input_tokens":10,"output_tokens":5}}})
}
#[test]
fn native_usage_normalization_preserves_existing_accounting() {
    let mut value = row();
    value["message"]["usage"] = json!({"input_tokens":-5,"output_tokens":20,
        "cache_creation_input_tokens":30,"cache_creation":{"ephemeral_1h_input_tokens":99}});
    let out = parse(&value);
    assert_eq!(out.errors, 0);
    let entry = &out.entries[0];
    assert_eq!(entry.input_tokens, 0);
    assert_eq!(entry.output_tokens, 20);
    assert_eq!(entry.cache_creation_1h, 30);
    assert_eq!(entry.timestamp, "2026-02-06T10:00:00.000Z");
}
#[test]
fn empty_and_synthetic_models_are_not_usage() {
    for model in ["", "<synthetic>"] {
        let mut value = row();
        value["message"]["model"] = json!(model);
        assert!(parse(&value).entries.is_empty());
    }
    let mut value = row();
    value["message"].as_object_mut().unwrap().remove("model");
    assert_eq!(parse(&value).entries[0].model, UNKNOWN);
}
#[test]
fn missing_usage_and_time_are_skipped_while_malformed_time_is_an_error() {
    let mut value = row();
    value.as_object_mut().unwrap().remove("timestamp");
    assert!(parse(&value).entries.is_empty());
    let mut value = row();
    value["message"].as_object_mut().unwrap().remove("usage");
    assert!(parse(&value).entries.is_empty());
    let mut value = row();
    value["timestamp"] = json!("bad-date");
    assert_eq!(parse(&value).errors, 1);
}
#[test]
fn empty_counters_and_sidechain_usage_are_retained() {
    let mut value = row();
    value["isSidechain"] = json!(true);
    value["message"]["usage"] = json!({});
    let out = parse(&value);
    assert_eq!(out.errors, 0);
    assert_eq!(out.entries.len(), 1);
    assert_eq!(out.entries[0].input_tokens, 0);
}

fn transcript(lines: &[Value]) -> ParseOutput {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s.jsonl");
    let body = lines
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    std::fs::write(&path, body).unwrap();
    parse_claude_file_with_debug(&path, Timezone::Named(chrono_tz::UTC), false)
}
fn user(uuid: &str, parent: Option<&str>, at: &str) -> Value {
    json!({"type":"user","uuid":uuid,"parentUuid":parent,"timestamp":at,
        "message":{"role":"user","content":"prompt"}})
}
fn other(kind: &str, uuid: &str, parent: &str, at: &str) -> Value {
    json!({"type":kind,"uuid":uuid,"parentUuid":parent,"timestamp":at})
}
fn block(uuid: &str, parent: &str, at: &str, id: &str, model: &str, output: i64) -> Value {
    json!({"type":"assistant","uuid":uuid,"parentUuid":parent,"timestamp":at,
        "message":{"id":id,"model":model,"stop_reason":"end_turn",
            "usage":{"input_tokens":1,"output_tokens":output},"content":[]}})
}
/// `(message id suffix, output tokens, duration ms)` of each usage row.
fn timings(out: &ParseOutput) -> Vec<(String, i64, Option<i64>)> {
    out.entries
        .iter()
        .map(|e| {
            let r = e.response.unwrap();
            let id = e.message_id.as_deref().unwrap();
            (
                id[id.len() - 2..].to_string(),
                r.output_tokens,
                r.duration_ms,
            )
        })
        .collect()
}
#[test]
fn responses_run_from_trigger_to_block_and_unreliable_chains_have_no_duration() {
    let out = transcript(&[
        // Prompt -> attachment -> two streamed blocks.
        user("u1", None, "2026-11-01T00:00:00Z"),
        other("attachment", "a1", "u1", "2026-11-01T00:00:01Z"),
        block("b1", "a1", "2026-11-01T00:00:02Z", "m1", "claude-opus", 40),
        block("b2", "b1", "2026-11-01T00:00:04Z", "m1", "claude-opus", 200),
        // Tool execution before the result is not generation time.
        user("t1", Some("b2"), "2026-11-01T00:01:00Z"),
        block("b3", "t1", "2026-11-01T00:01:03Z", "m2", "claude-opus", 300),
        // API retry recorded as system: no start.
        user("u2", Some("b3"), "2026-11-01T00:02:00Z"),
        other("system", "e1", "u2", "2026-11-01T00:02:30Z"),
        block("b4", "e1", "2026-11-01T00:05:00Z", "m3", "claude-opus", 500),
        // A prompt queued during an earlier response starts when that one ends.
        user("q1", Some("b4"), "2026-11-01T00:04:00Z"),
        block(
            "b5",
            "q1",
            "2026-11-01T00:05:01Z",
            "m4",
            "claude-sonnet",
            120,
        ),
        // Synthetic notices are not usage; a block chained to a response has no trigger.
        block("b6", "b5", "2026-11-01T00:06:06Z", "m5", "<synthetic>", 0),
        block("b7", "b5", "2026-11-01T00:06:09Z", "m6", "claude-opus", 90),
        // A streaming snapshot without a stop reason is not the final count.
        user("u3", Some("b7"), "2026-11-01T00:07:00Z"),
        {
            let mut snapshot = block("b8", "u3", "2026-11-01T00:07:01Z", "m7", "claude-opus", 6);
            snapshot["message"]["stop_reason"] = Value::Null;
            snapshot
        },
    ]);
    assert_eq!(out.errors, 0);
    let expected = [
        ("m1", 40, Some(2_000)),
        ("m1", 200, Some(4_000)),
        ("m2", 300, Some(3_000)),
        ("m3", 500, None),
        ("m4", 120, Some(1_000)),
        ("m6", 90, None),
        ("m7", 6, None),
    ];
    assert_eq!(
        timings(&out),
        expected.map(|(id, tokens, ms)| (id.to_string(), tokens, ms))
    );
}
