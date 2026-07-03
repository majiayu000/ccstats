use std::fs;
use std::path::Path;
use std::sync::Mutex;

use ccstats::{
    CostSummary, MultiSummaryOptions, SummaryOptions, UsageRange, UsageSource, summarize_cost,
    summarize_cost_ranges,
};
use chrono::{Datelike, Days, NaiveDate, Utc};

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn write_file(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent dirs");
    }
    fs::write(path, content).expect("write test file");
}

fn write_pricing_cache(home: &Path, xdg_cache: &Path, contents: &str) {
    write_file(&xdg_cache.join("ccstats/pricing.json"), contents);
    write_file(&home.join("Library/Caches/ccstats/pricing.json"), contents);
    write_file(&home.join(".cache/ccstats/pricing.json"), contents);
}

fn assert_stable_summary_eq(actual: &CostSummary, expected: &CostSummary) {
    assert_eq!(actual.source, expected.source);
    assert_eq!(actual.source_name, expected.source_name);
    assert_eq!(actual.display_name, expected.display_name);
    assert_eq!(actual.range, expected.range);
    assert_eq!(actual.since, expected.since);
    assert_eq!(actual.until, expected.until);
    assert_eq!(actual.currency, expected.currency);
    assert_eq!(actual.cost, expected.cost);
    assert_eq!(actual.cost_usd, expected.cost_usd);
    assert_eq!(actual.tokens, expected.tokens);
    assert_eq!(actual.models, expected.models);
    assert_eq!(actual.valid_entries, expected.valid_entries);
    assert_eq!(actual.skipped_entries, expected.skipped_entries);
    assert_eq!(actual.parse_error_entries, expected.parse_error_entries);
    assert!(actual.elapsed_ms.is_finite());
}

#[test]
fn sdk_offline_corrupt_pricing_cache_returns_error() {
    let _guard = ENV_LOCK.lock().expect("env lock");
    let root = tempfile::tempdir().expect("temp dir");
    let xdg_cache = root.path().join("xdg-cache");
    let codex_home = root.path().join("codex-home");
    write_pricing_cache(root.path(), &xdg_cache, "{not json");

    let previous_home = std::env::var_os("HOME");
    let previous_xdg_cache = std::env::var_os("XDG_CACHE_HOME");
    let previous_codex_home = std::env::var_os("CODEX_HOME");
    unsafe {
        std::env::set_var("HOME", root.path());
        std::env::set_var("XDG_CACHE_HOME", &xdg_cache);
        std::env::set_var("CODEX_HOME", &codex_home);
    }

    let error = summarize_cost(SummaryOptions {
        source: UsageSource::Codex,
        offline: true,
        ..SummaryOptions::default()
    })
    .expect_err("corrupt offline pricing cache should return SDK error");

    match previous_home {
        Some(value) => unsafe {
            std::env::set_var("HOME", value);
        },
        None => unsafe {
            std::env::remove_var("HOME");
        },
    }
    match previous_xdg_cache {
        Some(value) => unsafe {
            std::env::set_var("XDG_CACHE_HOME", value);
        },
        None => unsafe {
            std::env::remove_var("XDG_CACHE_HOME");
        },
    }
    match previous_codex_home {
        Some(value) => unsafe {
            std::env::set_var("CODEX_HOME", value);
        },
        None => unsafe {
            std::env::remove_var("CODEX_HOME");
        },
    }

    let message = error.to_string();
    assert!(message.contains("pricing cache"), "{message}");
    assert!(message.contains("malformed"), "{message}");
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
fn sdk_batch_summarizes_codex_ranges_like_repeated_single_calls() {
    let _guard = ENV_LOCK.lock().expect("env lock");
    let root = tempfile::tempdir().expect("temp dir");
    let codex_home = root.path().join("codex-home");
    let session_file = codex_home.join("sessions").join("sdk-batch-session.jsonl");

    let today = Utc::now().date_naive();
    let week_start = today
        .checked_sub_days(Days::new(u64::from(today.weekday().num_days_from_monday())))
        .unwrap();
    let month_start = today.with_day(1).unwrap();
    write_file(
        &session_file,
        &format!(
            r#"{{"timestamp":"{month_start}T12:00:00Z","type":"turn_context","payload":{{"model":"gpt-5"}}}}
{{"timestamp":"{month_start}T12:00:00Z","type":"event_msg","payload":{{"type":"token_count","info":{{"total_token_usage":{{"input_tokens":100,"cached_input_tokens":20,"output_tokens":30,"reasoning_output_tokens":10,"total_tokens":140}},"last_token_usage":{{"input_tokens":100,"cached_input_tokens":20,"output_tokens":30,"reasoning_output_tokens":10,"total_tokens":140}},"model":"gpt-5"}}}}}}
{{"timestamp":"{week_start}T12:00:00Z","type":"event_msg","payload":{{"type":"token_count","info":{{"total_token_usage":{{"input_tokens":220,"cached_input_tokens":40,"output_tokens":80,"reasoning_output_tokens":20,"total_tokens":300}},"last_token_usage":{{"input_tokens":120,"cached_input_tokens":20,"output_tokens":50,"reasoning_output_tokens":10,"total_tokens":160}},"model":"gpt-5"}}}}}}
{{"timestamp":"{today}T12:00:00Z","type":"event_msg","payload":{{"type":"token_count","info":{{"total_token_usage":{{"input_tokens":360,"cached_input_tokens":70,"output_tokens":140,"reasoning_output_tokens":40,"total_tokens":500}},"last_token_usage":{{"input_tokens":140,"cached_input_tokens":30,"output_tokens":60,"reasoning_output_tokens":20,"total_tokens":200}},"model":"gpt-5"}}}}}}
"#
        ),
    );

    let previous_codex_home = std::env::var_os("CODEX_HOME");
    unsafe {
        std::env::set_var("CODEX_HOME", &codex_home);
    }

    let ranges = vec![
        UsageRange::Today,
        UsageRange::ThisWeek,
        UsageRange::ThisMonth,
    ];
    let batch = summarize_cost_ranges(MultiSummaryOptions {
        source: UsageSource::Codex,
        ranges: ranges.clone(),
        timezone: Some("UTC".to_string()),
        offline: true,
        strict_pricing: false,
        currency: None,
    })
    .expect("summarize codex ranges");

    let repeated: Vec<_> = ranges
        .iter()
        .cloned()
        .map(|range| {
            summarize_cost(SummaryOptions {
                source: UsageSource::Codex,
                range,
                timezone: Some("UTC".to_string()),
                offline: true,
                ..SummaryOptions::default()
            })
            .expect("summarize codex single range")
        })
        .collect();

    match previous_codex_home {
        Some(value) => unsafe {
            std::env::set_var("CODEX_HOME", value);
        },
        None => unsafe {
            std::env::remove_var("CODEX_HOME");
        },
    }

    assert_eq!(batch.source, UsageSource::Codex);
    assert_eq!(batch.source_name, "codex");
    assert_eq!(batch.currency, "USD");
    assert_eq!(batch.summaries.len(), ranges.len());
    assert!(batch.elapsed_ms.is_finite());
    assert!(!batch.generated_at.is_empty());
    for (actual, expected) in batch.summaries.iter().zip(repeated.iter()) {
        assert_stable_summary_eq(actual, expected);
    }
}

#[test]
fn sdk_batch_deduplicates_replayed_codex_token_counts_across_files() {
    let _guard = ENV_LOCK.lock().expect("env lock");
    let root = tempfile::tempdir().expect("temp dir");
    let codex_home = root.path().join("codex-home");
    let replay_a = codex_home.join("sessions").join("replay-a.jsonl");
    let replay_b = codex_home.join("sessions").join("replay-b.jsonl");
    let parent_meta = r#"{"timestamp":"2026-02-06T10:00:00Z","type":"session_meta","payload":{"id":"parent-session"}}"#;
    let fork_meta = r#"{"timestamp":"2026-02-06T10:00:00Z","type":"session_meta","payload":{"id":"forked-session"}}"#;
    let replayed = r#"{"timestamp":"2026-02-06T10:00:00Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":220,"cached_input_tokens":40,"output_tokens":80,"reasoning_output_tokens":20,"total_tokens":300},"last_token_usage":{"input_tokens":120,"cached_input_tokens":20,"output_tokens":50,"reasoning_output_tokens":10,"total_tokens":160},"model":"gpt-5"}}}"#;
    write_file(&replay_a, &format!("{parent_meta}\n{replayed}\n"));
    write_file(
        &replay_b,
        &format!(
            r#"{fork_meta}
{parent_meta}
{replayed}
{{"timestamp":"2026-02-06T10:01:00Z","type":"event_msg","payload":{{"type":"token_count","info":{{"total_token_usage":{{"input_tokens":360,"cached_input_tokens":70,"output_tokens":140,"reasoning_output_tokens":40,"total_tokens":500}},"last_token_usage":{{"input_tokens":140,"cached_input_tokens":30,"output_tokens":60,"reasoning_output_tokens":20,"total_tokens":200}},"model":"gpt-5"}}}}}}
"#
        ),
    );

    let previous_codex_home = std::env::var_os("CODEX_HOME");
    unsafe {
        std::env::set_var("CODEX_HOME", &codex_home);
    }

    let batch = summarize_cost_ranges(MultiSummaryOptions {
        source: UsageSource::Codex,
        ranges: vec![UsageRange::DateRange {
            since: Some(NaiveDate::from_ymd_opt(2026, 2, 6).unwrap()),
            until: Some(NaiveDate::from_ymd_opt(2026, 2, 6).unwrap()),
        }],
        timezone: Some("UTC".to_string()),
        offline: true,
        strict_pricing: false,
        currency: None,
    })
    .expect("summarize codex ranges");

    match previous_codex_home {
        Some(value) => unsafe {
            std::env::set_var("CODEX_HOME", value);
        },
        None => unsafe {
            std::env::remove_var("CODEX_HOME");
        },
    }

    let summary = &batch.summaries[0];
    assert_eq!(summary.valid_entries, 2);
    assert_eq!(summary.skipped_entries, 1);
    assert_eq!(summary.tokens.total_tokens, 370);
}

#[test]
fn sdk_batch_respects_timezone_boundaries_like_single_range() {
    let _guard = ENV_LOCK.lock().expect("env lock");
    let root = tempfile::tempdir().expect("temp dir");
    let codex_home = root.path().join("codex-home");
    let session_file = codex_home
        .join("sessions")
        .join("sdk-timezone-session.jsonl");
    write_file(
        &session_file,
        r#"{"timestamp":"2026-02-05T16:30:00Z","type":"turn_context","payload":{"model":"gpt-5"}}
{"timestamp":"2026-02-05T16:30:00Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":100,"cached_input_tokens":20,"output_tokens":30,"reasoning_output_tokens":10,"total_tokens":140},"last_token_usage":{"input_tokens":100,"cached_input_tokens":20,"output_tokens":30,"reasoning_output_tokens":10,"total_tokens":140},"model":"gpt-5"}}}
"#,
    );

    let previous_codex_home = std::env::var_os("CODEX_HOME");
    unsafe {
        std::env::set_var("CODEX_HOME", &codex_home);
    }

    let range = UsageRange::DateRange {
        since: Some(NaiveDate::from_ymd_opt(2026, 2, 6).unwrap()),
        until: Some(NaiveDate::from_ymd_opt(2026, 2, 6).unwrap()),
    };
    let shanghai_batch = summarize_cost_ranges(MultiSummaryOptions {
        source: UsageSource::Codex,
        ranges: vec![range.clone()],
        timezone: Some("Asia/Shanghai".to_string()),
        offline: true,
        strict_pricing: false,
        currency: None,
    })
    .expect("summarize shanghai range");
    let shanghai_single = summarize_cost(SummaryOptions {
        source: UsageSource::Codex,
        range: range.clone(),
        timezone: Some("Asia/Shanghai".to_string()),
        offline: true,
        ..SummaryOptions::default()
    })
    .expect("summarize shanghai single range");
    let utc_batch = summarize_cost_ranges(MultiSummaryOptions {
        source: UsageSource::Codex,
        ranges: vec![range],
        timezone: Some("UTC".to_string()),
        offline: true,
        strict_pricing: false,
        currency: None,
    })
    .expect("summarize utc range");

    match previous_codex_home {
        Some(value) => unsafe {
            std::env::set_var("CODEX_HOME", value);
        },
        None => unsafe {
            std::env::remove_var("CODEX_HOME");
        },
    }

    assert_stable_summary_eq(&shanghai_batch.summaries[0], &shanghai_single);
    assert_eq!(shanghai_batch.summaries[0].valid_entries, 1);
    assert_eq!(utc_batch.summaries[0].valid_entries, 0);
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
