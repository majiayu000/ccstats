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
use crate::source::ParseOutput;
use crate::utils::Timezone;

// ============================================================================
// Internal types for JSONL parsing
// ============================================================================

#[derive(Debug, Deserialize)]
struct UsageEntry {
    #[serde(default, rename = "isSidechain")]
    is_sidechain: bool,
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
#[allow(clippy::struct_field_names)] // field names match JSON schema
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

fn estimate_entry_capacity(file: &File, approx_bytes_per_entry: u64) -> usize {
    let estimate = file
        .metadata()
        .ok()
        .map(|meta| meta.len() / approx_bytes_per_entry)
        .and_then(|n| usize::try_from(n).ok())
        .unwrap_or(0);
    estimate.saturating_add(1)
}

/// Normalize model name by removing prefixes and date suffixes
fn normalize_model_name(model: &str) -> String {
    let mut name = model;
    if let Some(stripped) = name.strip_prefix("anthropic.") {
        name = stripped;
    }
    if let Some(stripped) = name.strip_prefix("claude-") {
        name = stripped;
    }

    // Remove date suffix like -20251101
    if let Some(pos) = name.rfind('-') {
        let suffix = &name[pos + 1..];
        if suffix.len() == 8 && suffix.chars().all(|c| c.is_ascii_digit()) {
            return name[..pos].to_string();
        }
    }

    name.to_string()
}

fn derive_project_path(path: &Path) -> String {
    let mut components = path.parent().into_iter().flat_map(Path::components);
    while let Some(component) = components.next() {
        if component.as_os_str() == "projects" {
            if let Some(project) = components.next() {
                return project.as_os_str().to_string_lossy().into_owned();
            }
            break;
        }
    }

    path.parent()
        .and_then(|p| p.file_name())
        .and_then(|s| s.to_str())
        .unwrap_or(UNKNOWN)
        .to_string()
}

pub(super) fn parse_claude_file_with_debug(
    path: &Path,
    timezone: Timezone,
    debug: bool,
) -> ParseOutput {
    let session_key = path.display().to_string();
    let session_id = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(UNKNOWN)
        .to_string();

    let project_path = derive_project_path(path);

    let file = match File::open(path) {
        Ok(f) => f,
        Err(err) => {
            if debug {
                eprintln!("Failed to open {}: {}", path.display(), err);
            }
            return ParseOutput {
                entries: Vec::new(),
                errors: 1,
            };
        }
    };
    let estimated_capacity = estimate_entry_capacity(&file, 220);
    let reader = BufReader::new(file);

    let mut entries = Vec::with_capacity(estimated_capacity);
    let mut parse_errors = 0usize;
    let mut line = String::new();
    let mut line_no = 0usize;
    let mut reader = reader;
    loop {
        line.clear();
        let bytes_read = match reader.read_line(&mut line) {
            Ok(n) => n,
            Err(err) => {
                line_no += 1;
                if debug {
                    eprintln!(
                        "Failed to read line {} in {}: {}",
                        line_no,
                        path.display(),
                        err
                    );
                }
                parse_errors += 1;
                continue;
            }
        };
        if bytes_read == 0 {
            break;
        }
        line_no += 1;

        let line = line.trim_end_matches(['\n', '\r']);
        if line.is_empty() {
            continue;
        }

        let entry: UsageEntry = match serde_json::from_str(line) {
            Ok(entry) => entry,
            Err(err) => {
                if debug {
                    eprintln!("Invalid JSON at {}:{}: {}", path.display(), line_no, err);
                }
                parse_errors += 1;
                continue;
            }
        };

        if let Some(entry) = parse_entry_with_debug(
            entry,
            path,
            &session_key,
            &session_id,
            &project_path,
            timezone,
            line_no,
            debug,
            &mut parse_errors,
        ) {
            entries.push(entry);
        }
    }
    ParseOutput {
        entries,
        errors: parse_errors,
    }
}

#[cfg(test)]
fn parse_entry(
    entry: UsageEntry,
    path: &Path,
    session_key: &str,
    session_id: &str,
    project_path: &str,
    timezone: Timezone,
    line_no: usize,
) -> Option<RawEntry> {
    let mut parse_errors = 0usize;
    parse_entry_with_debug(
        entry,
        path,
        session_key,
        session_id,
        project_path,
        timezone,
        line_no,
        false,
        &mut parse_errors,
    )
}

#[allow(clippy::too_many_arguments)] // Keeps call site explicit for parse context + diagnostics.
fn parse_entry_with_debug(
    entry: UsageEntry,
    path: &Path,
    session_key: &str,
    session_id: &str,
    project_path: &str,
    timezone: Timezone,
    line_no: usize,
    debug: bool,
    parse_errors: &mut usize,
) -> Option<RawEntry> {
    if entry.is_sidechain {
        return None;
    }

    let ts = entry.timestamp?;
    let msg = entry.message?;
    let usage = msg.usage?;

    let model = msg
        .model
        .as_deref()
        .map_or_else(|| UNKNOWN.to_string(), normalize_model_name);

    if model == "<synthetic>" || model.is_empty() {
        return None;
    }

    // Parse timestamp
    let utc_dt = match ts.parse::<DateTime<Utc>>() {
        Ok(dt) => dt,
        Err(err) => {
            if debug {
                eprintln!(
                    "Invalid timestamp at {}:{}: {} ({})",
                    path.display(),
                    line_no,
                    ts,
                    err
                );
            }
            *parse_errors += 1;
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
        session_key: session_key.to_string(),
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
        assert_eq!(normalize_model_name("anthropic.some-model"), "some-model");
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
            is_sidechain: false,
            timestamp: Some(timestamp.to_string()),
            message: Some(Message {
                id: Some("msg_001".to_string()),
                model: model.map(ToString::to_string),
                stop_reason: stop_reason.map(ToString::to_string),
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
        let result = parse_entry(
            entry,
            Path::new("test.jsonl"),
            "scope/test",
            "sess1",
            "proj1",
            tz,
            1,
        );
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
            is_sidechain: false,
            timestamp: None,
            message: Some(Message {
                id: Some("msg_001".to_string()),
                model: Some("claude-3-5-sonnet-20241022".to_string()),
                stop_reason: None,
                usage: Some(Usage::default()),
            }),
        };
        let tz = make_timezone();
        assert!(parse_entry(entry, Path::new("t.jsonl"), "scope/t", "s", "p", tz, 1).is_none());
    }

    #[test]
    fn test_parse_entry_no_message_returns_none() {
        let entry = UsageEntry {
            is_sidechain: false,
            timestamp: Some("2025-01-15T10:00:00Z".to_string()),
            message: None,
        };
        let tz = make_timezone();
        assert!(parse_entry(entry, Path::new("t.jsonl"), "scope/t", "s", "p", tz, 1).is_none());
    }

    #[test]
    fn test_parse_entry_no_usage_returns_none() {
        let entry = UsageEntry {
            is_sidechain: false,
            timestamp: Some("2025-01-15T10:00:00Z".to_string()),
            message: Some(Message {
                id: Some("msg_001".to_string()),
                model: Some("claude-3-5-sonnet-20241022".to_string()),
                stop_reason: None,
                usage: None,
            }),
        };
        let tz = make_timezone();
        assert!(parse_entry(entry, Path::new("t.jsonl"), "scope/t", "s", "p", tz, 1).is_none());
    }

    #[test]
    fn test_parse_entry_synthetic_model_filtered() {
        let entry = make_usage_entry("2025-01-15T10:00:00Z", Some("<synthetic>"), None, 10, 5);
        let tz = make_timezone();
        assert!(parse_entry(entry, Path::new("t.jsonl"), "scope/t", "s", "p", tz, 1).is_none());
    }

    #[test]
    fn test_parse_entry_empty_model_filtered() {
        let entry = make_usage_entry("2025-01-15T10:00:00Z", Some(""), None, 10, 5);
        let tz = make_timezone();
        assert!(parse_entry(entry, Path::new("t.jsonl"), "scope/t", "s", "p", tz, 1).is_none());
    }

    #[test]
    fn test_parse_entry_no_model_uses_unknown() {
        let entry = make_usage_entry("2025-01-15T10:00:00Z", None, None, 10, 5);
        let tz = make_timezone();
        let raw = parse_entry(entry, Path::new("t.jsonl"), "scope/t", "s", "p", tz, 1).unwrap();
        assert_eq!(raw.model, UNKNOWN);
    }

    #[test]
    fn test_parse_entry_invalid_timestamp_returns_none() {
        let entry = make_usage_entry(
            "not-a-date",
            Some("claude-3-5-sonnet-20241022"),
            None,
            10,
            5,
        );
        let tz = make_timezone();
        assert!(parse_entry(entry, Path::new("t.jsonl"), "scope/t", "s", "p", tz, 1).is_none());
    }

    #[test]
    fn test_parse_entry_cache_tokens() {
        let entry = UsageEntry {
            is_sidechain: false,
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
        let raw = parse_entry(entry, Path::new("t.jsonl"), "scope/t", "s", "p", tz, 1).unwrap();
        assert_eq!(raw.cache_creation, 30);
        assert_eq!(raw.cache_read, 20);
    }

    #[test]
    fn test_parse_entry_none_tokens_default_to_zero() {
        let entry = UsageEntry {
            is_sidechain: false,
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
        let raw = parse_entry(entry, Path::new("t.jsonl"), "scope/t", "s", "p", tz, 1).unwrap();
        assert_eq!(raw.input_tokens, 0);
        assert_eq!(raw.output_tokens, 0);
        assert_eq!(raw.cache_creation, 0);
        assert_eq!(raw.cache_read, 0);
    }
}
