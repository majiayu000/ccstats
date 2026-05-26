use std::fs;
use std::path::Path;
use std::sync::Mutex;

use ccstats::{SummaryOptions, UsageRange, UsageSource, summarize_cost};
use chrono::NaiveDate;

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn write_file(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent dirs");
    }
    fs::write(path, content).expect("write test file");
}

#[test]
fn sdk_summarizes_codex_cost_without_running_cli() {
    let _guard = ENV_LOCK.lock().expect("env lock");
    let root = tempfile::tempdir().expect("temp dir");
    let codex_home = root.path().join("codex-home");
    let session_file = codex_home.join("sessions").join("sdk-session.jsonl");
    write_file(
        &session_file,
        r#"{"timestamp":"2026-02-06T10:00:00Z","type":"turn_context","payload":{"model":"gpt-5"}}
{"timestamp":"2026-02-06T10:00:00Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":100,"cached_input_tokens":20,"output_tokens":30,"reasoning_output_tokens":10,"total_tokens":140},"last_token_usage":{"input_tokens":100,"cached_input_tokens":20,"output_tokens":30,"reasoning_output_tokens":10,"total_tokens":140},"model":"gpt-5"}}}
"#,
    );

    let previous_codex_home = std::env::var_os("CODEX_HOME");
    unsafe {
        std::env::set_var("CODEX_HOME", &codex_home);
    }

    let summary = summarize_cost(SummaryOptions {
        source: UsageSource::Codex,
        range: UsageRange::DateRange {
            since: Some(NaiveDate::from_ymd_opt(2026, 2, 6).unwrap()),
            until: Some(NaiveDate::from_ymd_opt(2026, 2, 6).unwrap()),
        },
        timezone: Some("UTC".to_string()),
        offline: true,
        ..SummaryOptions::default()
    })
    .expect("summarize codex");

    match previous_codex_home {
        Some(value) => unsafe {
            std::env::set_var("CODEX_HOME", value);
        },
        None => unsafe {
            std::env::remove_var("CODEX_HOME");
        },
    }

    assert_eq!(summary.source, UsageSource::Codex);
    assert_eq!(summary.source_name, "codex");
    assert_eq!(summary.valid_entries, 1);
    assert_eq!(summary.currency, "USD");
    assert_eq!(summary.tokens.input_tokens, 80);
    assert_eq!(summary.tokens.cache_read_tokens, 20);
    assert_eq!(summary.tokens.output_tokens, 20);
    assert_eq!(summary.tokens.reasoning_tokens, 10);
    assert_eq!(summary.tokens.total_tokens, 130);
    assert_eq!(summary.models.len(), 1);
    assert_eq!(summary.models[0].model, "gpt-5");
    assert!(summary.cost_usd.is_some_and(|cost| cost > 0.0));
}

#[test]
fn sdk_summarizes_grok_context_tokens_without_running_cli() {
    let _guard = ENV_LOCK.lock().expect("env lock");
    let root = tempfile::tempdir().expect("temp dir");
    let grok_home = root.path().join("grok-home");
    let session_dir = grok_home
        .join("sessions")
        .join("%2Ftmp%2Fgrok-project")
        .join("sdk-grok-session");
    write_file(
        &session_dir.join("signals.json"),
        r#"{"contextTokensUsed": 1200, "totalTokensBeforeCompaction": 300, "primaryModelId": "grok-build"}"#,
    );
    write_file(
        &session_dir.join("summary.json"),
        r#"{"updated_at": "2026-02-06T10:00:00Z", "current_model_id": "grok-build"}"#,
    );

    let previous_grok_home = std::env::var_os("GROK_HOME");
    unsafe {
        std::env::set_var("GROK_HOME", &grok_home);
    }

    let summary = summarize_cost(SummaryOptions {
        source: UsageSource::Grok,
        range: UsageRange::DateRange {
            since: Some(NaiveDate::from_ymd_opt(2026, 2, 6).unwrap()),
            until: Some(NaiveDate::from_ymd_opt(2026, 2, 6).unwrap()),
        },
        timezone: Some("UTC".to_string()),
        offline: true,
        ..SummaryOptions::default()
    })
    .expect("summarize grok");

    match previous_grok_home {
        Some(value) => unsafe {
            std::env::set_var("GROK_HOME", value);
        },
        None => unsafe {
            std::env::remove_var("GROK_HOME");
        },
    }

    assert_eq!(summary.source, UsageSource::Grok);
    assert_eq!(summary.source_name, "grok");
    assert_eq!(summary.valid_entries, 1);
    assert_eq!(summary.tokens.input_tokens, 1500);
    assert_eq!(summary.tokens.output_tokens, 0);
    assert_eq!(summary.tokens.total_tokens, 1500);
    assert_eq!(summary.models.len(), 1);
    assert_eq!(summary.models[0].model, "grok-build");
    assert!(summary.cost_usd.is_some_and(|cost| cost > 0.0));
}
