//! Qwen Code usage-ledger source.

use std::env;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::consts::DATE_FORMAT;
use crate::core::{CostKind, Endpoint, RawEntry};
use crate::source::{Capabilities, ParseOutput, Source};
use crate::utils::Timezone;

const CURRENT_SCHEMA_VERSION: u32 = 1;
const DEFAULT_QWEN_DIR: &str = ".qwen";
const QWEN_HOME_ENV: &str = "QWEN_HOME";
const QWEN_RUNTIME_DIR_ENV: &str = "QWEN_RUNTIME_DIR";

pub(crate) struct QwenSource;

impl QwenSource {
    pub(crate) const fn new() -> Self {
        Self
    }
}

impl Source for QwenSource {
    fn name(&self) -> &'static str {
        "qwen"
    }

    fn display_name(&self) -> &'static str {
        "Qwen Code"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["qw"]
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            has_projects: false,
            has_billing_blocks: false,
            has_reasoning_tokens: true,
            has_cache_creation: false,
            has_cache_read: true,
            needs_dedup: false,
            has_tool_calls: false,
            has_endpoints: false,
        }
    }

    fn find_files(&self) -> Vec<PathBuf> {
        find_qwen_files()
    }

    fn parse_file(&self, path: &Path, timezone: Timezone, debug: bool) -> ParseOutput {
        parse_qwen_file(path, timezone, debug)
    }
}

fn configured_root(variable: &str) -> Option<PathBuf> {
    env::var_os(variable)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn qwen_root() -> Option<PathBuf> {
    configured_root(QWEN_RUNTIME_DIR_ENV)
        .or_else(|| configured_root(QWEN_HOME_ENV))
        .or_else(|| dirs::home_dir().map(|home| home.join(DEFAULT_QWEN_DIR)))
}

fn find_qwen_files_in_root(root: &Path) -> Vec<PathBuf> {
    let pattern = root.join("usage/token-usage-*.jsonl");
    let mut files = glob::glob(&pattern.to_string_lossy())
        .into_iter()
        .flatten()
        .flatten()
        .filter(|path| path.is_file())
        .collect::<Vec<_>>();
    files.sort();
    files.dedup();
    files
}

fn find_qwen_files() -> Vec<PathBuf> {
    qwen_root()
        .map(|root| find_qwen_files_in_root(&root))
        .unwrap_or_default()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct QwenUsageRecord {
    schema_version: u32,
    id: String,
    timestamp: String,
    session_id: String,
    model: String,
    input_tokens: i64,
    output_tokens: i64,
    cached_tokens: i64,
    thoughts_tokens: i64,
    total_tokens: i64,
}

fn entry_from_record(
    record: &QwenUsageRecord,
    path: &Path,
    timezone: Timezone,
) -> Result<Option<RawEntry>, &'static str> {
    if record.schema_version != CURRENT_SCHEMA_VERSION {
        return Err("unsupported schemaVersion");
    }
    if record.id.trim().is_empty()
        || record.session_id.trim().is_empty()
        || record.model.trim().is_empty()
    {
        return Err("missing required identity field");
    }
    if record.input_tokens < 0
        || record.output_tokens < 0
        || record.cached_tokens < 0
        || record.thoughts_tokens < 0
        || record.total_tokens < 0
    {
        return Err("negative token count");
    }
    if record.cached_tokens > record.input_tokens {
        return Err("cachedTokens exceeds inputTokens");
    }

    let input_tokens = record.input_tokens - record.cached_tokens;
    if input_tokens == 0
        && record.output_tokens == 0
        && record.cached_tokens == 0
        && record.thoughts_tokens == 0
    {
        return if record.total_tokens == 0 {
            Ok(None)
        } else {
            Err("totalTokens has no component token counts")
        };
    }

    let timestamp = DateTime::parse_from_rfc3339(&record.timestamp)
        .map_err(|_| "invalid timestamp")?
        .with_timezone(&Utc);
    let timestamp_ms = timestamp.timestamp_millis();
    let session_id = record.session_id.trim().to_string();

    Ok(Some(RawEntry {
        timestamp: timestamp.to_rfc3339(),
        timestamp_ms,
        date_str: timezone
            .to_fixed_offset(timestamp)
            .date_naive()
            .format(DATE_FORMAT)
            .to_string(),
        message_id: None,
        session_key: format!("{}::{session_id}", path.display()),
        session_id,
        project_path: String::new(),
        model: record.model.trim().to_string(),
        input_tokens,
        output_tokens: record.output_tokens,
        cache_creation: 0,
        cache_creation_1h: 0,
        cache_read: record.cached_tokens,
        reasoning_tokens: record.thoughts_tokens,
        stop_reason: None,
        cost_kind: CostKind::Real,
        endpoint: Endpoint::Unknown,
        call_count: 1,
        reported_total_tokens: None,
        recorded_cost_usd: None,
        api_equivalent_priced_tokens: 0,
        api_equivalent_coverage_tokens: 0,
    }))
}

fn parse_qwen_file(path: &Path, timezone: Timezone, debug: bool) -> ParseOutput {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) => {
            if debug {
                eprintln!("Failed to open {}: {error}", path.display());
            }
            return ParseOutput {
                entries: Vec::new(),
                errors: 1,
            };
        }
    };
    let mut output = ParseOutput::default();

    for (line_index, line) in BufReader::new(file).lines().enumerate() {
        let line = match line {
            Ok(line) => line,
            Err(error) => {
                output.errors += 1;
                if debug {
                    eprintln!(
                        "Failed to read {} line {}: {error}",
                        path.display(),
                        line_index + 1
                    );
                }
                continue;
            }
        };
        if line.trim().is_empty() {
            continue;
        }
        let record = match serde_json::from_str(&line) {
            Ok(record) => record,
            Err(error) => {
                output.errors += 1;
                if debug {
                    eprintln!(
                        "Invalid JSON in {} line {}: {error}",
                        path.display(),
                        line_index + 1
                    );
                }
                continue;
            }
        };

        match entry_from_record(&record, path, timezone) {
            Ok(Some(entry)) => output.entries.push(entry),
            Ok(None) => {}
            Err(error) => {
                output.errors += 1;
                if debug {
                    eprintln!(
                        "Invalid Qwen usage in {} line {}: {error}",
                        path.display(),
                        line_index + 1
                    );
                }
            }
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    use tempfile::tempdir;

    #[test]
    fn usage_ledger_splits_cached_input_and_reasoning() {
        let temp = tempdir().unwrap();
        let usage_dir = temp.path().join("usage");
        fs::create_dir_all(&usage_dir).unwrap();
        let path = usage_dir.join("token-usage-2026-08.jsonl");
        fs::write(
            &path,
            r#"{"schemaVersion":1,"id":"usage-1","timestamp":"2026-08-31T03:04:05Z","localDate":"2026-08-31","localMonth":"2026-08","sessionId":"qwen-session","model":"qwen3-coder-plus","authType":"qwen-oauth","source":"main","inputTokens":100,"outputTokens":20,"cachedTokens":30,"thoughtsTokens":10,"totalTokens":130,"apiDurationMs":50}"#,
        )
        .unwrap();

        let parsed = parse_qwen_file(&path, Timezone::Named(chrono_tz::UTC), false);

        assert_eq!(parsed.errors, 0);
        assert_eq!(parsed.entries.len(), 1);
        let entry = &parsed.entries[0];
        assert_eq!(entry.session_id, "qwen-session");
        assert_eq!(entry.project_path, "");
        assert_eq!(entry.model, "qwen3-coder-plus");
        assert_eq!(entry.input_tokens, 70);
        assert_eq!(entry.output_tokens, 20);
        assert_eq!(entry.reasoning_tokens, 10);
        assert_eq!(entry.cache_read, 30);
    }

    #[test]
    fn discovery_only_includes_usage_ledgers() {
        let temp = tempdir().unwrap();
        let usage_dir = temp.path().join("usage");
        let chats_dir = temp.path().join("projects/project/chats");
        fs::create_dir_all(&usage_dir).unwrap();
        fs::create_dir_all(&chats_dir).unwrap();
        let ledger = usage_dir.join("token-usage-2026-08.jsonl");
        fs::write(&ledger, "").unwrap();
        fs::write(chats_dir.join("session.jsonl"), "").unwrap();

        assert_eq!(find_qwen_files_in_root(temp.path()), vec![ledger]);
    }

    #[test]
    fn future_schema_is_reported_as_an_error() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("token-usage-2026-08.jsonl");
        fs::write(
            &path,
            r#"{"schemaVersion":2,"id":"usage-1","timestamp":"2026-08-31T03:04:05Z","sessionId":"qwen-session","model":"qwen3-coder-plus","inputTokens":10,"outputTokens":2,"cachedTokens":0,"thoughtsTokens":0,"totalTokens":12}"#,
        )
        .unwrap();

        let parsed = parse_qwen_file(&path, Timezone::Named(chrono_tz::UTC), false);

        assert_eq!(parsed.errors, 1);
        assert!(parsed.entries.is_empty());
    }
}
