//! Parser for tool_use blocks in Claude Code JSONL logs
//!
//! Extracts tool call names from assistant messages using partial
//! serde_json::Value parsing to avoid full deserialization.

use chrono::{DateTime, Utc};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use crate::consts::{DATE_FORMAT, UNKNOWN};
use crate::core::ToolCall;
use crate::utils::Timezone;

/// Parse a single JSONL file and extract tool calls
pub(crate) fn parse_tool_calls(path: &Path, timezone: Timezone) -> Vec<ToolCall> {
    let file = match File::open(path) {
        Ok(f) => f,
        Err(_) => return Vec::new(),
    };
    let reader = BufReader::new(file);

    let mut calls = Vec::new();
    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => continue,
        };
        if line.trim().is_empty() {
            continue;
        }

        // Quick pre-filter: skip lines that can't contain tool_use
        if !line.contains("\"tool_use\"") {
            continue;
        }

        let val: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        // Extract timestamp for date filtering
        let date_str = extract_date(&val, timezone);

        // Handle direct assistant messages: {"type":"assistant","message":{"content":[...]}}
        if let Some(content) = val
            .pointer("/message/content")
            .and_then(serde_json::Value::as_array)
        {
            for item in content {
                if let Some(name) = extract_tool_name(item) {
                    calls.push(ToolCall {
                        name,
                        date_str: date_str.clone(),
                    });
                }
            }
        }

        // Handle progress messages (subagent): {"type":"progress","data":{"message":{"message":{"content":[...]}}}}
        if let Some(content) = val
            .pointer("/data/message/message/content")
            .and_then(serde_json::Value::as_array)
        {
            for item in content {
                if let Some(name) = extract_tool_name(item) {
                    calls.push(ToolCall {
                        name,
                        date_str: date_str.clone(),
                    });
                }
            }
        }
    }

    calls
}

fn extract_tool_name(item: &serde_json::Value) -> Option<String> {
    if item.get("type")?.as_str()? == "tool_use" {
        Some(item.get("name")?.as_str()?.to_string())
    } else {
        None
    }
}

fn extract_date(val: &serde_json::Value, timezone: Timezone) -> String {
    // Try direct timestamp field
    let ts = val
        .get("timestamp")
        .and_then(serde_json::Value::as_str)
        // Try nested in progress messages
        .or_else(|| {
            val.pointer("/data/message/timestamp")
                .and_then(serde_json::Value::as_str)
        });

    if let Some(ts) = ts {
        if let Ok(utc_dt) = ts.parse::<DateTime<Utc>>() {
            let local_dt = timezone.to_fixed_offset(utc_dt);
            return local_dt.date_naive().format(DATE_FORMAT).to_string();
        }
    }

    UNKNOWN.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn write_jsonl(lines: &[&str]) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        for line in lines {
            writeln!(f, "{line}").unwrap();
        }
        f.flush().unwrap();
        f
    }

    fn tz() -> Timezone {
        Timezone::parse(None).unwrap()
    }

    #[test]
    fn parse_direct_tool_use() {
        let line = r#"{"type":"assistant","timestamp":"2025-03-01T10:00:00Z","message":{"content":[{"type":"tool_use","name":"Read","id":"t1","input":{}}]}}"#;
        let f = write_jsonl(&[line]);
        let calls = parse_tool_calls(f.path(), tz());
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "Read");
    }

    #[test]
    fn parse_multiple_tools_in_one_message() {
        let line = r#"{"type":"assistant","timestamp":"2025-03-01T10:00:00Z","message":{"content":[{"type":"tool_use","name":"Read","id":"t1","input":{}},{"type":"tool_use","name":"Bash","id":"t2","input":{}},{"type":"text","text":"hello"}]}}"#;
        let f = write_jsonl(&[line]);
        let calls = parse_tool_calls(f.path(), tz());
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].name, "Read");
        assert_eq!(calls[1].name, "Bash");
    }

    #[test]
    fn parse_progress_tool_use() {
        let line = r#"{"type":"progress","data":{"message":{"timestamp":"2025-03-01T10:00:00Z","message":{"content":[{"type":"tool_use","name":"Grep","id":"t1","input":{}}]}}},"toolUseID":"agent_123"}"#;
        let f = write_jsonl(&[line]);
        let calls = parse_tool_calls(f.path(), tz());
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "Grep");
    }

    #[test]
    fn skip_non_tool_use_lines() {
        let lines = &[
            r#"{"type":"human","message":{"content":[{"type":"text","text":"hello"}]}}"#,
            r#"{"type":"assistant","timestamp":"2025-03-01T10:00:00Z","message":{"content":[{"type":"text","text":"hi"}]}}"#,
        ];
        let f = write_jsonl(lines);
        let calls = parse_tool_calls(f.path(), tz());
        assert!(calls.is_empty());
    }

    #[test]
    fn empty_file() {
        let f = write_jsonl(&[]);
        let calls = parse_tool_calls(f.path(), tz());
        assert!(calls.is_empty());
    }
}
