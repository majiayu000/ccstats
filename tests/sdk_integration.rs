use std::fs;
use std::path::Path;
use std::sync::Mutex;

use ccstats::{
    CodexQuotaError, CodexQuotaStatus, CodexWeeklyValueError, CodexWeeklyValueWindow,
    CodexWeeklyValueWindowError, CostSummary, MultiSummaryOptions, SummaryOptions, UsageRange,
    UsageSource, estimate_codex_weekly_value, estimate_codex_weekly_value_for_window,
    load_codex_weekly_quota, summarize_cost, summarize_cost_ranges,
};
use chrono::{Datelike, Days, Duration, NaiveDate, Timelike, Utc};

static ENV_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn sdk_session_titles_respect_source_roots_and_leave_accounting_unchanged() {
    let _guard = ENV_LOCK.lock().expect("env lock");
    let root = tempfile::tempdir().expect("temp dir");
    let claude_home = root.path().join("claude-home");
    let codex_home = root.path().join("codex-home");
    let transcript = claude_home.join("projects/project/one.jsonl");
    let content = r#"{"timestamp":"2026-09-02T06:00:00Z","message":{"id":"message-one","model":"claude-sonnet-4-20250514","usage":{"input_tokens":100,"output_tokens":20}}}
"#;
    write_file(&transcript, content);
    write_file(
        &claude_home.join("projects/project/sessions-index.json"),
        r#"{"entries":[{"sessionId":"one","summary":"Claude summary"}]}"#,
    );
    write_file(
        &codex_home.join("session_index.jsonl"),
        "{\"id\":\"one\",\"thread_name\":\"Codex title\"}\n",
    );
    let previous_claude = std::env::var_os("CLAUDE_CONFIG_DIR");
    let previous_codex = std::env::var_os("CODEX_HOME");
    // Match this integration suite's serialized source-environment convention.
    unsafe {
        std::env::set_var("CLAUDE_CONFIG_DIR", &claude_home);
        std::env::set_var("CODEX_HOME", &codex_home);
    }
    let options = SummaryOptions {
        source: UsageSource::Claude,
        range: UsageRange::DateRange {
            since: NaiveDate::from_ymd_opt(2026, 9, 2),
            until: NaiveDate::from_ymd_opt(2026, 9, 2),
        },
        timezone: Some("UTC".into()),
        offline: true,
        ..SummaryOptions::default()
    };
    let before = ccstats::summarize_project_drilldown(&options);
    let ids = vec!["one".to_owned()];
    let claude = ccstats::load_session_titles(UsageSource::Claude, &ids);
    let codex = ccstats::load_session_titles(UsageSource::Codex, &ids);
    let after = ccstats::summarize_project_drilldown(&options);
    match previous_claude {
        Some(value) => unsafe { std::env::set_var("CLAUDE_CONFIG_DIR", value) },
        None => unsafe { std::env::remove_var("CLAUDE_CONFIG_DIR") },
    }
    match previous_codex {
        Some(value) => unsafe { std::env::set_var("CODEX_HOME", value) },
        None => unsafe { std::env::remove_var("CODEX_HOME") },
    }
    assert_eq!(claude.expect("Claude titles")["one"].text, "Claude summary");
    assert_eq!(codex.expect("Codex titles")["one"].text, "Codex title");
    let before = before.expect("project accounting");
    assert_eq!(before.projects[0].sessions[0].session_id, "one");
    assert_eq!(before.projects[0].metrics.tokens.total_tokens, 120);
    assert_eq!(before, after.expect("accounting after title lookup"));
    assert_eq!(
        fs::read_to_string(transcript).expect("original log"),
        content
    );
}

#[test]
fn sdk_loads_codex_weekly_quota_from_explicit_home() {
    let root = tempfile::tempdir().expect("temp dir");
    let codex_home = root.path().join("isolated-[quota]*-home");
    let session_file = codex_home.join("sessions").join("quota.jsonl");
    let now = Utc::now();
    let observed_at = now - Duration::hours(1);
    let resets_at = now + Duration::days(6);
    let event = serde_json::json!({
        "timestamp": observed_at.to_rfc3339(),
        "type": "event_msg",
        "payload": {
            "type": "token_count",
            "rate_limits": {
                "secondary": {
                    "used_percent": 25.0,
                    "window_minutes": 10_080,
                    "resets_at": resets_at.timestamp(),
                }
            }
        }
    });
    write_file(&session_file, &format!("{event}\n"));

    let report = load_codex_weekly_quota(Some(&codex_home)).expect("load quota report");

    assert_eq!(report.observed_at, observed_at);
    assert_eq!(report.resets_at.timestamp(), resets_at.timestamp());
    assert_eq!(report.window_minutes, 10_080);
    assert_eq!(report.used_pct, 25.0);
    assert_eq!(report.remaining_pct, 75.0);
    assert!(report.projected_pct_at_reset > 100.0);
    assert_eq!(report.status, CodexQuotaStatus::LikelyExhausted);

    let serialized = serde_json::to_value(&report).expect("serialize quota report");
    assert_eq!(serialized["status"], "likely_exhausted");
    assert_eq!(
        serialized["projected_pct_at_reset"],
        report.projected_pct_at_reset
    );
}

#[test]
fn sdk_estimates_codex_weekly_api_equivalent_value() {
    let _guard = ENV_LOCK.lock().expect("env lock");
    let root = tempfile::tempdir().expect("temp dir");
    let codex_home = root.path().join("codex-home");
    let session_file = codex_home.join("sessions").join("quota-and-usage.jsonl");
    let observed_at = (Utc::now() - Duration::hours(1))
        .with_nanosecond(0)
        .expect("valid timestamp");
    let resets_at = observed_at + Duration::days(6);
    let event = serde_json::json!({
        "timestamp": observed_at.to_rfc3339(),
        "type": "event_msg",
        "payload": {
            "type": "token_count",
            "info": {
                "total_token_usage": {
                    "input_tokens": 1_000_000,
                    "cached_input_tokens": 200_000,
                    "output_tokens": 100_000,
                    "reasoning_output_tokens": 50_000,
                    "total_tokens": 1_100_000,
                },
                "last_token_usage": {
                    "input_tokens": 1_000_000,
                    "cached_input_tokens": 200_000,
                    "output_tokens": 100_000,
                    "reasoning_output_tokens": 50_000,
                    "total_tokens": 1_100_000,
                },
                "model": "gpt-5",
            },
            "rate_limits": {
                "secondary": {
                    "used_percent": 25.0,
                    "window_minutes": 10_080,
                    "resets_at": resets_at.timestamp(),
                }
            }
        }
    });
    write_file(&session_file, &format!("{event}\n"));

    let estimate =
        estimate_codex_weekly_value(Some(&codex_home), true, false).expect("estimate weekly value");

    assert_eq!(estimate.observed_at, observed_at);
    assert_eq!(
        estimate.window_started_at,
        resets_at - Duration::minutes(10_080)
    );
    assert_eq!(estimate.observed_tokens, 1_100_000);
    assert!(estimate.observed_cost_usd > 0.0);
    assert!(
        (estimate.estimated_weekly_value_usd / estimate.observed_cost_usd - 4.0).abs()
            < f64::EPSILON
    );
    assert_eq!(estimate.estimated_weekly_tokens, 4_400_000.0);
}

#[test]
fn sdk_estimates_codex_value_for_an_explicit_exact_window() {
    let _guard = ENV_LOCK.lock().expect("env lock");
    let root = tempfile::tempdir().expect("temp dir");
    let codex_home = root.path().join("codex-home");
    let session_file = codex_home.join("sessions").join("usage.jsonl");
    let observed_at = (Utc::now() - Duration::hours(1))
        .with_nanosecond(0)
        .expect("valid timestamp");
    let resets_at = observed_at + Duration::days(6);
    let before_window = observed_at - Duration::days(8);
    let in_window = observed_at - Duration::hours(12);
    let usage_event = |timestamp, total: i64, delta: i64| {
        serde_json::json!({
            "timestamp": timestamp,
            "type": "event_msg",
            "payload": {
                "type": "token_count",
                "info": {
                    "total_token_usage": {
                        "input_tokens": total,
                        "cached_input_tokens": 0,
                        "output_tokens": 0,
                        "reasoning_output_tokens": 0,
                        "total_tokens": total,
                    },
                    "last_token_usage": {
                        "input_tokens": delta,
                        "cached_input_tokens": 0,
                        "output_tokens": 0,
                        "reasoning_output_tokens": 0,
                        "total_tokens": delta,
                    },
                    "model": "gpt-5",
                }
            }
        })
    };
    write_file(
        &session_file,
        &format!(
            "{}\n{}\n",
            usage_event(before_window, 500_000, 500_000),
            usage_event(in_window, 1_500_000, 1_000_000),
        ),
    );
    let window = CodexWeeklyValueWindow {
        observed_at,
        resets_at,
        window_minutes: 10_080,
        used_pct: 25.0,
    };

    let estimate = estimate_codex_weekly_value_for_window(&window, Some(&codex_home), true, false)
        .expect("estimate explicit weekly window");

    assert_eq!(estimate.observed_tokens, 1_000_000);
    assert_eq!(estimate.used_pct, 25.0);
    assert_eq!(estimate.estimated_weekly_tokens, 4_000_000.0);
}

#[test]
fn sdk_explicit_window_rejects_usage_without_a_timestamp() {
    let _guard = ENV_LOCK.lock().expect("env lock");
    let root = tempfile::tempdir().expect("temp dir");
    let codex_home = root.path().join("codex-home");
    let session_file = codex_home.join("sessions").join("usage.jsonl");
    let observed_at = (Utc::now() - Duration::hours(1))
        .with_nanosecond(0)
        .expect("valid timestamp");
    let resets_at = observed_at + Duration::days(6);
    let valid = serde_json::json!({
        "timestamp": observed_at - Duration::minutes(1),
        "type": "event_msg",
        "payload": {
            "type": "token_count",
            "info": {
                "last_token_usage": {
                    "input_tokens": 1_000_000,
                    "cached_input_tokens": 0,
                    "output_tokens": 0,
                    "reasoning_output_tokens": 0,
                    "total_tokens": 1_000_000,
                },
                "model": "gpt-5",
            }
        }
    });
    let missing_timestamp = serde_json::json!({
        "type": "event_msg",
        "payload": {
            "type": "token_count",
            "info": {
                "last_token_usage": {
                    "input_tokens": 500_000,
                    "cached_input_tokens": 0,
                    "output_tokens": 0,
                    "reasoning_output_tokens": 0,
                    "total_tokens": 500_000,
                },
                "model": "gpt-5",
            }
        }
    });
    write_file(&session_file, &format!("{valid}\n{missing_timestamp}\n"));
    let window = CodexWeeklyValueWindow {
        observed_at,
        resets_at,
        window_minutes: 10_080,
        used_pct: 25.0,
    };

    let error = estimate_codex_weekly_value_for_window(&window, Some(&codex_home), true, false)
        .expect_err("missing usage timestamp must fail closed");

    assert!(matches!(
        error,
        CodexWeeklyValueWindowError::Estimate(CodexWeeklyValueError::Quota(
            CodexQuotaError::UsageParse { count: 1 }
        ))
    ));
}

#[test]
fn sdk_explicit_codex_home_never_falls_back() {
    let root = tempfile::tempdir().expect("temp dir");
    let codex_home = root.path().join("missing-codex-home");

    let error = load_codex_weekly_quota(Some(&codex_home)).expect_err("missing sessions dir");

    assert!(matches!(
        error,
        CodexQuotaError::SessionsDirectoryNotFound { path }
            if path == codex_home.join("sessions")
    ));
}

#[test]
fn sdk_codex_weekly_quota_errors_are_typed() {
    let root = tempfile::tempdir().expect("temp dir");
    let codex_home = root.path().join("codex-home");
    let sessions_dir = codex_home.join("sessions");
    fs::create_dir_all(&sessions_dir).expect("create sessions dir");

    let missing = load_codex_weekly_quota(Some(&codex_home)).expect_err("missing snapshot");
    assert!(matches!(missing, CodexQuotaError::SnapshotNotFound));

    let malformed_file = sessions_dir.join("malformed.jsonl");
    write_file(&malformed_file, "{not json}\n");
    let malformed = load_codex_weekly_quota(Some(&codex_home)).expect_err("malformed snapshot");
    assert!(matches!(
        malformed,
        CodexQuotaError::SessionFile {
            action: "read",
            path,
            source,
        } if path == malformed_file && source.kind() == std::io::ErrorKind::InvalidData
    ));
}

#[cfg(unix)]
#[test]
fn sdk_rejects_symlinked_codex_sessions_root() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().expect("temp dir");
    let codex_home = root.path().join("codex-home");
    let real_sessions = root.path().join("real-sessions");
    fs::create_dir_all(&codex_home).expect("create Codex home");
    fs::create_dir_all(&real_sessions).expect("create real sessions");
    symlink(&real_sessions, codex_home.join("sessions")).expect("link sessions root");

    let error = load_codex_weekly_quota(Some(&codex_home)).expect_err("reject sessions symlink");
    assert!(matches!(
        error,
        CodexQuotaError::SessionDiscovery { path, source }
            if path == codex_home.join("sessions")
                && source.kind() == std::io::ErrorKind::InvalidInput
    ));
}

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
    assert_eq!(actual.estimated_cost, expected.estimated_cost);
    assert_eq!(actual.estimated_cost_usd, expected.estimated_cost_usd);
    assert_eq!(actual.cost_kind, expected.cost_kind);
    assert_eq!(
        actual.api_equivalent_cost_coverage,
        expected.api_equivalent_cost_coverage
    );
    assert_eq!(
        actual.grok_api_equivalent_cost,
        expected.grok_api_equivalent_cost
    );
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
    assert_eq!(summary.tokens.cache_hit_rate, Some(20.0));
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
fn sdk_exact_ranges_preserve_submillisecond_boundaries() {
    let _guard = ENV_LOCK.lock().expect("env lock");
    let root = tempfile::tempdir().expect("temp dir");
    let codex_home = root.path().join("codex-home");
    let session_file = codex_home.join("sessions").join("submillisecond.jsonl");
    write_file(
        &session_file,
        r#"{"timestamp":"2026-08-21T05:41:00.000100Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":100,"cached_input_tokens":0,"output_tokens":10,"reasoning_output_tokens":0,"total_tokens":110},"last_token_usage":{"input_tokens":100,"cached_input_tokens":0,"output_tokens":10,"reasoning_output_tokens":0,"total_tokens":110},"model":"gpt-5"}}}
"#,
    );
    let range = UsageRange::TimestampRange {
        since: "2026-08-21T05:41:00.000200Z".parse().expect("valid since"),
        until: "2026-08-21T05:41:00.000300Z".parse().expect("valid until"),
    };
    let previous_codex_home = std::env::var_os("CODEX_HOME");
    unsafe {
        std::env::set_var("CODEX_HOME", &codex_home);
    }

    let single = summarize_cost(SummaryOptions {
        source: UsageSource::Codex,
        range: range.clone(),
        timezone: Some("UTC".to_string()),
        offline: true,
        ..SummaryOptions::default()
    })
    .expect("summarize exact range");
    let batch = summarize_cost_ranges(MultiSummaryOptions {
        source: UsageSource::Codex,
        ranges: vec![range],
        timezone: Some("UTC".to_string()),
        offline: true,
        strict_pricing: false,
        currency: None,
    })
    .expect("summarize exact range batch");

    match previous_codex_home {
        Some(value) => unsafe {
            std::env::set_var("CODEX_HOME", value);
        },
        None => unsafe {
            std::env::remove_var("CODEX_HOME");
        },
    }

    assert_eq!(single.valid_entries, 0);
    assert_eq!(batch.summaries[0].valid_entries, 0);
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
    let outside_window = summarize_cost(SummaryOptions {
        source: UsageSource::Grok,
        range: UsageRange::TimestampRange {
            since: "2026-02-06T11:00:00Z".parse().expect("valid since"),
            until: "2026-02-06T12:00:00Z".parse().expect("valid until"),
        },
        timezone: Some("UTC".to_string()),
        offline: true,
        ..SummaryOptions::default()
    })
    .expect("summarize exact Grok window");

    assert_eq!(summary.source, UsageSource::Grok);
    assert_eq!(summary.source_name, "grok");
    assert_eq!(summary.valid_entries, 1);
    assert_eq!(summary.tokens.input_tokens, 1500);
    assert_eq!(summary.tokens.output_tokens, 0);
    assert_eq!(summary.tokens.total_tokens, 1500);
    assert_eq!(summary.tokens.cache_hit_rate, Some(0.0));
    assert_eq!(summary.models.len(), 1);
    assert_eq!(summary.models[0].model, "grok-build");
    assert!(summary.cost_usd.is_some_and(|cost| cost > 0.0));
    assert_eq!(summary.cost_kind, "estimated_proxy");
    assert_eq!(summary.estimated_cost_usd, summary.cost_usd);
    assert_eq!(summary.models[0].cost_kind, "estimated_proxy");
    assert_eq!(
        summary.models[0].estimated_cost_usd,
        summary.models[0].cost_usd
    );
    assert_eq!(outside_window.valid_entries, 0);
    assert_eq!(outside_window.tokens.total_tokens, 0);

    let single_json = serde_json::to_value(&summary).expect("serialize single Grok summary");
    assert_eq!(
        single_json["api_equivalent_cost_coverage"]["total_tokens"],
        1500
    );
    assert_eq!(
        single_json["api_equivalent_cost_coverage"]["priced_tokens"],
        0
    );

    let batch = summarize_cost_ranges(MultiSummaryOptions {
        source: UsageSource::Grok,
        ranges: vec![UsageRange::DateRange {
            since: Some(NaiveDate::from_ymd_opt(2026, 2, 6).unwrap()),
            until: Some(NaiveDate::from_ymd_opt(2026, 2, 6).unwrap()),
        }],
        timezone: Some("UTC".to_string()),
        offline: true,
        strict_pricing: false,
        currency: None,
    })
    .expect("summarize Grok batch");
    match previous_grok_home {
        Some(value) => unsafe {
            std::env::set_var("GROK_HOME", value);
        },
        None => unsafe {
            std::env::remove_var("GROK_HOME");
        },
    }
    let batch_json = serde_json::to_value(&batch).expect("serialize batch Grok summary");
    assert_eq!(
        batch_json["summaries"][0]["api_equivalent_cost_coverage"]["total_tokens"],
        1500
    );
}

#[test]
fn sdk_exposes_grok_partial_cost_estimate_for_exact_window() {
    let _guard = ENV_LOCK.lock().expect("env lock");
    let root = tempfile::tempdir().expect("temp dir");
    let grok_home = root.path().join("grok-home");
    let session_dir = grok_home
        .join("sessions")
        .join("%2Ftmp%2Fgrok-project")
        .join("sdk-grok-cost-session");
    write_file(
        &session_dir.join("summary.json"),
        r#"{"updated_at":"2026-08-16T01:00:00Z","current_model_id":"grok-4.5-build","git_root_dir":"/tmp/grok-project/"}"#,
    );
    write_file(
        &session_dir.join("updates.jsonl"),
        r#"{"timestamp":1786838400,"params":{"sessionId":"sdk-grok-cost-session","update":{"sessionUpdate":"turn_completed","prompt_id":"p1","usage":{"inputTokens":100,"outputTokens":20,"cachedReadTokens":40,"reasoningTokens":5,"modelCalls":1,"costUsdTicks":20000000000,"modelUsage":{"grok-4.5-build":{"inputTokens":100,"outputTokens":20,"cachedReadTokens":40,"reasoningTokens":5,"modelCalls":1,"costUsdTicks":20000000000}}}}}}
{"timestamp":1786839000,"params":{"sessionId":"sdk-grok-cost-session","update":{"sessionUpdate":"turn_completed","prompt_id":"p2","usage":{"inputTokens":50,"outputTokens":10,"cachedReadTokens":10,"reasoningTokens":2,"modelCalls":1,"costUsdTicks":5000000000,"modelUsage":{"grok-4.5-build":{"inputTokens":50,"outputTokens":10,"cachedReadTokens":10,"reasoningTokens":2,"modelCalls":1,"costUsdTicks":5000000000}}}}}}
{"timestamp":1786842000,"params":{"sessionId":"sdk-grok-cost-session","update":{"sessionUpdate":"turn_completed","prompt_id":"outside","usage":{"inputTokens":1000,"outputTokens":100,"cachedReadTokens":0,"reasoningTokens":0,"modelCalls":1,"costUsdTicks":5000000000,"modelUsage":{"grok-4.5-build":{"inputTokens":1000,"outputTokens":100,"cachedReadTokens":0,"reasoningTokens":0,"modelCalls":1,"costUsdTicks":5000000000}}}}}}
"#,
    );
    write_file(
        &grok_home.join("logs/unified.jsonl"),
        r#"{"ts":"2026-08-16T00:00:00Z","sid":"sdk-grok-cost-session","msg":"shell.turn.inference_done","ctx":{"loop_index":1,"prompt_tokens":100,"cached_prompt_tokens":40,"completion_tokens":20,"reasoning_tokens":5}}
{"ts":"2026-08-16T01:00:00Z","sid":"sdk-grok-cost-session","msg":"shell.turn.inference_done","ctx":{"loop_index":2,"prompt_tokens":1000,"cached_prompt_tokens":0,"completion_tokens":100,"reasoning_tokens":0}}
"#,
    );

    let range = UsageRange::TimestampRange {
        since: "2026-08-16T00:00:00Z".parse().expect("valid since"),
        until: "2026-08-16T00:30:00Z".parse().expect("valid until"),
    };
    let previous_home = std::env::var_os("HOME");
    let previous_xdg_data = std::env::var_os("XDG_DATA_HOME");
    let previous_grok_home = std::env::var_os("GROK_HOME");
    unsafe {
        std::env::set_var("HOME", root.path());
        std::env::set_var("XDG_DATA_HOME", root.path().join("xdg-data"));
        std::env::set_var("GROK_HOME", &grok_home);
    }

    let summary = summarize_cost(SummaryOptions {
        source: UsageSource::Grok,
        range: range.clone(),
        timezone: Some("UTC".to_string()),
        offline: true,
        ..SummaryOptions::default()
    })
    .expect("summarize exact Grok cost window");
    let batch = summarize_cost_ranges(MultiSummaryOptions {
        source: UsageSource::Grok,
        ranges: vec![range],
        timezone: Some("UTC".to_string()),
        offline: true,
        strict_pricing: false,
        currency: None,
    })
    .expect("summarize exact Grok cost window in batch");

    match previous_home {
        Some(value) => unsafe { std::env::set_var("HOME", value) },
        None => unsafe { std::env::remove_var("HOME") },
    }
    match previous_xdg_data {
        Some(value) => unsafe { std::env::set_var("XDG_DATA_HOME", value) },
        None => unsafe { std::env::remove_var("XDG_DATA_HOME") },
    }
    match previous_grok_home {
        Some(value) => unsafe { std::env::set_var("GROK_HOME", value) },
        None => unsafe { std::env::remove_var("GROK_HOME") },
    }

    assert_eq!(summary.tokens.total_tokens, 180);
    let coverage = summary
        .api_equivalent_cost_coverage
        .as_ref()
        .expect("Grok cost coverage");
    assert_eq!(coverage.total_tokens, 180);
    assert_eq!(coverage.priced_tokens, 120);
    assert!((coverage.percent - 66.666_666_666_666_66).abs() < 1e-12);

    let estimate = summary
        .grok_api_equivalent_cost
        .as_ref()
        .expect("Grok API-equivalent estimate");
    assert!((estimate.observed_usd - 0.000_252).abs() < 1e-12);
    assert!((estimate.estimated_usd.expect("point estimate") - 0.000_395).abs() < 1e-12);
    assert!((estimate.minimum_usd.expect("minimum") - 0.000_395).abs() < 1e-12);
    assert!((estimate.maximum_usd.expect("maximum") - 0.000_538).abs() < 1e-12);
    assert_eq!(estimate.priced_tokens, 120);
    assert_eq!(estimate.total_tokens, 180);
    assert_eq!(estimate.coverage_status, "partial");
    assert_eq!(estimate.excluded_request_tokens, 0);
    assert_eq!(
        batch.summaries[0].grok_api_equivalent_cost,
        summary.grok_api_equivalent_cost
    );
}
