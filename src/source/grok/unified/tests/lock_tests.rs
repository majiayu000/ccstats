use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use fs4::FileExt;

use super::*;

#[test]
fn sync_waits_for_the_ledger_lock_before_merging() {
    let root = tempdir().expect("temp dir");
    let sessions_dir = root.path().join("sessions");
    let source_path = root.path().join("unified.jsonl");
    let ledger_path = root.path().join(LEDGER_FILE);
    let lock_path = root.path().join("inference-v1.lock");
    write_session_summary(&sessions_dir, "session-1", "grok-4.6");
    fs::write(
        &source_path,
        inference_line("2026-08-21T05:42:57.046Z", "session-1", 1, 100, 0, 10, 0),
    )
    .expect("write source");
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock_path)
        .expect("open lock file");
    FileExt::lock(&lock).expect("hold ledger lock");

    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        sender
            .send(sync_ledger_at(&source_path, &ledger_path, &sessions_dir))
            .expect("send sync result");
    });

    assert!(matches!(
        receiver.recv_timeout(Duration::from_millis(200)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));
    FileExt::unlock(&lock).expect("release ledger lock");
    assert_eq!(
        receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("sync finishes after unlock")
            .expect("sync succeeds"),
        1
    );
}

#[test]
fn priced_token_coverage_honors_exact_timestamp_range() {
    let records = [
        InferenceRecord {
            event_key: "first".to_string(),
            timestamp: "2026-08-21T05:00:00Z".to_string(),
            session_id: "session-1".to_string(),
            session_key: "session-1".to_string(),
            project_path: String::new(),
            model: "grok-4.6".to_string(),
            prompt_tokens: 100,
            cached_prompt_tokens: 0,
            completion_tokens: 10,
            reasoning_tokens: 0,
        },
        InferenceRecord {
            event_key: "second".to_string(),
            timestamp: "2026-08-21T06:00:00Z".to_string(),
            session_id: "session-1".to_string(),
            session_key: "session-1".to_string(),
            project_path: String::new(),
            model: "grok-4.6".to_string(),
            prompt_tokens: 200,
            cached_prompt_tokens: 0,
            completion_tokens: 20,
            reasoning_tokens: 0,
        },
    ];
    let since = "2026-08-21T05:30:00Z"
        .parse::<DateTime<Utc>>()
        .expect("valid since")
        .timestamp_millis();
    let until = "2026-08-21T06:30:00Z"
        .parse::<DateTime<Utc>>()
        .expect("valid until")
        .timestamp_millis();
    let filter = DateFilter::new(None, None).with_timestamp_range(since, until);

    assert_eq!(priced_tokens_in_records(records, &filter, tz()), 220);
}
