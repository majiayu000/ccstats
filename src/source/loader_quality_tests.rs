use std::path::{Path, PathBuf};

use chrono::NaiveDate;

use crate::core::{DateFilter, RawEntry};
use crate::source::{Capabilities, ParseOutput, Source};
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
        cache_read: 0,
        reasoning_tokens: 0,
        stop_reason: Some("end_turn".to_string()),
    }
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
