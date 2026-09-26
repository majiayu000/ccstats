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
