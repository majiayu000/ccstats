use super::*;

#[test]
fn parses_completion_without_double_counting_reasoning() {
    let root = tempdir().expect("temp dir");
    let sessions_dir = root.path().join("sessions");
    let source_path = root.path().join("unified.jsonl");
    let ledger_path = root.path().join("grok-inference.jsonl");
    write_session_summary(&sessions_dir, "session-1", "grok-4.6");
    fs::write(
        &source_path,
        inference_line(
            "2026-08-21T05:42:57.046Z",
            "session-1",
            1,
            148_540,
            148_224,
            1_356,
            949,
        ),
    )
    .expect("write source");

    assert_eq!(
        sync_ledger_at(&source_path, &ledger_path, &sessions_dir).expect("sync ledger"),
        1
    );
    let parsed = parse_ledger_file(&ledger_path, tz(), true);
    assert_eq!(parsed.errors, 0);
    assert_eq!(parsed.entries.len(), 1);
    let entry = &parsed.entries[0];
    assert_eq!(entry.to_stats().total_tokens(), 0);
    assert_eq!(entry.call_count, 0);
    assert!((entry.recorded_cost_usd.expect("calculated cost") - 0.082_88).abs() < 1e-12);
}

#[test]
fn rejects_inference_records_missing_required_fields() {
    let root = tempdir().expect("temp dir");
    let source_path = root.path().join("unified.jsonl");
    let ledger_path = root.path().join("grok-inference.jsonl");
    let sessions_dir = root.path().join("sessions");
    fs::write(
        &source_path,
        r#"{"ts":"2026-08-21T05:42:57.046Z","sid":"session-1","msg":"shell.turn.inference_done","ctx":{}}"#,
    )
    .expect("write incomplete inference");

    let error = sync_ledger_at(&source_path, &ledger_path, &sessions_dir)
        .expect_err("missing prompt tokens must fail ingestion");

    assert!(error.contains("prompt_tokens"));
    assert!(!ledger_path.exists());
}

#[test]
fn attributes_each_inference_to_the_model_active_at_that_time() {
    let root = tempdir().expect("temp dir");
    let sessions_dir = root.path().join("sessions");
    let source_path = root.path().join("unified.jsonl");
    let ledger_path = root.path().join("grok-inference.jsonl");
    write_session_summary(&sessions_dir, "session-1", "grok-4.6");
    fs::write(
        &source_path,
        [
            model_changed_line("2026-08-21T05:40:00Z", "session-1", "grok-4.5"),
            inference_line("2026-08-21T05:41:00Z", "session-1", 1, 100, 0, 10, 0),
            model_changed_line("2026-08-21T05:42:00Z", "session-1", "grok-4.6"),
            inference_line("2026-08-21T05:43:00Z", "session-1", 2, 200, 0, 20, 0),
        ]
        .join("\n"),
    )
    .expect("write model changes and inferences");

    sync_ledger_at(&source_path, &ledger_path, &sessions_dir).expect("sync ledger");
    let parsed = parse_ledger_file(&ledger_path, tz(), true);

    assert_eq!(parsed.errors, 0);
    assert_eq!(parsed.entries.len(), 2);
    assert_eq!(parsed.entries[0].model, "grok-4.5");
    assert_eq!(parsed.entries[1].model, "grok-4.6");
}

#[test]
fn ledger_deduplicates_and_survives_source_trimming() {
    let root = tempdir().expect("temp dir");
    let sessions_dir = root.path().join("sessions");
    let source_path = root.path().join("unified.jsonl");
    let ledger_path = root.path().join("grok-inference.jsonl");
    write_session_summary(&sessions_dir, "session-1", "grok-4.6");
    let first = inference_line("2026-08-21T05:42:57.046Z", "session-1", 1, 100, 50, 10, 4);
    let second = inference_line("2026-08-21T05:43:11.682Z", "session-1", 2, 200, 150, 20, 8);
    fs::write(&source_path, format!("{first}\n{second}\n")).expect("write source");

    assert_eq!(
        sync_ledger_at(&source_path, &ledger_path, &sessions_dir).expect("first sync"),
        2
    );
    assert_eq!(
        sync_ledger_at(&source_path, &ledger_path, &sessions_dir).expect("repeat sync"),
        2
    );

    fs::write(&source_path, format!("{second}\n")).expect("trim source head");
    assert_eq!(
        sync_ledger_at(&source_path, &ledger_path, &sessions_dir).expect("trimmed sync"),
        2
    );
    let parsed = parse_ledger_file(&ledger_path, tz(), true);
    assert_eq!(parsed.errors, 0);
    assert_eq!(parsed.entries.len(), 2);
    assert!(
        parsed
            .entries
            .iter()
            .all(|entry| entry.to_stats().total_tokens() == 0)
    );
    let cost: f64 = parsed
        .entries
        .iter()
        .filter_map(|entry| entry.recorded_cost_usd)
        .sum();
    assert!((cost - 0.000_48).abs() < 1e-12);
}

#[test]
fn malformed_live_tail_fails_closed_instead_of_returning_stale_ledger() {
    let root = tempdir().expect("temp dir");
    let sessions_dir = root.path().join("sessions");
    let source_path = root.path().join("unified.jsonl");
    let ledger_path = root.path().join("grok-inference.jsonl");
    write_session_summary(&sessions_dir, "session-1", "grok-4.6");
    fs::write(
        &source_path,
        inference_line("2026-08-21T05:42:57.046Z", "session-1", 1, 100, 50, 10, 4),
    )
    .expect("write initial source");
    assert_eq!(
        sync_ledger_at(&source_path, &ledger_path, &sessions_dir).expect("initial sync"),
        1
    );

    fs::write(
        &source_path,
        r#"{"msg":"shell.turn.inference_done","ctx":{"prompt_tokens":200"#,
    )
    .expect("write partial live record");

    let selected = sync_or_select_grok_file(&source_path, &ledger_path, &sessions_dir);
    assert_eq!(
        selected.file_name().and_then(|name| name.to_str()),
        Some(SYNC_ERROR_FILE)
    );
    let parsed = parse_grok_file_with_debug(&selected, tz(), true);
    assert_eq!(parsed.errors, 1);
    assert!(parsed.entries.is_empty());
}
