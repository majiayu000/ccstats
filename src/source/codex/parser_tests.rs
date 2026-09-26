use super::*;

#[test]
fn escaped_working_directories_preserve_session_and_model_metadata() {
    for cwd in [
        r"C:\Users\example\项目",
        r"\\server\share\project",
        "/work/project\"quoted\"",
        "/work/project",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("usage.jsonl");
        let rows = [
            serde_json::json!({"type": "session_meta", "payload": {
                "id": "path-test-session", "source": "cli", "cwd": cwd
            }}),
            serde_json::json!({"type": "turn_context", "payload": {
                "model": "gpt-6-astra", "cwd": cwd
            }}),
            serde_json::json!({
                "timestamp": "2026-09-05T00:51:00Z", "type": "event_msg",
                "payload": {"type": "token_count", "info": {
                    "total_token_usage": {"input_tokens": 100, "output_tokens": 5}
                }}
            }),
        ];
        let lines = rows.map(|row| row.to_string()).join("\n");
        // Exercise escaped Unicode as well as backslashes and quotes.
        std::fs::write(&path, lines.replace("项目", r"\u9879\u76ee")).unwrap();

        for result in [
            parse_codex_file_for_quota(&path, Timezone::Named(chrono_tz::UTC)),
            parse_codex_file_with_scope(
                &path,
                Timezone::Named(chrono_tz::UTC),
                false,
                CodexScope::Interactive,
            ),
        ] {
            assert_eq!(result.errors, 0, "cwd: {cwd}");
            assert_eq!(result.entries.len(), 1, "cwd: {cwd}");
            let entry = &result.entries[0];
            assert_eq!(entry.project_path, cwd);
            assert_eq!(entry.session_id, "path-test-session");
            assert_eq!(entry.model, "gpt-6-astra");
            assert_eq!(entry.to_stats().total_tokens(), 105);
        }
    }
}

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

fn native_rows(rows: &[serde_json::Value]) -> ParseOutput {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("native.jsonl");
    std::fs::write(
        &path,
        rows.iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n"),
    )
    .unwrap();
    parse_codex_file_for_quota(&path, Timezone::Named(chrono_tz::UTC))
}
fn native_count(total: i64, last: Option<i64>) -> serde_json::Value {
    let mut info =
        serde_json::json!({"total_token_usage":{"input_tokens":total,"output_tokens":0}});
    if let Some(last) = last {
        info["last_token_usage"] = serde_json::json!({"input_tokens":last});
    }
    serde_json::json!({"type":"event_msg","timestamp":"2026-09-05T00:51:00Z","payload":{"type":"token_count","info":info}})
}
#[test]
fn native_deltas_duplicate_vectors_and_resets_preserve_statistics() {
    let output = native_rows(&[
        native_count(100, None),
        native_count(100, None),
        native_count(20, None),
        native_count(30, None),
    ]);
    assert_eq!(output.errors, 0);
    assert_eq!(
        output
            .entries
            .iter()
            .map(|e| e.input_tokens)
            .collect::<Vec<_>>(),
        vec![100, 10]
    );
}
#[test]
fn last_sample_wins_over_cumulative_difference() {
    let output = native_rows(&[native_count(100, None), native_count(200, Some(7))]);
    assert_eq!(output.entries[1].input_tokens, 7);
}
#[test]
fn response_only_files_are_supported_without_combining_ledgers() {
    let response = serde_json::json!({"type":"token_usage_record","timestamp":"2026-09-05T00:51:00Z",
        "payload":{"response_id":"r1","usage":{"input_tokens":9,"output_tokens":2}}});
    let output = native_rows(std::slice::from_ref(&response));
    assert_eq!(output.errors, 0);
    assert_eq!(output.entries[0].input_tokens, 9);
    let mixed = native_rows(&[native_count(100, None), response]);
    assert_eq!(mixed.entries.len(), 1);
    assert_eq!(mixed.entries[0].input_tokens, 100);
}
#[test]
fn native_model_fallbacks_and_cache_aliases() {
    for info in [
        serde_json::json!({"model":"chosen"}),
        serde_json::json!({"model":" ","model_name":"chosen"}),
        serde_json::json!({"metadata":{"model":"chosen"}}),
    ] {
        let mut row = native_count(100, None);
        for (key, value) in info.as_object().unwrap() {
            row["payload"]["info"][key] = value.clone();
        }
        row["payload"]["info"]["total_token_usage"]["cache_read_input_tokens"] =
            serde_json::json!(20);
        let output = native_rows(&[row]);
        assert_eq!(output.errors, 0);
        assert_eq!(output.entries[0].model, "chosen");
        assert_eq!(output.entries[0].cache_read, 20);
        assert_eq!(output.entries[0].input_tokens, 80);
    }
}
#[test]
fn ide_is_in_interactive_scope_and_subagent_object_wins() {
    for (source, scope, count) in [
        (serde_json::json!("vscode"), CodexScope::Interactive, 1),
        (
            serde_json::json!({"subagent":{}}),
            CodexScope::Interactive,
            0,
        ),
        (serde_json::json!({"subagent":{}}), CodexScope::Subagent, 1),
        (serde_json::json!("exec"), CodexScope::Exec, 1),
        (serde_json::json!("future"), CodexScope::Subagent, 0),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("scope.jsonl");
        let meta = serde_json::json!({"type":"session_meta","payload":{"source":source,"originator":"Codex Desktop"}});
        std::fs::write(&path, format!("{}\n{}", meta, native_count(100, None))).unwrap();
        let output =
            parse_codex_file_with_scope(&path, Timezone::Named(chrono_tz::UTC), false, scope);
        assert_eq!(output.errors, 0);
        assert_eq!(output.entries.len(), count);
    }
}
