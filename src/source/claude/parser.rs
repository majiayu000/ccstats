//! Claude Code JSONL parser
//!
//! Parses JSONL logs from ~/.claude/projects/ directory.

use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use crate::consts::{DATE_FORMAT, UNKNOWN};
use crate::core::RawEntry;
use crate::utils::{Timezone, parse_debug_enabled};

// ============================================================================
// Internal types for JSONL parsing
// ============================================================================

#[derive(Debug, Deserialize)]
struct UsageEntry {
    timestamp: Option<String>,
    message: Option<Message>,
}

#[derive(Debug, Deserialize)]
struct Message {
    id: Option<String>,
    model: Option<String>,
    stop_reason: Option<String>,
    usage: Option<Usage>,
}

#[derive(Debug, Deserialize, Clone, Default)]
struct Usage {
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    cache_creation_input_tokens: Option<i64>,
    cache_read_input_tokens: Option<i64>,
}

// ============================================================================
// File discovery
// ============================================================================

pub(super) fn find_claude_files() -> Vec<PathBuf> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    let claude_path = home.join(".claude").join("projects");

    let mut files = Vec::new();
    if let Ok(entries) = glob::glob(&format!("{}/**/*.jsonl", claude_path.display())) {
        for entry in entries.flatten() {
            files.push(entry);
        }
    }
    files
}

// ============================================================================
// Parsing
// ============================================================================

/// Normalize model name by removing prefixes and date suffixes
fn normalize_model_name(model: &str) -> String {
    let name = model
        .strip_prefix("anthropic.")
        .unwrap_or(model)
        .to_string();
    let name = name.strip_prefix("claude-").unwrap_or(&name).to_string();

    // Remove date suffix like -20251101
    if let Some(pos) = name.rfind('-') {
        let suffix = &name[pos + 1..];
        if suffix.len() == 8 && suffix.chars().all(|c| c.is_ascii_digit()) {
            return name[..pos].to_string();
        }
    }

    name
}

pub(super) fn parse_claude_file(path: &Path, timezone: Timezone) -> Vec<RawEntry> {
    let session_id = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(UNKNOWN)
        .to_string();

    let project_path = path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|s| s.to_str())
        .unwrap_or(UNKNOWN)
        .to_string();

    let file = match File::open(path) {
        Ok(f) => f,
        Err(err) => {
            if parse_debug_enabled() {
                eprintln!("Failed to open {}: {}", path.display(), err);
            }
            return Vec::new();
        }
    };
    let reader = BufReader::new(file);

    let mut entries = Vec::new();
    for (line_no, line) in reader.lines().enumerate() {
        let line = match line {
            Ok(line) => line,
            Err(err) => {
                if parse_debug_enabled() {
                    eprintln!(
                        "Failed to read line {} in {}: {}",
                        line_no + 1,
                        path.display(),
                        err
                    );
                }
                continue;
            }
        };

        if line.trim().is_empty() {
            continue;
        }

        let entry: UsageEntry = match serde_json::from_str(&line) {
            Ok(entry) => entry,
            Err(err) => {
                if parse_debug_enabled() {
                    eprintln!(
                        "Invalid JSON at {}:{}: {}",
                        path.display(),
                        line_no + 1,
                        err
                    );
                }
                continue;
            }
        };

        if let Some(entry) = parse_entry(
            entry,
            path,
            &session_id,
            &project_path,
            timezone,
            line_no + 1,
        ) {
            entries.push(entry);
        }
    }
    entries
}

fn parse_entry(
    entry: UsageEntry,
    path: &Path,
    session_id: &str,
    project_path: &str,
    timezone: Timezone,
    line_no: usize,
) -> Option<RawEntry> {
    let ts = entry.timestamp?;
    let msg = entry.message?;
    let usage = msg.usage?;

    let model = msg
        .model
        .as_deref().map_or_else(|| UNKNOWN.to_string(), normalize_model_name);

    if model == "<synthetic>" || model.is_empty() {
        return None;
    }

    // Parse timestamp
    let utc_dt = match ts.parse::<DateTime<Utc>>() {
        Ok(dt) => dt,
        Err(err) => {
            if parse_debug_enabled() {
                eprintln!(
                    "Invalid timestamp at {}:{}: {} ({})",
                    path.display(),
                    line_no,
                    ts,
                    err
                );
            }
            return None;
        }
    };
    let local_dt = timezone.to_fixed_offset(utc_dt);
    let date = local_dt.date_naive();

    Some(RawEntry {
        timestamp: ts,
        timestamp_ms: utc_dt.timestamp_millis(),
        date_str: date.format(DATE_FORMAT).to_string(),
        message_id: msg.id,
        session_id: session_id.to_string(),
        project_path: project_path.to_string(),
        model,
        input_tokens: usage.input_tokens.unwrap_or(0),
        output_tokens: usage.output_tokens.unwrap_or(0),
        cache_creation: usage.cache_creation_input_tokens.unwrap_or(0),
        cache_read: usage.cache_read_input_tokens.unwrap_or(0),
        reasoning_tokens: 0, // Claude doesn't have reasoning tokens
        stop_reason: msg.stop_reason,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================================================
    // normalize_model_name
    // ========================================================================

    #[test]
    fn test_normalize_strips_anthropic_and_claude_prefix_and_date() {
        assert_eq!(
            normalize_model_name("anthropic.claude-3-5-sonnet-20241022"),
            "3-5-sonnet"
        );
    }

    #[test]
    fn test_normalize_strips_claude_prefix_and_date() {
        assert_eq!(normalize_model_name("claude-3-opus-20240229"), "3-opus");
    }

    #[test]
    fn test_normalize_no_prefix_no_date() {
        assert_eq!(normalize_model_name("gpt-4"), "gpt-4");
    }

    #[test]
    fn test_normalize_only_anthropic_prefix() {
        assert_eq!(
            normalize_model_name("anthropic.some-model"),
            "some-model"
        );
    }

    #[test]
    fn test_normalize_short_suffix_not_stripped() {
        // Suffix "123" is only 3 chars, not 8 — should NOT be stripped
        assert_eq!(normalize_model_name("claude-model-123"), "model-123");
    }

    #[test]
    fn test_normalize_non_digit_suffix_not_stripped() {
        assert_eq!(
            normalize_model_name("claude-model-2024abcd"),
            "model-2024abcd"
        );
    }

    #[test]
    fn test_normalize_no_dash() {
        assert_eq!(normalize_model_name("singleword"), "singleword");
    }

    #[test]
    fn test_normalize_anthropic_claude_no_date() {
        assert_eq!(
            normalize_model_name("anthropic.claude-4-sonnet"),
            "4-sonnet"
        );
    }

    // ========================================================================
    // parse_entry
    // ========================================================================

    fn make_timezone() -> Timezone {
        Timezone::Local
    }

    fn make_usage_entry(
        timestamp: &str,
        model: Option<&str>,
        stop_reason: Option<&str>,
        input: i64,
        output: i64,
    ) -> UsageEntry {
        UsageEntry {
            timestamp: Some(timestamp.to_string()),
            message: Some(Message {
                id: Some("msg_001".to_string()),
                model: model.map(|s| s.to_string()),
                stop_reason: stop_reason.map(|s| s.to_string()),
                usage: Some(Usage {
                    input_tokens: Some(input),
                    output_tokens: Some(output),
                    cache_creation_input_tokens: None,
                    cache_read_input_tokens: None,
                }),
            }),
        }
    }

    #[test]
    fn test_parse_entry_valid() {
        let entry = make_usage_entry(
            "2025-01-15T10:00:00Z",
            Some("claude-3-5-sonnet-20241022"),
            Some("end_turn"),
            100,
            50,
        );
        let tz = make_timezone();
        let result = parse_entry(entry, Path::new("test.jsonl"), "sess1", "proj1", tz, 1);
        let raw = result.unwrap();
        assert_eq!(raw.input_tokens, 100);
        assert_eq!(raw.output_tokens, 50);
        assert_eq!(raw.model, "3-5-sonnet");
        assert_eq!(raw.session_id, "sess1");
        assert_eq!(raw.project_path, "proj1");
    }

    #[test]
    fn test_parse_entry_no_timestamp_returns_none() {
        let entry = UsageEntry {
            timestamp: None,
            message: Some(Message {
                id: Some("msg_001".to_string()),
                model: Some("claude-3-5-sonnet-20241022".to_string()),
                stop_reason: None,
                usage: Some(Usage::default()),
            }),
        };
        let tz = make_timezone();
        assert!(parse_entry(entry, Path::new("t.jsonl"), "s", "p", tz, 1).is_none());
    }

    #[test]
    fn test_parse_entry_no_message_returns_none() {
        let entry = UsageEntry {
            timestamp: Some("2025-01-15T10:00:00Z".to_string()),
            message: None,
        };
        let tz = make_timezone();
        assert!(parse_entry(entry, Path::new("t.jsonl"), "s", "p", tz, 1).is_none());
    }

    #[test]
    fn test_parse_entry_no_usage_returns_none() {
        let entry = UsageEntry {
            timestamp: Some("2025-01-15T10:00:00Z".to_string()),
            message: Some(Message {
                id: Some("msg_001".to_string()),
                model: Some("claude-3-5-sonnet-20241022".to_string()),
                stop_reason: None,
                usage: None,
            }),
        };
        let tz = make_timezone();
        assert!(parse_entry(entry, Path::new("t.jsonl"), "s", "p", tz, 1).is_none());
    }

    #[test]
    fn test_parse_entry_synthetic_model_filtered() {
        let entry = make_usage_entry("2025-01-15T10:00:00Z", Some("<synthetic>"), None, 10, 5);
        let tz = make_timezone();
        assert!(parse_entry(entry, Path::new("t.jsonl"), "s", "p", tz, 1).is_none());
    }

    #[test]
    fn test_parse_entry_empty_model_filtered() {
        let entry = make_usage_entry("2025-01-15T10:00:00Z", Some(""), None, 10, 5);
        let tz = make_timezone();
        assert!(parse_entry(entry, Path::new("t.jsonl"), "s", "p", tz, 1).is_none());
    }

    #[test]
    fn test_parse_entry_no_model_uses_unknown() {
        let entry = make_usage_entry("2025-01-15T10:00:00Z", None, None, 10, 5);
        let tz = make_timezone();
        let raw = parse_entry(entry, Path::new("t.jsonl"), "s", "p", tz, 1).unwrap();
        assert_eq!(raw.model, UNKNOWN);
    }

    #[test]
    fn test_parse_entry_invalid_timestamp_returns_none() {
        let entry = make_usage_entry("not-a-date", Some("claude-3-5-sonnet-20241022"), None, 10, 5);
        let tz = make_timezone();
        assert!(parse_entry(entry, Path::new("t.jsonl"), "s", "p", tz, 1).is_none());
    }

    #[test]
    fn test_parse_entry_cache_tokens() {
        let entry = UsageEntry {
            timestamp: Some("2025-01-15T10:00:00Z".to_string()),
            message: Some(Message {
                id: Some("msg_002".to_string()),
                model: Some("claude-3-5-sonnet-20241022".to_string()),
                stop_reason: Some("end_turn".to_string()),
                usage: Some(Usage {
                    input_tokens: Some(100),
                    output_tokens: Some(50),
                    cache_creation_input_tokens: Some(30),
                    cache_read_input_tokens: Some(20),
                }),
            }),
        };
        let tz = make_timezone();
        let raw = parse_entry(entry, Path::new("t.jsonl"), "s", "p", tz, 1).unwrap();
        assert_eq!(raw.cache_creation, 30);
        assert_eq!(raw.cache_read, 20);
    }

    #[test]
    fn test_parse_entry_none_tokens_default_to_zero() {
        let entry = UsageEntry {
            timestamp: Some("2025-01-15T10:00:00Z".to_string()),
            message: Some(Message {
                id: Some("msg_003".to_string()),
                model: Some("claude-3-5-sonnet-20241022".to_string()),
                stop_reason: None,
                usage: Some(Usage {
                    input_tokens: None,
                    output_tokens: None,
                    cache_creation_input_tokens: None,
                    cache_read_input_tokens: None,
                }),
            }),
        };
        let tz = make_timezone();
        let raw = parse_entry(entry, Path::new("t.jsonl"), "s", "p", tz, 1).unwrap();
        assert_eq!(raw.input_tokens, 0);
        assert_eq!(raw.output_tokens, 0);
        assert_eq!(raw.cache_creation, 0);
        assert_eq!(raw.cache_read, 0);
    }
}
