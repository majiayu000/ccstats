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
fn valid_live_tail_reports_a_malformed_ledger_sync_failure() {
    let root = tempdir().expect("temp dir");
    let sessions_dir = root.path().join("sessions");
    let source_path = root.path().join(UNIFIED_LOG);
    let ledger_path = root.path().join(LEDGER_FILE);
    write_session_summary(&sessions_dir, "session-1", "grok-4.6");
    fs::write(
        &source_path,
        inference_line("2026-08-21T05:42:57.046Z", "session-1", 1, 100, 0, 10, 0),
    )
    .expect("write valid live source");
    fs::write(&ledger_path, "{not valid ledger json}\n").expect("write malformed ledger");

    let selected = sync_or_select_grok_file(&source_path, &ledger_path, &sessions_dir);
    let parsed = parse_grok_file_with_debug(&selected, tz(), true);

    assert_eq!(parsed.errors, 1);
    assert!(parsed.entries.is_empty());
}
