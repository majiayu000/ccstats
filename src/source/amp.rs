//! Amp local thread usage source.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::consts::{DATE_FORMAT, UNKNOWN};
use crate::core::{CostKind, Endpoint, RawEntry};
use crate::source::{Capabilities, ParseOutput, Source};
use crate::utils::Timezone;

const XDG_DATA_HOME_ENV: &str = "XDG_DATA_HOME";

pub(crate) struct AmpSource;

impl AmpSource {
    pub(crate) const fn new() -> Self {
        Self
    }
}

impl Source for AmpSource {
    fn name(&self) -> &'static str {
        "amp"
    }

    fn display_name(&self) -> &'static str {
        "Amp"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["ampcode"]
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            has_projects: false,
            has_billing_blocks: false,
            has_reasoning_tokens: false,
            has_cache_creation: true,
            has_cache_read: true,
            needs_dedup: false,
            has_tool_calls: false,
            has_endpoints: false,
        }
    }

    fn find_files(&self) -> Vec<PathBuf> {
        find_amp_files()
    }

    fn parse_file(&self, path: &Path, timezone: Timezone, debug: bool) -> ParseOutput {
        parse_amp_file(path, timezone, debug)
    }
}

fn amp_threads_dir() -> Option<PathBuf> {
    let data_home = match env::var(XDG_DATA_HOME_ENV) {
        Ok(value) if !value.trim().is_empty() => PathBuf::from(value),
        Ok(_) | Err(_) => dirs::home_dir()?.join(".local/share"),
    };
    Some(data_home.join("amp/threads"))
}

fn find_amp_files() -> Vec<PathBuf> {
    let Some(root) = amp_threads_dir() else {
        return Vec::new();
    };
    let pattern = root.join("**").join("T-*.json");
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

fn file_modified_ms(path: &Path) -> Option<i64> {
    let millis = fs::metadata(path)
        .ok()?
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_millis();
    i64::try_from(millis).ok()
}

fn positive(value: Option<i64>) -> i64 {
    value.unwrap_or(0).max(0)
}

#[derive(Debug, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct AmpTokens {
    input: Option<i64>,
    output: Option<i64>,
    cache_read_input_tokens: Option<i64>,
    cache_creation_input_tokens: Option<i64>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct LedgerEvent {
    timestamp: Option<String>,
    model: Option<String>,
    tokens: Option<AmpTokens>,
    to_message_id: Option<i64>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct UsageLedger {
    events: Vec<LedgerEvent>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct MessageUsage {
    model: Option<String>,
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    cache_read_input_tokens: Option<i64>,
    cache_creation_input_tokens: Option<i64>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct AmpMessage {
    role: Option<String>,
    message_id: Option<i64>,
    usage: Option<MessageUsage>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct AmpThread {
    id: Option<String>,
    created: Option<i64>,
    messages: Vec<AmpMessage>,
    usage_ledger: Option<UsageLedger>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct TokenBuckets {
    input: i64,
    output: i64,
    cache_read: i64,
    cache_creation: i64,
}

impl TokenBuckets {
    fn is_empty(&self) -> bool {
        self.input == 0 && self.output == 0 && self.cache_read == 0 && self.cache_creation == 0
    }
}

#[derive(Debug, Clone)]
struct AmpRecord {
    model: String,
    timestamp_ms: i64,
    message_id: Option<i64>,
    ledger_to_message_id: Option<i64>,
    tokens: TokenBuckets,
}

fn non_empty(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn parse_rfc3339_ms(value: Option<&str>) -> Option<i64> {
    value.and_then(|value| {
        DateTime::parse_from_rfc3339(value)
            .ok()
            .map(|timestamp| timestamp.timestamp_millis())
    })
}

fn valid_base_timestamp(thread_created: Option<i64>, file_mtime: Option<i64>) -> Option<i64> {
    thread_created.filter(|value| *value > 0).or(file_mtime)
}

fn ledger_records(thread: &mut AmpThread, fallback: Option<i64>) -> (Vec<AmpRecord>, usize) {
    let Some(ledger) = thread.usage_ledger.take() else {
        return (Vec::new(), 0);
    };
    let mut errors = 0;
    let records = ledger
        .events
        .into_iter()
        .filter_map(|event| {
            let model = non_empty(event.model)?;
            let explicit_timestamp = parse_rfc3339_ms(event.timestamp.as_deref());
            let timestamp_ms = explicit_timestamp.or(fallback);
            let Some(timestamp_ms) = timestamp_ms else {
                errors += 1;
                return None;
            };
            let tokens = event.tokens.unwrap_or_default();
            let tokens = TokenBuckets {
                input: positive(tokens.input),
                output: positive(tokens.output),
                cache_read: positive(tokens.cache_read_input_tokens),
                cache_creation: positive(tokens.cache_creation_input_tokens),
            };
            if tokens.is_empty() {
                return None;
            }
            Some(AmpRecord {
                model,
                timestamp_ms,
                message_id: None,
                ledger_to_message_id: event.to_message_id.filter(|id| *id > 0),
                tokens,
            })
        })
        .collect();
    (records, errors)
}

fn message_records(thread: AmpThread) -> Vec<AmpRecord> {
    thread
        .messages
        .into_iter()
        .filter(|message| message.role.as_deref() == Some("assistant"))
        .filter_map(|message| {
            let usage = message.usage?;
            let model = non_empty(usage.model)?;
            let message_id = message.message_id.filter(|id| *id > 0);
            let tokens = TokenBuckets {
                input: positive(usage.input_tokens),
                output: positive(usage.output_tokens),
                cache_read: positive(usage.cache_read_input_tokens),
                cache_creation: positive(usage.cache_creation_input_tokens),
            };
            if tokens.is_empty() {
                return None;
            }
            Some(AmpRecord {
                model,
                timestamp_ms: 0,
                message_id,
                ledger_to_message_id: None,
                tokens,
            })
        })
        .collect()
}

fn matching_ledger(
    ledger: &[AmpRecord],
    consumed: &[bool],
    search_start: usize,
    message: &AmpRecord,
) -> Option<usize> {
    let find = |predicate: &dyn Fn(usize) -> bool| {
        (search_start..ledger.len())
            .find(|index| predicate(*index))
            .or_else(|| (0..search_start).find(|index| predicate(*index)))
    };
    if let Some(message_id) = message.message_id
        && let Some(index) = find(&|index| {
            !consumed[index] && ledger[index].ledger_to_message_id == Some(message_id)
        })
    {
        return Some(index);
    }
    find(&|index| {
        !consumed[index]
            && ledger[index].model == message.model
            && ledger[index].tokens == message.tokens
    })
}

fn merge_records(ledger: AmpRecord, message: &AmpRecord) -> AmpRecord {
    AmpRecord {
        message_id: message.message_id,
        ..ledger
    }
}

fn merge_usage_records(
    mut ledger: Vec<AmpRecord>,
    messages: Vec<AmpRecord>,
) -> (Vec<AmpRecord>, usize) {
    if ledger.is_empty() {
        return (Vec::new(), messages.len());
    }

    let mut consumed = vec![false; ledger.len()];
    let mut search_start = 0;
    let mut unmatched = 0usize;
    for message in messages {
        if let Some(index) = matching_ledger(&ledger, &consumed, search_start, &message) {
            consumed[index] = true;
            search_start = index.saturating_add(1);
            ledger[index] = merge_records(ledger[index].clone(), &message);
        } else {
            unmatched += 1;
        }
    }
    ledger.sort_by_key(|record| record.timestamp_ms);
    (ledger, unmatched)
}

fn into_raw_entry(
    path: &Path,
    timezone: Timezone,
    session_id: &str,
    record: AmpRecord,
) -> Option<RawEntry> {
    let utc = DateTime::<Utc>::from_timestamp_millis(record.timestamp_ms)?;
    Some(RawEntry {
        timestamp: utc.to_rfc3339(),
        timestamp_ms: record.timestamp_ms,
        date_str: timezone
            .to_fixed_offset(utc)
            .date_naive()
            .format(DATE_FORMAT)
            .to_string(),
        message_id: record.message_id.map(|id| format!("amp:{session_id}:{id}")),
        session_key: format!("{}::{session_id}", path.display()),
        session_id: session_id.to_string(),
        project_path: String::new(),
        model: record.model,
        input_tokens: record.tokens.input,
        output_tokens: record.tokens.output,
        cache_creation: record.tokens.cache_creation,
        cache_creation_1h: 0,
        cache_read: record.tokens.cache_read,
        reasoning_tokens: 0,
        stop_reason: None,
        cost_kind: CostKind::Real,
        endpoint: Endpoint::Unknown,
        call_count: 1,
        recorded_cost_usd: None,
    })
}

fn parse_amp_file(path: &Path, timezone: Timezone, debug: bool) -> ParseOutput {
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) => {
            if debug {
                eprintln!("Failed to read {}: {error}", path.display());
            }
            return ParseOutput {
                entries: Vec::new(),
                errors: 1,
            };
        }
    };
    let mut thread: AmpThread = match serde_json::from_str(&content) {
        Ok(thread) => thread,
        Err(error) => {
            if debug {
                eprintln!("Invalid JSON in {}: {error}", path.display());
            }
            return ParseOutput {
                entries: Vec::new(),
                errors: 1,
            };
        }
    };
    let session_id = non_empty(thread.id.clone()).unwrap_or_else(|| {
        path.file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or(UNKNOWN)
            .to_string()
    });
    let fallback = valid_base_timestamp(thread.created, file_modified_ms(path));
    let (ledger, ledger_errors) = ledger_records(&mut thread, fallback);
    let messages = message_records(thread);
    let (records, message_errors) = merge_usage_records(ledger, messages);
    let mut invalid_timestamps = 0;
    let entries = records
        .into_iter()
        .filter_map(|record| {
            let entry = into_raw_entry(path, timezone, &session_id, record);
            if entry.is_none() {
                invalid_timestamps += 1;
            }
            entry
        })
        .collect();
    ParseOutput {
        entries,
        errors: ledger_errors + message_errors + invalid_timestamps,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use tempfile::tempdir;

    #[test]
    fn ledger_and_message_usage_for_same_call_are_counted_once() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("T-thread.json");
        fs::write(
            &path,
            r#"{
                "id": "thread-1",
                "created": 1788145445000,
                "usageLedger": {"events": [{
                    "timestamp": "2026-08-31T03:04:05Z",
                    "model": "claude-sonnet-4",
                    "toMessageId": 7,
                    "tokens": {
                        "input": 100,
                        "output": 20,
                        "cacheReadInputTokens": 30,
                        "cacheCreationInputTokens": 5
                    }
                }]},
                "messages": [{
                    "role": "assistant",
                    "messageId": 7,
                    "usage": {
                        "model": "claude-sonnet-4",
                        "inputTokens": 100,
                        "outputTokens": 20,
                        "cacheReadInputTokens": 30,
                        "cacheCreationInputTokens": 5
                    }
                }]
            }"#,
        )
        .unwrap();

        let parsed = parse_amp_file(&path, Timezone::Named(chrono_tz::UTC), false);

        assert_eq!(parsed.errors, 0);
        assert_eq!(parsed.entries.len(), 1);
        let entry = &parsed.entries[0];
        assert_eq!(entry.session_id, "thread-1");
        assert_eq!(entry.input_tokens, 100);
        assert_eq!(entry.output_tokens, 20);
        assert_eq!(entry.cache_read, 30);
        assert_eq!(entry.cache_creation, 5);
    }

    #[test]
    fn message_usage_without_a_real_timestamp_is_reported_and_skipped() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("T-thread.json");
        fs::write(
            &path,
            r#"{
                "id": "thread-1",
                "created": 1788145445000,
                "messages": [{
                    "role": "assistant",
                    "messageId": 86400,
                    "usage": {"model": "claude-sonnet-4", "inputTokens": 10}
                }]
            }"#,
        )
        .unwrap();

        let parsed = parse_amp_file(&path, Timezone::Named(chrono_tz::UTC), false);

        assert!(parsed.entries.is_empty());
        assert_eq!(parsed.errors, 1);
    }
}
