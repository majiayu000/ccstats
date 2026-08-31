use super::*;

use crate::core::DedupAccumulator;

fn message() -> String {
    r#"{"type":"message","id":"call","timestamp":"2026-08-31T01:00:02Z","message":{"role":"assistant","provider":"openai","model":"gpt-5","timestamp":1788138002000,"usage":{"input":1,"output":1,"cacheRead":0,"cacheWrite":0}}}"#.to_string()
}

#[test]
fn fork_copy_deduplication_keeps_original_ownership() {
    let fork = parse_lines(
        vec![
            r#"{"type":"session","version":3,"id":"fork","timestamp":"2026-08-31T01:01:00Z","cwd":"/fork"}"#.to_string(),
            message(),
        ],
        Timezone::Named(chrono_tz::UTC),
    );
    let original = parse_lines(
        vec![
            r#"{"type":"session","version":3,"id":"original","timestamp":"2026-08-31T01:00:00Z","cwd":"/original"}"#.to_string(),
            message(),
        ],
        Timezone::Named(chrono_tz::UTC),
    );
    let mut dedup = DedupAccumulator::new();
    dedup.extend(fork.entries);
    dedup.extend(original.entries);

    let (entries, skipped) = dedup.finalize();

    assert_eq!(skipped, 1);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].project_path, "/original");
}

#[test]
fn session_without_v3_marker_is_rejected() {
    let parsed = parse_lines(
        vec![
            r#"{"type":"session","id":"legacy","timestamp":"2026-08-31T01:00:00Z","cwd":"/legacy"}"#.to_string(),
            message(),
        ],
        Timezone::Named(chrono_tz::UTC),
    );

    assert!(parsed.entries.is_empty());
    assert_eq!(parsed.errors, 2);
}

#[test]
fn v3_session_without_valid_header_timestamp_is_rejected() {
    for timestamp in [None, Some("not-a-timestamp")] {
        let timestamp =
            timestamp.map_or_else(String::new, |value| format!(r#","timestamp":"{value}""#));
        let parsed = parse_lines(
            vec![
                format!(r#"{{"type":"session","version":3,"id":"fork"{timestamp},"cwd":"/fork"}}"#),
                message(),
            ],
            Timezone::Named(chrono_tz::UTC),
        );

        assert!(parsed.entries.is_empty());
        assert_eq!(parsed.errors, 2);
    }
}
