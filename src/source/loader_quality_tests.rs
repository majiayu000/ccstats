use std::path::{Path, PathBuf};

use chrono::{DateTime, NaiveDate, Utc};

use crate::core::{DateFilter, RawEntry};
use crate::source::{Capabilities, ParseOutput, Source, load_entries};
use crate::utils::Timezone;

use super::load_daily;

struct TestSource {
    needs_dedup: bool,
    files: Vec<(PathBuf, Vec<RawEntry>, usize)>,
}

impl Source for TestSource {
    fn name(&self) -> &'static str {
        "test"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            needs_dedup: self.needs_dedup,
            ..Capabilities::default()
        }
    }

    fn find_files(&self) -> Vec<PathBuf> {
        self.files.iter().map(|(path, _, _)| path.clone()).collect()
    }

    fn parse_file(&self, path: &Path, _timezone: Timezone, _debug: bool) -> ParseOutput {
        let (_, entries, errors) = self
            .files
            .iter()
            .find(|(candidate, _, _)| candidate == path)
            .expect("known test path");
        ParseOutput {
            entries: entries.clone(),
            errors: *errors,
        }
    }
}

fn entry(id: &str, input_tokens: i64) -> RawEntry {
    RawEntry {
        timestamp: "2026-02-06T12:00:00Z".to_string(),
        timestamp_ms: 1_770_379_200_000,
        date_str: "2026-02-06".to_string(),
        message_id: Some(id.to_string()),
        session_key: "session".to_string(),
        session_id: "session".to_string(),
        project_path: String::new(),
        model: "model".to_string(),
        input_tokens,
        output_tokens: 0,
        cache_creation: 0,
        cache_creation_1h: 0,
        cache_read: 0,
        reasoning_tokens: 0,
        stop_reason: Some("end_turn".to_string()),
        cost_kind: crate::core::CostKind::Real,
        endpoint: crate::core::Endpoint::Unknown,
        call_count: 1,
        reported_total_tokens: None,
        recorded_cost_usd: None,
        api_equivalent_priced_tokens: 0,
        api_equivalent_coverage_tokens: 0,
    }
}

fn entry_at(id: &str, timestamp: &str) -> RawEntry {
    let mut entry = entry(id, 10);
    entry.timestamp = timestamp.to_string();
    entry.timestamp_ms = timestamp
        .parse::<DateTime<Utc>>()
        .expect("valid timestamp")
        .timestamp_millis();
    entry.date_str = "2026-08-21".to_string();
    entry
}

fn filter() -> DateFilter {
    DateFilter::new(
        NaiveDate::from_ymd_opt(2026, 2, 6),
        NaiveDate::from_ymd_opt(2026, 2, 6),
    )
}

fn tz() -> Timezone {
    Timezone::parse(Some("UTC")).unwrap()
}

#[test]
fn load_daily_reports_parse_errors_not_file_count() {
    let source = TestSource {
        needs_dedup: false,
        files: vec![
            (PathBuf::from("a.jsonl"), vec![entry("a", 10)], 0),
            (PathBuf::from("b.jsonl"), vec![entry("b", 20)], 0),
        ],
    };

    let result = load_daily(&source, &filter(), tz(), true, false);

    assert_eq!(result.valid, 2);
    assert_eq!(result.parse_errors, 0);
}

#[test]
fn load_daily_dedup_reports_skipped_and_parse_errors() {
    let source = TestSource {
        needs_dedup: true,
        files: vec![
            (PathBuf::from("a.jsonl"), vec![entry("dup", 10)], 1),
            (PathBuf::from("b.jsonl"), vec![entry("dup", 20)], 2),
        ],
    };

    let result = load_daily(&source, &filter(), tz(), true, false);

    assert_eq!(result.valid, 1);
    assert_eq!(result.skipped, 1);
    assert_eq!(result.parse_errors, 3);
}

#[test]
fn load_entries_preserves_non_deduplicated_records_without_message_identity() {
    let mut record = entry("unused", 10);
    record.message_id = None;
    record.stop_reason = None;
    let source = TestSource {
        needs_dedup: false,
        files: vec![(PathBuf::from("usage.jsonl"), vec![record], 2)],
    };

    let (entries, skipped, parse_errors) = load_entries(&source, &filter(), tz());

    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].input_tokens, 10);
    assert_eq!(skipped, 0);
    assert_eq!(parse_errors, 2);
}

#[test]
fn load_daily_honors_exact_timestamp_bounds() {
    let since = "2026-08-21T05:41:00Z"
        .parse::<DateTime<Utc>>()
        .expect("valid since")
        .timestamp_millis();
    let until = "2026-08-21T05:43:00Z"
        .parse::<DateTime<Utc>>()
        .expect("valid until")
        .timestamp_millis();
    let source = TestSource {
        needs_dedup: false,
        files: vec![(
            PathBuf::from("grok-ledger.jsonl"),
            {
                let mut inside = entry_at("inside", "2026-08-21T05:42:00Z");
                inside.date_str = "invalid".to_string();
                vec![
                    entry_at("before", "2026-08-21T05:40:00Z"),
                    inside,
                    entry_at("after", "2026-08-21T05:44:00Z"),
                ]
            },
            0,
        )],
    };
    let filter = DateFilter::new(None, None).with_timestamp_range(since, until);

    let result = load_daily(&source, &filter, tz(), true, false);

    assert_eq!(result.valid, 1);
    assert_eq!(result.day_stats["2026-08-21"].stats.input_tokens, 10);
}
