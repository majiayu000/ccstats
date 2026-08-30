//! GitHub Copilot CLI OpenTelemetry JSONL usage source.

use std::env;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde_json::{Map, Value};

use crate::consts::{DATE_FORMAT, UNKNOWN};
use crate::core::{CostKind, Endpoint, RawEntry, source_wide_message_id};
use crate::source::{Capabilities, ParseOutput, Source};
use crate::utils::Timezone;

const COPILOT_OTEL_PATH_ENV: &str = "COPILOT_OTEL_FILE_EXPORTER_PATH";
const USAGE_KEYS: [&str; 5] = [
    "gen_ai.usage.input_tokens",
    "gen_ai.usage.output_tokens",
    "gen_ai.usage.cache_read.input_tokens",
    "gen_ai.usage.cache_creation.input_tokens",
    "gen_ai.usage.reasoning.output_tokens",
];

pub(crate) struct CopilotSource;

impl CopilotSource {
    pub(crate) const fn new() -> Self {
        Self
    }
}

impl Source for CopilotSource {
    fn name(&self) -> &'static str {
        "copilot"
    }

    fn display_name(&self) -> &'static str {
        "GitHub Copilot CLI"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["github-copilot"]
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            has_projects: false,
            has_billing_blocks: false,
            has_reasoning_tokens: true,
            has_cache_creation: true,
            has_cache_read: true,
            needs_dedup: true,
            has_tool_calls: false,
            has_endpoints: false,
        }
    }

    fn find_files(&self) -> Vec<PathBuf> {
        find_copilot_files()
    }

    fn parse_file(&self, path: &Path, timezone: Timezone, debug: bool) -> ParseOutput {
        parse_copilot_file(path, timezone, debug)
    }
}

fn find_copilot_files() -> Vec<PathBuf> {
    let mut files = Vec::new();
    if let Some(path) = env::var_os(COPILOT_OTEL_PATH_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_file())
    {
        files.push(path);
    }

    if let Some(home) = dirs::home_dir() {
        let pattern = home.join(".copilot/otel/**/*.jsonl");
        if let Ok(matches) = glob::glob(&pattern.to_string_lossy()) {
            files.extend(matches.flatten().filter(|path| path.is_file()));
        }
    }

    files = files
        .into_iter()
        .map(|path| fs::canonicalize(&path).unwrap_or(path))
        .collect();
    files.sort();
    files.dedup();
    files
}

fn strict_token(attributes: &Map<String, Value>, key: &str) -> Result<i64, &'static str> {
    let Some(value) = attributes.get(key) else {
        return Ok(0);
    };
    let value = value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|value| i64::try_from(value).ok()))
        .ok_or("token attribute is not an integer")?;
    if value < 0 {
        return Err("negative token count");
    }
    Ok(value)
}

fn non_empty_attr<'a>(attributes: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    attributes
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn span_timestamp_ms(value: &Value) -> Result<i64, &'static str> {
    let parts = value.as_array().ok_or("span time is not a tuple")?;
    let seconds = parts
        .first()
        .and_then(Value::as_i64)
        .ok_or("invalid span seconds")?;
    let nanos = parts
        .get(1)
        .and_then(Value::as_i64)
        .ok_or("invalid span nanoseconds")?;
    if seconds < 0 || !(0..1_000_000_000).contains(&nanos) {
        return Err("invalid span time");
    }
    seconds
        .checked_mul(1_000)
        .and_then(|millis| millis.checked_add(nanos / 1_000_000))
        .ok_or("span time overflow")
}

fn valid_w3c_id(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        && value.bytes().any(|byte| byte != b'0')
}

fn finish_reason(attributes: &Map<String, Value>) -> Result<String, &'static str> {
    let Some(value) = attributes.get("gen_ai.response.finish_reasons") else {
        return Ok("completed".to_string());
    };
    if let Some(reason) = value
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Ok(reason.to_string());
    }
    if let Some(reason) = value
        .as_array()
        .and_then(|values| values.first())
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Ok(reason.to_string());
    }
    Err("invalid finish reasons")
}

fn chat_entry(record: &Value, timezone: Timezone) -> Result<Option<RawEntry>, &'static str> {
    if record.get("type").and_then(Value::as_str) != Some("span") {
        return Ok(None);
    }
    let Some(attributes) = record.get("attributes").and_then(Value::as_object) else {
        return Ok(None);
    };
    if non_empty_attr(attributes, "gen_ai.operation.name") != Some("chat") {
        return Ok(None);
    }

    if !USAGE_KEYS.iter().any(|key| attributes.contains_key(*key)) {
        return Ok(None);
    }

    let input_total = strict_token(attributes, USAGE_KEYS[0])?;
    let output_total = strict_token(attributes, USAGE_KEYS[1])?;
    let cache_read = strict_token(attributes, USAGE_KEYS[2])?;
    let cache_creation = strict_token(attributes, USAGE_KEYS[3])?;
    let reasoning = strict_token(attributes, USAGE_KEYS[4])?;
    if cache_read.saturating_add(cache_creation) > input_total {
        return Err("cache tokens exceed total input");
    }
    if reasoning > output_total {
        return Err("reasoning tokens exceed total output");
    }
    if input_total == 0 && output_total == 0 {
        return Ok(None);
    }

    let start_ms = record
        .get("startTime")
        .ok_or("missing span start time")
        .and_then(span_timestamp_ms)?;
    if let Some(end_time) = record.get("endTime") {
        let end_ms = span_timestamp_ms(end_time)?;
        if end_ms < start_ms {
            return Err("span end precedes start");
        }
    }
    let timestamp = DateTime::<Utc>::from_timestamp_millis(start_ms)
        .ok_or("span time is outside supported range")?;

    let trace_id = record
        .get("traceId")
        .and_then(Value::as_str)
        .filter(|value| valid_w3c_id(value, 32));
    let span_id = record
        .get("spanId")
        .and_then(Value::as_str)
        .filter(|value| valid_w3c_id(value, 16));
    let response_id = non_empty_attr(attributes, "gen_ai.response.id");
    let message_identity = trace_id
        .zip(span_id)
        .map(|(trace, span)| format!("{trace}:{span}"))
        .or_else(|| response_id.map(|response| format!("response:{response}")));
    let session_id = non_empty_attr(attributes, "gen_ai.conversation.id")
        .or(trace_id)
        .unwrap_or("unknown-session")
        .to_string();
    let model = non_empty_attr(attributes, "gen_ai.response.model")
        .or_else(|| non_empty_attr(attributes, "gen_ai.request.model"))
        .unwrap_or(UNKNOWN)
        .to_string();

    Ok(Some(RawEntry {
        timestamp: timestamp.to_rfc3339(),
        timestamp_ms: start_ms,
        date_str: timezone
            .to_fixed_offset(timestamp)
            .date_naive()
            .format(DATE_FORMAT)
            .to_string(),
        message_id: message_identity
            .as_deref()
            .map(|identity| source_wide_message_id("copilot", identity)),
        session_key: format!("copilot::{session_id}"),
        session_id,
        project_path: String::new(),
        model,
        input_tokens: input_total - cache_read - cache_creation,
        output_tokens: output_total - reasoning,
        cache_creation,
        cache_creation_1h: 0,
        cache_read,
        reasoning_tokens: reasoning,
        stop_reason: Some(finish_reason(attributes)?),
        cost_kind: CostKind::Real,
        endpoint: Endpoint::Unknown,
        call_count: 1,
        // GitHub documents this as monetary cost but does not publish a currency code.
        recorded_cost_usd: None,
    }))
}

fn parse_copilot_file(path: &Path, timezone: Timezone, debug: bool) -> ParseOutput {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) => {
            if debug {
                eprintln!(
                    "Failed to open Copilot OTel file {}: {error}",
                    path.display()
                );
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
        let record: Value = match serde_json::from_str(&line) {
            Ok(record) => record,
            Err(error) => {
                output.errors += 1;
                if debug {
                    eprintln!(
                        "Invalid Copilot JSON in {} line {}: {error}",
                        path.display(),
                        line_index + 1
                    );
                }
                continue;
            }
        };
        match chat_entry(&record, timezone) {
            Ok(Some(entry)) => output.entries.push(entry),
            Ok(None) => {}
            Err(error) => {
                output.errors += 1;
                if debug {
                    eprintln!(
                        "Invalid Copilot usage in {} line {}: {error}",
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

    #[test]
    fn inclusive_sub_buckets_are_made_mutually_exclusive() {
        let record: Value = serde_json::from_str(
            r#"{"type":"span","traceId":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","spanId":"bbbbbbbbbbbbbbbb","startTime":[1788145445,0],"attributes":{"gen_ai.operation.name":"chat","gen_ai.response.model":"gpt-5","gen_ai.usage.input_tokens":100,"gen_ai.usage.output_tokens":20,"gen_ai.usage.cache_read.input_tokens":30,"gen_ai.usage.cache_creation.input_tokens":10,"gen_ai.usage.reasoning.output_tokens":5}}"#,
        )
        .unwrap();
        let entry = chat_entry(&record, Timezone::Named(chrono_tz::UTC))
            .unwrap()
            .unwrap();
        assert_eq!(entry.input_tokens, 60);
        assert_eq!(entry.output_tokens, 15);
        assert_eq!(entry.reasoning_tokens, 5);
    }

    #[test]
    fn conflicting_sub_buckets_are_an_error() {
        let record: Value = serde_json::from_str(
            r#"{"type":"span","startTime":[1788145445,0],"attributes":{"gen_ai.operation.name":"chat","gen_ai.usage.input_tokens":10,"gen_ai.usage.output_tokens":2,"gen_ai.usage.cache_read.input_tokens":8,"gen_ai.usage.cache_creation.input_tokens":3}}"#,
        )
        .unwrap();
        assert!(matches!(
            chat_entry(&record, Timezone::Named(chrono_tz::UTC)),
            Err("cache tokens exceed total input")
        ));
    }
}
