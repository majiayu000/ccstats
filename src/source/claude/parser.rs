//! Claude Code JSONL parser
//!
//! Parses JSONL logs from ~/.claude/projects/ directory.

use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;

use crate::core::{DateFilter, RawEntry};
use crate::utils::{parse_debug_enabled, Timezone};

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

pub(super) fn parse_claude_file(
    path: &PathBuf,
    _filter: &DateFilter,
    timezone: &Timezone,
) -> Vec<RawEntry> {
    let session_id = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();

    let project_path = path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
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

        if let Some(entry) =
            parse_entry(entry, path, &session_id, &project_path, timezone, line_no + 1)
        {
            entries.push(entry);
        }
    }
    entries
}

fn parse_entry(
    entry: UsageEntry,
    path: &PathBuf,
    session_id: &str,
    project_path: &str,
    timezone: &Timezone,
    line_no: usize,
) -> Option<RawEntry> {
    let ts = entry.timestamp?;
    let msg = entry.message?;
    let usage = msg.usage?;

    let model = msg
        .model
        .as_deref()
        .map(normalize_model_name)
        .unwrap_or_else(|| "unknown".to_string());

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
        date_str: date.format("%Y-%m-%d").to_string(),
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

    #[test]
    fn test_normalize_model_name() {
        assert_eq!(
            normalize_model_name("anthropic.claude-3-5-sonnet-20241022"),
            "3-5-sonnet"
        );
        assert_eq!(normalize_model_name("claude-3-opus-20240229"), "3-opus");
        assert_eq!(normalize_model_name("gpt-4"), "gpt-4");
    }
}
