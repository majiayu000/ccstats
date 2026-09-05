use super::*;
use crate::core::DedupAccumulator;
use chrono::NaiveDate;
use std::io::Write;

fn cache(root: &Path) -> CodexCache {
    let cache = CodexCache::default();
    cache
        .connection
        .set(Ok(Mutex::new(open_cache(&root.join(CACHE_FILE)).unwrap())))
        .unwrap();
    cache
}

fn event(timestamp: &str, total: i64) -> String {
    format!(
        "{}\n",
        serde_json::json!({
            "type":"event_msg", "timestamp":timestamp,
            "payload":{"type":"token_count", "info":{
                "total_token_usage":{"input_tokens":total,"cached_input_tokens":0,"output_tokens":0,"total_tokens":total}
            }}
        })
    )
}

fn session(path: &Path) {
    fs::write(path, format!("{}\n{}\n{}{}",
        serde_json::json!({"type":"session_meta","payload":{"id":"shared-session","source":"cli"}}),
        serde_json::json!({"type":"turn_context","payload":{"model":"gpt-5"}}),
        event("2026-09-01T23:30:00.123456Z", 10),
        event("2026-09-02T00:30:00Z", 30),
    )).unwrap();
}

fn utc() -> Timezone {
    Timezone::Named(chrono_tz::UTC)
}

fn parse(cache: &CodexCache, path: &Path, filter: &DateFilter, tz: Timezone) -> ParseOutput {
    cache.parse(path, CodexScope::All, filter, tz, false)
}

fn assert_equal(actual: &ParseOutput, expected: &ParseOutput) {
    assert_eq!(actual.errors, expected.errors);
    assert_eq!(
        serde_json::to_value(&actual.entries).unwrap(),
        serde_json::to_value(&expected.entries).unwrap()
    );
    assert_eq!(
        actual
            .entries
            .iter()
            .map(|e| &e.session_key)
            .collect::<Vec<_>>(),
        expected
            .entries
            .iter()
            .map(|e| &e.session_key)
            .collect::<Vec<_>>()
    );
}

#[test]
fn warm_cache_preserves_all_fields_and_session_identity_across_reopening() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("session.jsonl");
    session(&path);
    let first = cache(root.path());
    let expected = parse(&first, &path, &DateFilter::default(), utc());
    assert_eq!(first.hits(), 0);
    let second = cache(root.path());
    assert_equal(
        &parse(&second, &path, &DateFilter::default(), utc()),
        &expected,
    );
    assert_eq!(second.hits(), 1);
}

#[test]
fn cache_writes_and_long_context_classification_survive_reopening() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("writes.jsonl");
    let mut lines = vec![
        serde_json::json!({"type":"session_meta","payload":{"id":"writes","source":"cli"}}),
        serde_json::json!({"type":"turn_context","payload":{"model":"gpt-5"}}),
    ];
    let mut total_input = 0;
    let mut total_writes = 0;
    for (index, input) in [272_000, 272_001, 100_000].into_iter().enumerate() {
        total_input += input;
        total_writes += 20_000;
        lines.push(serde_json::json!({
            "type":"event_msg", "timestamp":format!("2026-09-02T00:0{index}:00Z"),
            "payload":{"type":"token_count", "info":{
                "total_token_usage":{"input_tokens":total_input,
                    "cache_write_input_tokens":total_writes,"output_tokens":0,
                    "total_tokens":total_input}
            }}
        }));
    }
    fs::write(
        &path,
        lines
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n"),
    )
    .unwrap();
    let expected = parse_codex_file_with_scope(&path, utc(), false, CodexScope::All);
    assert_eq!(expected.errors, 0);
    assert_eq!(expected.entries.len(), 3);
    assert_eq!(
        expected
            .entries
            .iter()
            .map(|e| e.cache_creation)
            .sum::<i64>(),
        60_000
    );
    assert_eq!(expected.entries[0].to_stats().above_272k.cache_creation, 0);
    assert_eq!(
        expected.entries[1].to_stats().above_272k.cache_creation,
        20_000
    );
    let cold = cache(root.path());
    assert_equal(
        &parse(&cold, &path, &DateFilter::default(), utc()),
        &expected,
    );
    drop(cold);
    let reopened = cache(root.path());
    let warm = parse(&reopened, &path, &DateFilter::default(), utc());
    assert_equal(&warm, &expected);
    assert_eq!(reopened.hits(), 1);
    for (actual, expected) in warm.entries.iter().zip(&expected.entries) {
        assert_eq!(actual.to_stats().above_272k, expected.to_stats().above_272k);
    }
}

#[test]
fn cached_ranges_and_timezone_changes_match_uncached_filtering() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("old-session.jsonl");
    session(&path);
    let cache = cache(root.path());
    parse(&cache, &path, &DateFilter::default(), utc());
    let day = NaiveDate::from_ymd_opt(2026, 9, 2).unwrap();
    let filters = [
        DateFilter::new(Some(day), Some(day)),
        DateFilter::new(Some(day.succ_opt().unwrap()), None),
        DateFilter::default().with_exact_timestamp_range(
            "2026-09-01T23:30:00.123457Z".parse().unwrap(),
            "2026-09-02T00:30:00Z".parse().unwrap(),
        ),
    ];
    for timezone in [
        utc(),
        Timezone::Named(chrono_tz::Asia::Shanghai),
        Timezone::Named(chrono_tz::America::Los_Angeles),
    ] {
        for filter in &filters {
            let raw = parse_codex_file_with_scope(&path, timezone, false, CodexScope::All);
            let expected = ParseOutput {
                entries: DataLoader::filter_entries(raw.entries, filter, timezone),
                errors: raw.errors,
            };
            assert_equal(&parse(&cache, &path, filter, timezone), &expected);
        }
    }
    assert_eq!(cache.hits(), 9);
}

#[test]
fn date_filtered_first_run_keeps_history_in_the_cache() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("session.jsonl");
    session(&path);
    let cache = cache(root.path());
    let date = NaiveDate::from_ymd_opt(2026, 9, 2).unwrap();
    assert_eq!(
        parse(&cache, &path, &DateFilter::new(Some(date), None), utc())
            .entries
            .len(),
        1
    );
    assert_eq!(
        parse(&cache, &path, &DateFilter::default(), utc())
            .entries
            .len(),
        2
    );
    assert_eq!(cache.hits(), 1);
}

#[test]
fn append_rewrite_and_delete_invalidate_cached_records() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("session.jsonl");
    session(&path);
    let cache = cache(root.path());
    let filter = DateFilter::default();
    parse(&cache, &path, &filter, utc());
    fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(event("2026-09-03T00:00:00Z", 50).as_bytes())
        .unwrap();
    let appended = parse(&cache, &path, &filter, utc());
    assert_eq!(
        appended.entries.iter().map(|e| e.input_tokens).sum::<i64>(),
        50
    );
    assert_eq!(appended.entries.len(), 3);
    assert_eq!(cache.hits(), 0);
    let old_time = fs::metadata(&path).unwrap().modified().unwrap();
    let text = fs::read_to_string(&path).unwrap().replace("50", "60");
    fs::write(&path, text).unwrap();
    // Equal-length overwrite is detected even with restored mtime on Unix.
    #[cfg(unix)]
    fs::File::open(&path)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(old_time))
        .unwrap();
    #[cfg(not(unix))]
    let _ = old_time;
    assert_eq!(
        parse(&cache, &path, &filter, utc())
            .entries
            .iter()
            .map(|e| e.input_tokens)
            .sum::<i64>(),
        60
    );
    fs::remove_file(&path).unwrap();
    let deleted = parse(&cache, &path, &filter, utc());
    assert!(deleted.entries.is_empty());
    assert_eq!(deleted.errors, 1);
}

#[test]
fn scope_cache_does_not_mix_origins() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("session.jsonl");
    session(&path);
    let cache = cache(root.path());
    let filter = DateFilter::default();
    for scope in [
        CodexScope::All,
        CodexScope::Interactive,
        CodexScope::Exec,
        CodexScope::Subagent,
    ] {
        let expected = parse_codex_file_with_scope(&path, utc(), false, scope);
        assert_equal(&cache.parse(&path, scope, &filter, utc(), false), &expected);
        assert_equal(&cache.parse(&path, scope, &filter, utc(), false), &expected);
    }
    assert_eq!(cache.hits(), 4);
}

#[test]
fn malformed_source_is_not_cached_and_partial_tail_can_be_completed() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("session.jsonl");
    session(&path);
    let cache = cache(root.path());
    let filter = DateFilter::default();
    let line = event("2026-09-03T00:00:00Z", 60);
    let split = line.len() / 2;
    fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(&line.as_bytes()[..split])
        .unwrap();
    assert_eq!(parse(&cache, &path, &filter, utc()).errors, 1);
    assert_eq!(parse(&cache, &path, &filter, utc()).errors, 1);
    assert_eq!(cache.hits(), 0);
    fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(&line.as_bytes()[split..])
        .unwrap();
    let completed = parse(&cache, &path, &filter, utc());
    assert_eq!(completed.errors, 0);
    assert_eq!(completed.entries.len(), 3);
    assert_equal(&parse(&cache, &path, &filter, utc()), &completed);
    assert_eq!(cache.hits(), 1);
}

#[test]
fn corrupt_cache_is_rebuilt_from_source_without_losing_usage() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("session.jsonl");
    session(&path);
    let cache = cache(root.path());
    let filter = DateFilter::default();
    let expected = parse(&cache, &path, &filter, utc());
    cache
        .connection()
        .unwrap()
        .execute("UPDATE files SET payload = x'00'", [])
        .unwrap();
    assert_equal(&parse(&cache, &path, &filter, utc()), &expected);
    assert!(cache.reported_error.load(Ordering::Relaxed));
    assert_eq!(cache.hits(), 0);
    assert_equal(&parse(&cache, &path, &filter, utc()), &expected);
    assert_eq!(cache.hits(), 1);
}

#[test]
fn copied_and_archived_files_keep_cross_file_deduplication() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("session.jsonl");
    let copy = root.path().join("archived.jsonl");
    session(&path);
    fs::copy(&path, &copy).unwrap();
    let cache = cache(root.path());
    let filter = DateFilter::default();
    for _ in 0..2 {
        let mut all = DedupAccumulator::new();
        for p in [&path, &copy] {
            all.extend(parse(&cache, p, &filter, utc()).entries);
        }
        let (entries, skipped) = all.finalize();
        // The initial event is file-scoped; subsequent events are source-wide.
        assert_eq!(skipped, 1);
        assert_eq!(entries.iter().map(|e| e.input_tokens).sum::<i64>(), 40);
    }
    assert_eq!(cache.hits(), 2);
}
