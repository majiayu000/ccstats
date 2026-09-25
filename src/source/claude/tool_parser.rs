//! Client tool statistics projected from native session events.
use crate::source::session_reader;
use crate::{
    consts::{DATE_FORMAT, UNKNOWN},
    core::{ToolCall, ToolCallIdentity},
    utils::Timezone,
};
use agent_sessions::{Agent, CodexUsageMode, Event, EventKinds, ToolCallKind};
use std::path::Path;
pub(crate) fn parse_tool_calls(path: &Path, timezone: Timezone) -> Vec<ToolCall> {
    let Ok(reader) = session_reader::reader(
        path,
        Agent::ClaudeCode,
        EventKinds::TOOL_CALL,
        CodexUsageMode::TokenCount,
    ) else {
        return Vec::new();
    };
    let key = path.display().to_string();
    reader
        .filter_map(Result::ok)
        .filter_map(|event| {
            let Event::ToolCall(call) = event.value else {
                return None;
            };
            if call.kind != ToolCallKind::Function {
                return None;
            }
            let identity = event
                .message_id
                .zip(call.id)
                .map(|(message, id)| ToolCallIdentity::new(&key, &message, &id));
            Some(ToolCall {
                name: call.name,
                identity,
                date_str: event.at.map_or_else(
                    || UNKNOWN.into(),
                    |at| {
                        timezone
                            .to_fixed_offset(at)
                            .date_naive()
                            .format(DATE_FORMAT)
                            .to_string()
                    },
                ),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::aggregate_tools;
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
        let line = r#"{"type":"progress","data":{"message":{"timestamp":"2025-03-01T10:00:00Z","message":{"id":"msg_1","content":[{"type":"tool_use","name":"Grep","id":"t1","input":{}}]}}},"toolUseID":"agent_123"}"#;
        let f = write_jsonl(&[line]);
        let calls = parse_tool_calls(f.path(), tz());
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "Grep");
    }

    #[test]
    fn repeated_tool_use_records_are_deduplicated_by_identity() {
        let line = r#"{"type":"assistant","timestamp":"2025-03-01T10:00:00Z","message":{"id":"msg_1","content":[{"type":"tool_use","name":"Read","id":"toolu_1","input":{}}]}}"#;
        let f = write_jsonl(&[line, line]);
        let calls = parse_tool_calls(f.path(), tz());
        let summary = aggregate_tools(&calls);

        assert_eq!(calls.len(), 2);
        assert_eq!(summary.total, 1);
        assert_eq!(summary.tools.len(), 1);
        assert_eq!(summary.tools[0].name, "Read");
        assert_eq!(summary.tools[0].calls, 1);
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
