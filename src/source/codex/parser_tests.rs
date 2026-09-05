use super::*;

fn parse_cache_write_usage(writes: i64) -> ParseOutput {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("usage.jsonl");
    let usage = serde_json::json!({
        "input_tokens": 1000, "cached_input_tokens": 600,
        "cache_write_input_tokens": writes, "output_tokens": 50,
        "reasoning_output_tokens": 20, "total_tokens": 1050,
    });
    let event = serde_json::json!({
        "timestamp": "2026-09-05T00:51:00Z", "type": "event_msg",
        "payload": {"type": "token_count", "model": "gpt-6-astra",
            "info": {"total_token_usage": usage, "last_token_usage": usage}}
    });
    std::fs::write(&path, event.to_string()).unwrap();
    parse_codex_file_for_quota(&path, Timezone::Named(chrono_tz::UTC))
}

#[test]
fn cache_writes_are_separate_from_uncached_input() {
    let result = parse_cache_write_usage(300);
    assert_eq!(result.errors, 0);
    let entry = &result.entries[0];
    assert_eq!(entry.cache_creation, 300);
    assert_eq!(entry.input_tokens, 100);
    assert_eq!(entry.to_stats().total_tokens(), 1050);
}

#[test]
fn invalid_cache_write_buckets_fail_instead_of_underpricing() {
    for writes in [-1, 401] {
        let result = parse_cache_write_usage(writes);
        assert_eq!(result.errors, 1);
        assert!(result.entries.is_empty());
    }
}

#[test]
fn cumulative_cache_writes_participate_in_deltas_and_deduplication() {
    let first = UsageTotals {
        input_tokens: 1000,
        cache_write_input_tokens: 100,
        ..UsageTotals::default()
    };
    let second = UsageTotals {
        cache_write_input_tokens: 200,
        ..first
    };
    assert!(!second.is_duplicate_of(&first));
    assert_eq!(second.subtract(first).cache_write_input_tokens, 100);
}

#[test]
fn discovery_includes_active_and_archived_sessions() {
    let temp = tempfile::tempdir().unwrap();
    let active_dir = temp.path().join("sessions/2026/08/31");
    let archived_dir = temp.path().join("archived_sessions");
    std::fs::create_dir_all(&active_dir).unwrap();
    std::fs::create_dir_all(&archived_dir).unwrap();
    let active = active_dir.join("active.jsonl");
    let archived = archived_dir.join("archived.jsonl");
    std::fs::write(&active, "").unwrap();
    std::fs::write(&archived, "").unwrap();

    let files = find_codex_files_in_root(temp.path());

    assert_eq!(files, vec![archived, active]);
}

#[test]
fn test_subtract_normal() {
    let total = TokenUsage {
        cache_write_input_tokens: None,
        input_tokens: Some(1000),
        cached_input_tokens: Some(200),
        alt_cache_read_input_tokens: None,
        output_tokens: Some(500),
        reasoning_output_tokens: Some(100),
        total_tokens: Some(1500),
    };
    let prev = TokenUsage {
        cache_write_input_tokens: None,
        input_tokens: Some(400),
        cached_input_tokens: Some(100),
        alt_cache_read_input_tokens: None,
        output_tokens: Some(200),
        reasoning_output_tokens: Some(50),
        total_tokens: Some(600),
    };
    let delta = total.subtract(&prev);
    assert_eq!(delta.input_tokens, Some(600));
    assert_eq!(delta.cached_input_tokens, Some(100));
    assert_eq!(delta.output_tokens, Some(300));
    assert_eq!(delta.reasoning_output_tokens, Some(50));
    assert_eq!(delta.total_tokens, Some(900));
}

#[test]
fn test_subtract_clamps_negative_to_zero() {
    let total = TokenUsage {
        cache_write_input_tokens: None,
        input_tokens: Some(100),
        cached_input_tokens: Some(50),
        alt_cache_read_input_tokens: None,
        output_tokens: Some(10),
        reasoning_output_tokens: Some(0),
        total_tokens: Some(110),
    };
    let prev = TokenUsage {
        cache_write_input_tokens: None,
        input_tokens: Some(500),
        cached_input_tokens: Some(200),
        alt_cache_read_input_tokens: None,
        output_tokens: Some(300),
        reasoning_output_tokens: Some(100),
        total_tokens: Some(800),
    };
    let delta = total.subtract(&prev);
    assert_eq!(delta.input_tokens, Some(0));
    assert_eq!(delta.cached_input_tokens, Some(0));
    assert_eq!(delta.output_tokens, Some(0));
    assert_eq!(delta.reasoning_output_tokens, Some(0));
    assert_eq!(delta.total_tokens, Some(0));
}

#[test]
fn test_subtract_none_fields_treated_as_zero() {
    let total = TokenUsage {
        input_tokens: Some(100),
        ..Default::default()
    };
    let prev = TokenUsage::default();
    let delta = total.subtract(&prev);
    assert_eq!(delta.input_tokens, Some(100));
    assert_eq!(delta.output_tokens, Some(0));
    assert_eq!(delta.reasoning_output_tokens, Some(0));
}

#[test]
fn test_usage_totals_duplicate_when_complete_vector_matches() {
    let prev = UsageTotals {
        cache_write_input_tokens: 0,
        input_tokens: 100,
        cached_input_tokens: 20,
        output_tokens: 30,
        reasoning_output_tokens: 10,
        total_tokens: 0,
    };
    let total = UsageTotals {
        cache_write_input_tokens: 0,
        input_tokens: 100,
        cached_input_tokens: 20,
        output_tokens: 30,
        reasoning_output_tokens: 10,
        total_tokens: 0,
    };

    assert!(total.is_duplicate_of(&prev));
}

#[test]
fn test_usage_totals_not_duplicate_when_component_grows_with_zero_total() {
    let prev = UsageTotals {
        cache_write_input_tokens: 0,
        input_tokens: 100,
        cached_input_tokens: 20,
        output_tokens: 30,
        reasoning_output_tokens: 10,
        total_tokens: 0,
    };
    let total = UsageTotals {
        cache_write_input_tokens: 0,
        input_tokens: 150,
        cached_input_tokens: 20,
        output_tokens: 30,
        reasoning_output_tokens: 10,
        total_tokens: 0,
    };

    assert!(!total.is_duplicate_of(&prev));
    assert_eq!(total.subtract(prev).input_tokens, 50);
}

#[test]
fn test_is_empty_default() {
    assert!(TokenUsage::default().is_empty());
}

#[test]
fn test_is_empty_with_input() {
    let usage = TokenUsage {
        input_tokens: Some(1),
        ..Default::default()
    };
    assert!(!usage.is_empty());
}

#[test]
fn test_is_empty_with_cached_only() {
    let usage = TokenUsage {
        cached_input_tokens: Some(50),
        ..Default::default()
    };
    assert!(!usage.is_empty());
}

#[test]
fn test_is_empty_with_reasoning_only() {
    let usage = TokenUsage {
        reasoning_output_tokens: Some(10),
        ..Default::default()
    };
    assert!(!usage.is_empty());
}

#[test]
fn test_cached_input_prefers_cached_input_tokens() {
    let usage = TokenUsage {
        cached_input_tokens: Some(100),
        alt_cache_read_input_tokens: Some(50),
        ..Default::default()
    };
    assert_eq!(usage.cached_input(), 100);
}

#[test]
fn test_cached_input_falls_back_to_cache_read() {
    let usage = TokenUsage {
        cached_input_tokens: None,
        alt_cache_read_input_tokens: Some(75),
        ..Default::default()
    };
    assert_eq!(usage.cached_input(), 75);
}

#[test]
fn test_cached_input_both_none_returns_zero() {
    let usage = TokenUsage::default();
    assert_eq!(usage.cached_input(), 0);
}

#[test]
fn test_extract_model_from_info_model() {
    let payload = Payload {
        cwd: None,
        payload_type: None,
        id: None,
        model: Some("fallback-model"),
        source: None,
        thread_source: None,
        info: Some(TokenInfo {
            total_token_usage: None,
            last_token_usage: None,
            model: Some("info-model"),
            model_name: Some("info-model-name"),
            metadata: Some(Metadata {
                model: Some("meta-model"),
            }),
        }),
    };
    assert_eq!(extract_model(&payload), Some("info-model".to_string()));
}

#[test]
fn test_extract_model_falls_back_to_model_name() {
    let payload = Payload {
        cwd: None,
        payload_type: None,
        id: None,
        model: Some("fallback"),
        source: None,
        thread_source: None,
        info: Some(TokenInfo {
            total_token_usage: None,
            last_token_usage: None,
            model: None,
            model_name: Some("model-name"),
            metadata: None,
        }),
    };
    assert_eq!(extract_model(&payload), Some("model-name".to_string()));
}

#[test]
fn test_extract_model_falls_back_to_metadata() {
    let payload = Payload {
        cwd: None,
        payload_type: None,
        id: None,
        model: Some("fallback"),
        source: None,
        thread_source: None,
        info: Some(TokenInfo {
            total_token_usage: None,
            last_token_usage: None,
            model: None,
            model_name: None,
            metadata: Some(Metadata {
                model: Some("meta-model"),
            }),
        }),
    };
    assert_eq!(extract_model(&payload), Some("meta-model".to_string()));
}

#[test]
fn test_extract_model_falls_back_to_payload_model() {
    let payload = Payload {
        cwd: None,
        payload_type: None,
        id: None,
        model: Some("payload-model"),
        source: None,
        thread_source: None,
        info: Some(TokenInfo {
            total_token_usage: None,
            last_token_usage: None,
            model: None,
            model_name: None,
            metadata: None,
        }),
    };
    assert_eq!(extract_model(&payload), Some("payload-model".to_string()));
}

#[test]
fn test_extract_model_no_info_uses_payload() {
    let payload = Payload {
        cwd: None,
        payload_type: None,
        id: None,
        model: Some("payload-only"),
        source: None,
        thread_source: None,
        info: None,
    };
    assert_eq!(extract_model(&payload), Some("payload-only".to_string()));
}

#[test]
fn test_extract_model_all_none_returns_none() {
    let payload = Payload {
        cwd: None,
        payload_type: None,
        id: None,
        model: None,
        source: None,
        thread_source: None,
        info: None,
    };
    assert_eq!(extract_model(&payload), None);
}

#[test]
fn test_extract_model_empty_strings_skipped() {
    let payload = Payload {
        cwd: None,
        payload_type: None,
        id: None,
        model: Some("real-model"),
        source: None,
        thread_source: None,
        info: Some(TokenInfo {
            total_token_usage: None,
            last_token_usage: None,
            model: Some("  "),
            model_name: Some(""),
            metadata: None,
        }),
    };
    assert_eq!(extract_model(&payload), Some("real-model".to_string()));
}

#[test]
fn codex_origin_parses_source_strings() {
    let cli: Payload<'_> =
        serde_json::from_str(r#"{"source":"cli","thread_source":"user"}"#).unwrap();
    let exec: Payload<'_> =
        serde_json::from_str(r#"{"source":"exec","thread_source":"user"}"#).unwrap();

    assert_eq!(
        session_origin_from_payload(Some(&cli)),
        CodexSessionOrigin::Interactive
    );
    assert_eq!(
        session_origin_from_payload(Some(&exec)),
        CodexSessionOrigin::Exec
    );
}

#[test]
fn codex_origin_parses_tagged_subagent_source_shapes() {
    let review: Payload<'_> =
        serde_json::from_str(r#"{"source":{"subagent":"review"},"thread_source":"user"}"#).unwrap();
    let spawned: Payload<'_> = serde_json::from_str(
        r#"{"source":{"subagent":{"thread_spawn":{"id":"child"}}},"thread_source":"user"}"#,
    )
    .unwrap();
    let thread_source: Payload<'_> =
        serde_json::from_str(r#"{"source":{"future":"shape"},"thread_source":"subagent"}"#)
            .unwrap();

    assert_eq!(
        session_origin_from_payload(Some(&review)),
        CodexSessionOrigin::Subagent
    );
    assert_eq!(
        session_origin_from_payload(Some(&spawned)),
        CodexSessionOrigin::Subagent
    );
    assert_eq!(
        session_origin_from_payload(Some(&thread_source)),
        CodexSessionOrigin::Subagent
    );
}

#[test]
fn codex_origin_keeps_unknown_shapes_out_of_named_scopes() {
    let unknown: Payload<'_> =
        serde_json::from_str(r#"{"source":{"future":"shape"},"thread_source":"user"}"#).unwrap();
    let origin = session_origin_from_payload(Some(&unknown));

    assert_eq!(origin, CodexSessionOrigin::Unknown);
    assert!(scope_includes_origin(CodexScope::All, origin));
    assert!(!scope_includes_origin(CodexScope::Interactive, origin));
    assert!(!scope_includes_origin(CodexScope::Exec, origin));
    assert!(!scope_includes_origin(CodexScope::Subagent, origin));
}
