use super::*;
use crate::core::ClaudeDiagnostics;
use crate::source::Source;

fn entry(at: i64, writes: i64, endpoint: Endpoint, version: &str, turn: u64) -> RawEntry {
    let mut e: RawEntry = serde_json::from_value(serde_json::json!({
        "timestamp": timestamp(at), "timestamp_ms":at,"date_str":"2026-10-07",
        "message_id":format!("m-{at}-{turn}"),"session_id":"main","project_path":"redacted",
        "model":"sonnet","input_tokens":100,"output_tokens":0,"cache_creation":writes,
        "cache_read":0,"reasoning_tokens":0,"stop_reason":"end_turn"
    }))
    .unwrap();
    e.session_key = "main-file".into();
    e.endpoint = endpoint;
    e.agent_version = Some(version.into());
    e.claude_diagnostics = Some(ClaudeDiagnostics {
        turn: Some(turn),
        turn_start_ms: Some(at),
        cache_write_reported: true,
        model_id: "claude-sonnet-4-20250514".into(),
        ..Default::default()
    });
    e
}

fn kinds(report: &Report) -> Vec<&str> {
    report.findings.iter().map(|f| f.kind).collect()
}

#[test]
fn window_and_cache_share_thresholds_use_active_baseline_and_exact_cohorts() {
    let mut entries = vec![
        entry(1, 0, Endpoint::Native, "1", 1),
        entry(101, 0, Endpoint::Native, "1", 2),
        entry(201, 100, Endpoint::Native, "2", 3),
    ];
    let r = analyze(&entries, 200, 300, &[(0, 100), (100, 200)], None);
    assert_eq!(r.token_ratio, Some(2.0));
    assert!(kinds(&r).contains(&"window_above_baseline"));
    assert!(kinds(&r).contains(&"cache_write_share_increase"));
    entries[2].cache_creation = 20;
    let r = analyze(&entries, 200, 300, &[(0, 100), (100, 200)], None);
    assert!(!kinds(&r).contains(&"window_above_baseline"));
    assert!(!kinds(&r).contains(&"cache_write_share_increase"));
    entries[2].cache_creation = 100;
    entries[2].endpoint = Endpoint::Proxy;
    let r = analyze(&entries, 200, 300, &[(0, 100), (100, 200)], None);
    assert!(!kinds(&r).contains(&"cache_write_share_increase"));
    assert_eq!(r.dimensions[0].baseline_windows, 0);
}

#[test]
fn missing_cache_is_unknown_while_reported_zero_is_valid_evidence() {
    let mut entries = vec![
        entry(1, 0, Endpoint::Native, "1", 1),
        entry(101, 0, Endpoint::Native, "1", 2),
        entry(201, 500, Endpoint::Native, "2", 3),
    ];
    for e in &mut entries[..2] {
        e.claude_diagnostics.as_mut().unwrap().cache_write_reported = false;
    }
    let r = analyze(&entries, 200, 300, &[(0, 100), (100, 200)], None);
    assert!(!kinds(&r).contains(&"cache_write_share_increase"));
    assert_eq!(r.versions.len(), 1);
    entries[2]
        .claude_diagnostics
        .as_mut()
        .unwrap()
        .cache_write_reported = false;
    let r = analyze(&entries, 200, 300, &[(0, 100), (100, 200)], None);
    assert!(kinds(&r).contains(&"missing_cache_fields"));
    assert!(r.versions.is_empty());
}

#[test]
fn subagents_ttl_and_mixed_endpoints_have_trigger_and_nontrigger_cases() {
    let mut entries = vec![
        entry(201, 100, Endpoint::Native, "1", 1),
        entry(202, 100, Endpoint::Proxy, "1", 2),
    ];
    entries[1].session_key = "child-file".into();
    entries[1].session_id = "child".into();
    let d = entries[1].claude_diagnostics.as_mut().unwrap();
    d.is_subagent = true;
    d.parent_session_id = Some("main".into());
    entries[0].cache_creation_1h = 50;
    let r = analyze(&entries, 200, 300, &[], Some("main"));
    for kind in ["subagent_share", "one_hour_cache_writes", "mixed_endpoints"] {
        assert!(kinds(&r).contains(&kind));
    }
    assert_eq!(r.sessions.len(), 2);
    entries[1].claude_diagnostics.as_mut().unwrap().is_subagent = false;
    entries[1].endpoint = Endpoint::Native;
    entries[0].cache_creation_1h = 49;
    let r = analyze(&entries, 200, 300, &[], None);
    for kind in ["subagent_share", "one_hour_cache_writes", "mixed_endpoints"] {
        assert!(!kinds(&r).contains(&kind));
    }
}

#[test]
fn versions_require_100_complete_turns_per_exact_model_and_endpoint() {
    let mut entries = Vec::new();
    for (version, writes) in [("2.1.284", 100), ("2.1.286", 125)] {
        for _ in 0..100 {
            entries.push(entry(
                entries.len() as i64 + 1,
                writes,
                Endpoint::Native,
                version,
                entries.len() as u64 + 1,
            ));
        }
    }
    let refs: Vec<_> = entries.iter().collect();
    let rows = version_rows(&refs, 0, 1000);
    assert_eq!(rows[1].assessment, "observed_change_not_causation");
    assert_eq!(rows[1].change_pct, Some(25.0));
    let rows = version_rows(&refs[..199], 0, 1000);
    assert_eq!(rows[1].assessment, "insufficient_samples");
    // Same display model, different full model ID: never compare.
    for e in &mut entries[100..] {
        e.claude_diagnostics.as_mut().unwrap().model_id = "claude-sonnet-4-20250929".into();
    }
    assert!(
        version_rows(&entries.iter().collect::<Vec<_>>(), 0, 1000)
            .iter()
            .all(|r| r.previous_version.is_none())
    );
    for e in &mut entries[100..] {
        e.claude_diagnostics.as_mut().unwrap().model_id = "claude-sonnet-4-20250514".into();
        e.endpoint = Endpoint::Proxy;
        e.cache_creation = 0;
    }
    assert!(
        version_rows(&entries.iter().collect::<Vec<_>>(), 0, 1000)
            .iter()
            .all(|r| r.previous_version.is_none())
    );
}

#[test]
fn partial_mixed_and_unknown_turns_do_not_become_version_evidence() {
    let mut entries = vec![
        entry(10, 100, Endpoint::Native, "1", 1),
        entry(20, 200, Endpoint::Native, "2", 1),
    ];
    assert!(version_rows(&entries.iter().collect::<Vec<_>>(), 0, 100).is_empty());
    entries[1].agent_version = Some("1".into());
    assert!(version_rows(&entries.iter().collect::<Vec<_>>(), 15, 100).is_empty());
    for e in &mut entries {
        e.stop_reason = Some("tool_use".into());
    }
    assert!(version_rows(&entries.iter().collect::<Vec<_>>(), 0, 100).is_empty());
    entries[1].stop_reason = Some("end_turn".into());
    for e in &mut entries {
        e.endpoint = Endpoint::Unknown;
    }
    assert!(version_rows(&entries.iter().collect::<Vec<_>>(), 0, 100).is_empty());
}

#[test]
fn no_data_and_zero_baseline_do_not_generate_infinite_ratios_or_claims() {
    let r = analyze(&[], 200, 300, &[(0, 100), (100, 200)], None);
    assert!(r.findings.is_empty());
    assert_eq!(r.token_ratio, None);
    assert!(
        serde_json::to_string(&r)
            .unwrap()
            .contains("\"token_ratio\":null")
    );
    let mut entries = vec![
        entry(1, 0, Endpoint::Native, "1", 1),
        entry(101, 0, Endpoint::Native, "1", 2),
        entry(201, 1, Endpoint::Native, "2", 3),
    ];
    entries[0].input_tokens = 0;
    entries[1].input_tokens = 0;
    let r = analyze(&entries, 200, 300, &[(0, 100), (100, 200)], None);
    assert_eq!(r.token_ratio, None);
    assert!(!kinds(&r).contains(&"window_above_baseline"));
}

#[test]
fn native_user_boundaries_tool_results_metadata_and_compactions_survive_projection() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir
        .path()
        .join("projects/redacted/parent/subagents/agent-child.jsonl");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let rows = [
        serde_json::json!({"type":"user","version":"2.1.286","message":{"content":[{"type":"text","text":"redacted"}]}}),
        serde_json::json!({"type":"assistant","timestamp":"2026-10-07T10:00:00Z","message":{"id":"a","model":"claude-sonnet-4","stop_reason":"tool_use","usage":{"input_tokens":10,"output_tokens":1,"inference_geo":"not_available"}}}),
        serde_json::json!({"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t","content":"redacted"}]}}),
        serde_json::json!({"type":"system","subtype":"compact_boundary","timestamp":"2026-10-07T10:01:00Z","compactMetadata":{"trigger":"auto","preTokens":150_000},"content":"redacted"}),
        serde_json::json!({"type":"user","isCompactSummary":true,"message":{"content":"redacted"}}),
        serde_json::json!({"type":"user","isMeta":true,"message":{"content":"redacted"}}),
        serde_json::json!({"type":"assistant","timestamp":"2026-10-07T10:02:00Z","message":{"id":"b","model":"claude-sonnet-4","stop_reason":"end_turn","usage":{"input_tokens":10,"output_tokens":1,"cache_creation_input_tokens":0,"inference_geo":"not_available"}}}),
        serde_json::json!({"type":"user","message":{"content":"redacted"}}),
        serde_json::json!({"type":"assistant","timestamp":"2026-10-07T10:03:00Z","message":{"id":"c","model":"claude-sonnet-4","stop_reason":"end_turn","usage":{"input_tokens":10,"output_tokens":1,"cache_creation_input_tokens":100,"inference_geo":"not_available"}}}),
    ];
    std::fs::write(
        &path,
        rows.iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n"),
    )
    .unwrap();
    let parsed =
        ClaudeSource::new().parse_file(&path, crate::utils::Timezone::Named(chrono_tz::UTC), false);
    assert_eq!(parsed.errors, 0);
    assert_eq!(parsed.entries.len(), 3);
    let d: Vec<_> = parsed
        .entries
        .iter()
        .map(|e| e.claude_diagnostics.as_ref().unwrap())
        .collect();
    assert_eq!(
        d.iter().map(|d| d.turn).collect::<Vec<_>>(),
        [Some(1), Some(1), Some(2)]
    );
    assert!(!d[0].cache_write_reported);
    assert!(d[1].cache_write_reported);
    assert!(d[1].is_subagent);
    assert_eq!(d[1].parent_session_id.as_deref(), Some("parent"));
    assert_eq!(parsed.entries[2].agent_version.as_deref(), Some("2.1.286"));
    let start = chrono::DateTime::parse_from_rfc3339("2026-10-07T10:00:00Z")
        .unwrap()
        .timestamp_millis();
    let r = analyze(&parsed.entries, start, start + 600_000, &[], None);
    assert_eq!(r.usage.compactions, 1);
    assert_eq!(r.usage.turns, 2);
    assert_eq!(r.sessions[0].compaction_times, ["2026-10-07T10:01:00Z"]);
}

#[test]
fn cache_share_requires_two_fully_reported_baseline_windows() {
    let mut entries = vec![
        entry(1, 0, Endpoint::Native, "1", 1),
        entry(101, 0, Endpoint::Native, "1", 2),
        entry(201, 100, Endpoint::Native, "2", 3),
    ];
    entries[1]
        .claude_diagnostics
        .as_mut()
        .unwrap()
        .cache_write_reported = false;
    let r = analyze(&entries, 200, 300, &[(0, 100), (100, 200)], None);
    assert!(!kinds(&r).contains(&"cache_write_share_increase"));
    let dimension = serde_json::to_value(&r.dimensions[0]).unwrap();
    assert_eq!(dimension["cache_share_baseline_windows"], 1);
    assert_eq!(dimension["cache_share_baseline_messages"], 1);
}

#[test]
fn version_direction_is_independent_of_dedup_output_order() {
    let mut early = entry(10, 100, Endpoint::Native, "284", 1);
    early.stop_reason = Some("tool_use".into());
    let late = entry(40, 100, Endpoint::Native, "284", 1);
    let second = entry(30, 400, Endpoint::Native, "286", 2);
    let chronological = version_rows(&[&early, &second, &late], 0, 100);
    let unordered = version_rows(&[&late, &second, &early], 0, 100);
    assert_eq!(
        serde_json::to_value(&chronological).unwrap(),
        serde_json::to_value(&unordered).unwrap()
    );
    assert_eq!(unordered[1].version, "286");
    assert_eq!(unordered[1].change_pct, Some(100.0));
}

#[test]
fn loader_date_cut_does_not_turn_old_prefix_into_a_complete_new_turn() {
    use chrono::Duration;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("projects/redacted/main.jsonl");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let now = Utc::now();
    let old = (now - Duration::days(23)).to_rfc3339();
    let recent = (now - Duration::hours(1)).to_rfc3339();
    let rows = [
        serde_json::json!({"type":"user","timestamp":old,"version":"2.1.286","message":{"content":"redacted"}}),
        serde_json::json!({"type":"assistant","timestamp":old,"message":{"id":"old","model":"claude-sonnet-4","stop_reason":"tool_use","usage":{"input_tokens":1,"output_tokens":1,"cache_creation_input_tokens":100,"inference_geo":"not_available"}}}),
        serde_json::json!({"type":"assistant","timestamp":recent,"message":{"id":"recent","model":"claude-sonnet-4","stop_reason":"end_turn","usage":{"input_tokens":1,"output_tokens":1,"cache_creation_input_tokens":200,"inference_geo":"not_available"}}}),
    ];
    std::fs::write(
        &path,
        rows.iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n"),
    )
    .unwrap();
    let parsed =
        ClaudeSource::new().parse_file(&path, crate::utils::Timezone::Named(chrono_tz::UTC), false);
    assert_eq!(parsed.errors, 0);
    let cutoff = (now - Duration::days(22)).timestamp_millis();
    let filtered: Vec<_> = parsed
        .entries
        .iter()
        .filter(|e| e.timestamp_ms >= cutoff)
        .collect();
    assert_eq!(filtered.len(), 1);
    assert!(version_rows(&filtered, cutoff, now.timestamp_millis()).is_empty());
}

#[test]
fn zero_token_windows_do_not_establish_a_cache_share_baseline() {
    let mut entries = vec![
        entry(1, 0, Endpoint::Native, "1", 1),
        entry(101, 0, Endpoint::Native, "1", 2),
        entry(201, 100, Endpoint::Native, "2", 3),
    ];
    entries[0].input_tokens = 0;
    entries[1].input_tokens = 0;
    let r = analyze(&entries, 200, 300, &[(0, 100), (100, 200)], None);
    assert!(!kinds(&r).contains(&"cache_write_share_increase"));
    assert_eq!(r.dimensions[0].cache_share_baseline_windows, 0);
}
