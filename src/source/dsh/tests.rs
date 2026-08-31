use std::fs;

use serde_json::json;

use super::{DshSource, Source};
use crate::utils::Timezone;

#[test]
fn discovery_reuses_one_root_encoding_snapshot_for_all_parses() {
    let root = tempfile::tempdir().expect("temporary DSH root");
    let sessions = root.path().join("sessions");
    let session = sessions.join("--workspace-project--/session-a");
    fs::create_dir_all(&session).expect("create session directory");
    let header = json!({
        "type": "session",
        "version": 0,
        "id": "session-a",
        "createdAt": 1_788_145_200_000_i64,
        "cwd": "/workspace/project",
        "delegationDepth": 0
    });
    let message = json!({
        "type": "assistant/message",
        "seq": 0,
        "time": 1_788_145_200_000_i64,
        "data": {
            "turn": 1,
            "step": 1,
            "message": {
                "id": "message-a",
                "role": "assistant",
                "content": [],
                "source": {"kind": "model", "provider": "fixture", "model": "deepseek-v4"}
            },
            "usage": {"inputTokens": 3, "outputTokens": 1}
        }
    });
    fs::write(
        session.join("session.jsonl"),
        format!("{header}\n{message}\n"),
    )
    .expect("write plain session");

    let source = DshSource::new();
    let files = source.discover_root(&sessions);
    assert_eq!(files.len(), 1);

    let late_zstd = sessions.join("--workspace-other--/session-b/session.jsonl.zstd");
    fs::create_dir_all(late_zstd.parent().expect("zstd session parent"))
        .expect("create zstd session directory");
    fs::write(late_zstd, b"late writer data").expect("write late zstd session");

    let parsed = source.parse_file(&files[0], Timezone::Named(chrono_tz::UTC), false);
    assert_eq!(parsed.errors, 0);
    assert_eq!(parsed.entries.len(), 1);
}
