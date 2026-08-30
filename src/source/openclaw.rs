//! `OpenClaw` v3 transcript usage source.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::consts::{DATE_FORMAT, UNKNOWN};
use crate::core::{CostKind, Endpoint, RawEntry, source_wide_message_id};
use crate::source::openclaw_store::{find_transcript_stores, load_transcripts};
use crate::source::{Capabilities, ParseOutput, Source};
use crate::utils::Timezone;

pub(crate) struct OpenClawSource;

impl OpenClawSource {
    pub(crate) const fn new() -> Self {
        Self
    }
}

impl Source for OpenClawSource {
    fn name(&self) -> &'static str {
        "openclaw"
    }

    fn display_name(&self) -> &'static str {
        "OpenClaw"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            has_projects: true,
            has_billing_blocks: false,
            has_reasoning_tokens: false,
            has_cache_creation: true,
            has_cache_read: true,
            needs_dedup: true,
            has_tool_calls: false,
            has_endpoints: false,
        }
    }

    fn find_files(&self) -> Vec<PathBuf> {
        find_transcript_stores()
    }

    fn parse_file(&self, path: &Path, timezone: Timezone, debug: bool) -> ParseOutput {
        parse_transcript(path, timezone, debug)
    }
}

#[derive(Deserialize)]
struct TranscriptRow {
    #[serde(rename = "type")]
    kind: String,
    id: Option<String>,
    timestamp: Option<String>,
    version: Option<u32>,
    cwd: Option<String>,
    provider: Option<String>,
    #[serde(rename = "modelId")]
    model_id: Option<String>,
    message: Option<AssistantMessage>,
    #[serde(rename = "customType")]
    custom_type: Option<String>,
    data: Option<ModelSnapshot>,
}

#[derive(Deserialize)]
struct ModelSnapshot {
    provider: Option<String>,
    #[serde(rename = "modelId")]
    model_id: Option<String>,
}

#[derive(Deserialize)]
struct AssistantMessage {
    role: String,
    provider: Option<String>,
    model: Option<String>,
    timestamp: Option<i64>,
    usage: Option<Usage>,
}

#[derive(Deserialize)]
struct Usage {
    input: i64,
    output: i64,
    #[serde(rename = "cacheRead")]
    cache_read: i64,
    #[serde(rename = "cacheWrite")]
    cache_write: i64,
    #[serde(rename = "cacheWrite1h", default)]
    cache_write_1h: i64,
    cost: Option<UsageCost>,
}

#[derive(Deserialize)]
struct UsageCost {
    total: f64,
    #[serde(rename = "totalOrigin")]
    total_origin: Option<String>,
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.trim().is_empty())
}

fn timestamp_from_row(
    message_timestamp: Option<i64>,
    row_timestamp: Option<&str>,
) -> Result<DateTime<Utc>, &'static str> {
    if let Some(timestamp) = message_timestamp {
        return DateTime::<Utc>::from_timestamp_millis(timestamp)
            .filter(|_| timestamp > 0)
            .ok_or("invalid message timestamp");
    }
    row_timestamp
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.with_timezone(&Utc))
        .ok_or("missing or invalid entry timestamp")
}

fn build_entry(
    row: TranscriptRow,
    session_id: &str,
    project_path: &str,
    current_provider: Option<&str>,
    current_model: Option<&str>,
    timezone: Timezone,
) -> Result<Option<RawEntry>, &'static str> {
    let Some(message) = row.message else {
        return Err("message row is missing message payload");
    };
    if message.role != "assistant" {
        return Ok(None);
    }
    let Some(usage) = message.usage else {
        return Ok(None);
    };
    if [
        usage.input,
        usage.output,
        usage.cache_read,
        usage.cache_write,
        usage.cache_write_1h,
    ]
    .into_iter()
    .any(|value| value < 0)
    {
        return Err("negative token count");
    }
    if usage.cache_write_1h > usage.cache_write {
        return Err("one-hour cache writes exceed total cache writes");
    }
    let recorded_cost_usd = match usage.cost {
        Some(cost) if !cost.total.is_finite() || cost.total < 0.0 => return Err("invalid cost"),
        Some(cost) if cost.total_origin.as_deref() == Some("provider-billed") => Some(cost.total),
        Some(_) | None => None,
    };
    if usage.input == 0
        && usage.output == 0
        && usage.cache_read == 0
        && usage.cache_write == 0
        && recorded_cost_usd.is_none()
    {
        return Ok(None);
    }
    let id = non_empty(row.id).ok_or("message row is missing id")?;
    let model = non_empty(message.model)
        .or_else(|| current_model.map(str::to_string))
        .ok_or("assistant message is missing model")?;
    let _provider = non_empty(message.provider)
        .or_else(|| current_provider.map(str::to_string))
        .unwrap_or_else(|| UNKNOWN.to_string());
    let timestamp = timestamp_from_row(message.timestamp, row.timestamp.as_deref())?;
    let timestamp_ms = timestamp.timestamp_millis();

    Ok(Some(RawEntry {
        timestamp: timestamp.to_rfc3339(),
        timestamp_ms,
        date_str: timezone
            .to_fixed_offset(timestamp)
            .date_naive()
            .format(DATE_FORMAT)
            .to_string(),
        message_id: Some(source_wide_message_id("openclaw", &id)),
        session_key: format!("openclaw::{session_id}"),
        session_id: session_id.to_string(),
        project_path: project_path.to_string(),
        model,
        input_tokens: usage.input,
        output_tokens: usage.output,
        cache_creation: usage.cache_write,
        cache_creation_1h: usage.cache_write_1h,
        cache_read: usage.cache_read,
        reasoning_tokens: 0,
        stop_reason: Some("completed".to_string()),
        cost_kind: CostKind::Real,
        endpoint: Endpoint::Unknown,
        call_count: 1,
        recorded_cost_usd,
    }))
}

fn parse_transcript(path: &Path, timezone: Timezone, debug: bool) -> ParseOutput {
    let loaded = match load_transcripts(path) {
        Ok(loaded) => loaded,
        Err(error) => {
            if debug {
                eprintln!(
                    "Failed to read OpenClaw transcript store {}: {error}",
                    path.display()
                );
            }
            return ParseOutput {
                entries: Vec::new(),
                errors: 1,
            };
        }
    };
    let mut output = ParseOutput {
        entries: Vec::new(),
        errors: loaded.errors,
    };
    for lines in loaded.transcripts {
        let parsed = parse_lines(lines, timezone);
        output.entries.extend(parsed.entries);
        output.errors += parsed.errors;
    }
    output
}

fn parse_lines(lines: Vec<String>, timezone: Timezone) -> ParseOutput {
    let mut output = ParseOutput::default();
    let mut session_id = None;
    let mut project_path = None;
    let mut current_provider = None;
    let mut current_model = None;

    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        let Ok(row) = serde_json::from_str::<TranscriptRow>(&line) else {
            output.errors += 1;
            continue;
        };
        match row.kind.as_str() {
            "session" => {
                if row.version.is_some_and(|version| version != 3)
                    || row.id.as_deref().is_none_or(str::is_empty)
                {
                    output.errors += 1;
                    continue;
                }
                session_id = row.id;
                project_path = non_empty(row.cwd);
            }
            "model_change" => {
                current_provider = non_empty(row.provider);
                current_model = non_empty(row.model_id);
            }
            "custom" if row.custom_type.as_deref() == Some("model-snapshot") => {
                if let Some(data) = row.data {
                    current_provider = non_empty(data.provider).or(current_provider);
                    current_model = non_empty(data.model_id).or(current_model);
                }
            }
            "message" => {
                let (Some(session_id), Some(project_path)) =
                    (session_id.as_deref(), project_path.as_deref())
                else {
                    output.errors += 1;
                    continue;
                };
                match build_entry(
                    row,
                    session_id,
                    project_path,
                    current_provider.as_deref(),
                    current_model.as_deref(),
                    timezone,
                ) {
                    Ok(Some(entry)) => output.entries.push(entry),
                    Ok(None) => {}
                    Err(_) => output.errors += 1,
                }
            }
            _ => {}
        }
    }
    output
}
