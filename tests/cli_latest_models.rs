mod common;

use common::{run_ccstats, write_file};
use serde_json::{Value, json};
use std::path::Path;

// Minimal standard-rate extract from LiteLLM, checked against the official
// price pages on 2026-10-17. Offline tests exercise the external-cache path too.
fn install_pricing(root: &Path) {
    write_file(
        &root.join("cache/ccstats/pricing.json"),
        include_str!("fixtures/pricing/latest-models.json"),
    );
}

fn claude_row(model: &str, id: usize, input: i64, read: i64, write: i64) -> Value {
    json!({"type":"assistant", "timestamp":"2026-10-17T10:00:00Z",
        "message":{"id":format!("message-{id}"), "model":model,
            "stop_reason":"end_turn", "usage":{
                "input_tokens":input, "output_tokens":50,
                "cache_read_input_tokens":read, "cache_creation_input_tokens":write,
                "cache_creation":{"ephemeral_5m_input_tokens":write - 40,
                    "ephemeral_1h_input_tokens":40}}}})
}

fn report(root: &Path, source: &str, extra: &[&str]) -> Value {
    let mut args = vec![
        "daily",
        "--source",
        source,
        "--json",
        "--offline",
        "--breakdown",
        "--timezone",
        "UTC",
    ];
    args.extend_from_slice(extra);
    let (ok, stdout, stderr) = run_ccstats(
        &args,
        &[
            ("HOME", root),
            ("CLAUDE_CONFIG_DIR", &root.join("claude")),
            ("CODEX_HOME", &root.join("codex")),
            ("XDG_CACHE_HOME", &root.join("cache")),
        ],
    );
    assert!(ok, "{}", String::from_utf8_lossy(&stderr));
    serde_json::from_slice(&stdout).unwrap()
}

fn assert_cost(value: &Value, expected: f64) {
    let actual = value
        .as_f64()
        .unwrap_or_else(|| panic!("unpriced: {value}"));
    assert!(
        (actual - expected).abs() < 1e-9,
        "expected {expected}, got {actual}"
    );
}

fn check_claude(model: &str, rates: [f64; 5]) {
    let root = tempfile::tempdir().unwrap();
    let row = claude_row(model, 0, 1000, 200, 100);
    write_file(
        &root.path().join("claude/projects/project/session.jsonl"),
        &row.to_string(),
    );
    let [input, output, read, write, write_1h] = rates;
    let expected =
        (1000.0 * input + 50.0 * output + 200.0 * read + 60.0 * write + 40.0 * write_1h) / 1e6;
    for extra in [&[][..], &[][..], &["--no-cache"][..]] {
        let result = report(root.path(), "claude", extra);
        assert_eq!(result[0]["total_tokens"], 1350, "{model}: {result}");
        assert_eq!(
            result[0]["breakdown"][0]["model"],
            model.replace("anthropic.", "").replace("claude-", "")
        );
        assert_cost(&result[0]["cost"], expected);
    }
    install_pricing(root.path());
    let result = report(root.path(), "claude", &["--strict-pricing"]);
    assert_cost(&result[0]["cost"], expected);
}

#[test]
fn sonnet_55_uses_its_own_rates() {
    for model in ["claude-sonnet-5-5", "anthropic.claude-sonnet-5-5"] {
        check_claude(model, [2.0, 10.0, 0.1, 2.5, 4.0]);
    }
}

#[test]
fn haiku_55_uses_its_own_rates() {
    check_claude("claude-haiku-5-5", [0.1, 0.5, 0.01, 0.125, 0.2]);
}

#[test]
fn supported_claude_models_keep_tokens_and_cache_ttl_rates() {
    for (model, rates) in [
        ("claude-opus-5-5", [4.0, 20.0, 0.2, 5.0, 8.0]),
        ("claude-fable-5-1", [10.0, 50.0, 0.25, 12.5, 20.0]),
        ("claude-mythos-5-1", [10.0, 50.0, 0.25, 12.5, 20.0]),
        ("claude-opus-4-8", [5.0, 25.0, 0.5, 6.25, 10.0]),
        ("claude-sonnet-5", [2.0, 10.0, 0.2, 2.5, 4.0]),
        ("claude-haiku-4-5", [1.0, 5.0, 0.1, 1.25, 2.0]),
    ] {
        check_claude(model, rates);
    }
}

fn check_codex(model: &str, rates: [f64; 4]) {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("codex/sessions/session.jsonl");
    let meta = json!({"type":"session_meta","payload":{"id":"test-session","source":"cli"}});
    let context = json!({"type":"turn_context","payload":{"model":model}});
    let usage = json!({"input_tokens":1300,"cached_input_tokens":200,
        "cache_write_input_tokens":100,"output_tokens":50,"reasoning_output_tokens":10,
        "total_tokens":1350});
    let count = json!({"type":"event_msg","timestamp":"2026-10-17T10:00:00Z",
        "payload":{"type":"token_count","info":{"total_token_usage":usage,
            "last_token_usage":usage}}});
    let response = json!({"type":"token_usage_record","timestamp":"2026-10-17T10:00:00Z",
        "payload":{"response_id":"response-1","model":model,"usage":usage}});
    let [input, output, read, write] = rates;
    let expected = (1000.0 * input + 50.0 * output + 200.0 * read + 100.0 * write) / 1e6;
    // Both native telemetry formats preserve the model. Replayed counters and
    // coexisting response records must not count the same request twice.
    for rows in [
        vec![&meta, &context, &count, &count, &response],
        vec![&meta, &context, &response],
    ] {
        write_file(
            &path,
            &rows
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n"),
        );
        for extra in [&[][..], &[][..], &["--no-cache"][..]] {
            let result = report(root.path(), "codex", extra);
            assert_eq!(result[0]["total_tokens"], 1350, "{model}: {result}");
            assert_eq!(result[0]["breakdown"][0]["model"], model);
            assert_cost(&result[0]["cost"], expected);
        }
    }
    install_pricing(root.path());
    let result = report(root.path(), "codex", &["--strict-pricing"]);
    assert_cost(&result[0]["cost"], expected);
}

#[test]
fn gpt_61_sol_is_counted_and_priced_in_both_codex_formats() {
    for model in [
        "gpt-6.1-sol",
        "openai/gpt-6.1-sol",
        "gpt-6.1-sol-2026-10-15",
    ] {
        check_codex(model, [2.0, 10.0, 0.1, 2.5]);
    }
}

#[test]
fn supported_codex_models_keep_cache_and_reasoning_accounting() {
    for (model, rates) in [
        ("gpt-6-astra", [10.0, 50.0, 1.0, 12.5]),
        ("gpt-6-sol", [2.0, 10.0, 0.2, 2.5]),
        ("gpt-6-luna", [0.1, 0.5, 0.01, 0.125]),
        ("gpt-5.6-sol", [4.0, 20.0, 0.4, 5.0]),
    ] {
        check_codex(model, rates);
    }
}

#[test]
fn haiku_55_prices_each_request_at_the_100k_boundary() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("claude/projects/project/session.jsonl");
    let mut rows = Vec::new();
    let mut expected = 0.0;
    let mut total = 0;
    for (id, input) in [99_699, 99_700, 99_701, 300_000].into_iter().enumerate() {
        rows.push(claude_row("claude-haiku-5-5", id, input, 200, 100).to_string());
        total += input + 350;
        let multiplier = if input + 300 > 100_000 { 5.0 } else { 1.0 };
        expected += (input as f64 * 0.1 + 50.0 * 0.5 + 200.0 * 0.01 + 60.0 * 0.125 + 40.0 * 0.2)
            * multiplier
            / 1e6;
    }
    write_file(&path, &rows.join("\n"));
    for extra in [&[][..], &[][..], &["--no-cache"][..]] {
        let result = report(root.path(), "claude", extra);
        assert_eq!(result[0]["total_tokens"], total);
        assert_cost(&result[0]["cost"], expected);
    }
    install_pricing(root.path());
    let result = report(root.path(), "claude", &["--strict-pricing"]);
    assert_cost(&result[0]["cost"], expected);
}

#[test]
fn strict_pricing_keeps_unpriced_usage_and_unknown_models() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("claude/projects/project/session.jsonl");
    for model in ["claude-sonnet-5-5", "future-unpriced-model"] {
        write_file(&path, &claude_row(model, 0, 1000, 200, 100).to_string());
        let result = report(root.path(), "claude", &["--strict-pricing"]);
        assert_eq!(result[0]["total_tokens"], 1350);
        assert!(result[0]["cost"].is_null());
    }
    let result = report(root.path(), "claude", &[]);
    assert_eq!(result[0]["total_tokens"], 1350);
    assert!(result[0]["cost"].is_null());
}
