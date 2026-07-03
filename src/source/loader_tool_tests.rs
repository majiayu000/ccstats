use std::path::{Path, PathBuf};

use chrono::NaiveDate;

use crate::core::{DateFilter, ToolCall};
use crate::source::{Capabilities, ParseOutput, Source};
use crate::utils::Timezone;

use super::load_tool_calls;

struct TestToolSource {
    has_tool_calls: bool,
}

impl Source for TestToolSource {
    fn name(&self) -> &'static str {
        "test"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            has_tool_calls: self.has_tool_calls,
            ..Capabilities::default()
        }
    }

    fn find_files(&self) -> Vec<PathBuf> {
        Vec::new()
    }

    fn parse_file(&self, _path: &Path, _timezone: Timezone, _debug: bool) -> ParseOutput {
        ParseOutput::default()
    }

    fn find_tool_call_files(&self) -> Vec<PathBuf> {
        vec![PathBuf::from("tool-calls.jsonl")]
    }

    fn parse_tool_call_file(&self, _path: &Path, _timezone: Timezone) -> Vec<ToolCall> {
        vec![ToolCall {
            name: "Read".to_string(),
            date_str: "2026-02-06".to_string(),
            identity: None,
        }]
    }
}

fn tz() -> Timezone {
    Timezone::parse(None).unwrap()
}

#[test]
fn load_tool_calls_dispatches_through_source_trait() {
    let source = TestToolSource {
        has_tool_calls: true,
    };
    let filter = DateFilter::new(
        NaiveDate::from_ymd_opt(2026, 2, 6),
        NaiveDate::from_ymd_opt(2026, 2, 6),
    );

    let calls = load_tool_calls(&source, &filter, tz());

    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].name, "Read");
}

#[test]
fn load_tool_calls_returns_empty_without_capability() {
    let source = TestToolSource {
        has_tool_calls: false,
    };
    let filter = DateFilter::new(None, None);

    let calls = load_tool_calls(&source, &filter, tz());

    assert!(calls.is_empty());
}
