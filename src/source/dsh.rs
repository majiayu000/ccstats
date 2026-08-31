//! `DeepSeek` Harness durable session usage.

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::consts::DATE_FORMAT;
use crate::utils::Timezone;

use super::dsh_format::{
    matches_storage_identity, packed_row_span, supported_event, valid_event_envelope,
    valid_header_shape,
};
use super::dsh_usage::{Usage, usage_entry};
use super::{Capabilities, ParseOutput, Source, dsh_zstd};

const MAX_SAFE_INTEGER: i64 = 9_007_199_254_740_991;

pub(crate) struct DshSource {
    mixed_roots: Mutex<BTreeMap<PathBuf, bool>>,
}

impl DshSource {
    pub(crate) fn new() -> Self {
        Self {
            mixed_roots: Mutex::new(BTreeMap::new()),
        }
    }

    fn discover_root(&self, root: &Path) -> Vec<PathBuf> {
        let (files, mixed) = find_session_files(root);
        self.mixed_roots
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(root.to_path_buf(), mixed);
        files
    }

    fn discovered_root_is_mixed(&self, path: &Path) -> Option<bool> {
        let root = session_root(path)?;
        self.mixed_roots
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(root)
            .copied()
    }
}

impl Source for DshSource {
    fn name(&self) -> &'static str {
        "dsh"
    }

    fn display_name(&self) -> &'static str {
        "DeepSeek Harness"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            has_projects: true,
            has_billing_blocks: false,
            has_reasoning_tokens: true,
            has_cache_creation: true,
            has_cache_read: true,
            needs_dedup: false,
            has_tool_calls: false,
            has_endpoints: false,
        }
    }

    fn find_files(&self) -> Vec<PathBuf> {
        dsh_home()
            .map(|home| self.discover_root(&home.join("sessions")))
            .unwrap_or_default()
    }

    fn parse_file(&self, path: &Path, timezone: Timezone, debug: bool) -> ParseOutput {
        let Some(root_is_mixed) = self.discovered_root_is_mixed(path) else {
            return ParseOutput {
                entries: Vec::new(),
                errors: 1,
            };
        };
        parse_file(path, timezone, debug, root_is_mixed)
    }
}

fn configured_home() -> Option<PathBuf> {
    let value = env::var("DSH_HOME").ok()?;
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let path = if value == "~" {
        dirs::home_dir()?
    } else if let Some(relative) = value
        .strip_prefix("~/")
        .or_else(|| value.strip_prefix("~\\"))
    {
        dirs::home_dir()?.join(relative)
    } else {
        PathBuf::from(value)
    };
    if path.is_absolute() {
        Some(path)
    } else {
        env::current_dir().ok().map(|cwd| cwd.join(path))
    }
}

fn dsh_home() -> Option<PathBuf> {
    configured_home().or_else(|| dirs::home_dir().map(|home| home.join(".dsh")))
}

fn find_session_files(root: &Path) -> (Vec<PathBuf>, bool) {
    let mut by_directory = BTreeMap::<PathBuf, Vec<PathBuf>>::new();
    for project in child_directories(root) {
        for session in child_directories(&project) {
            for filename in ["session.jsonl", "session.jsonl.zstd"] {
                let path = session.join(filename);
                if path.is_file() {
                    by_directory.entry(session.clone()).or_default().push(path);
                }
            }
        }
    }
    let mut files = by_directory
        .into_values()
        .filter_map(|mut files| {
            files.sort();
            files.into_iter().next()
        })
        .collect::<Vec<_>>();
    files.sort();
    let mut physical_files = BTreeSet::new();
    files.retain(|path| {
        let physical = fs::canonicalize(path).unwrap_or_else(|_| path.clone());
        physical_files.insert(physical)
    });
    let has_plain = files.iter().any(|path| is_plain(path));
    let has_zstd = files.iter().any(|path| !is_plain(path));
    let mixed = has_plain && has_zstd;
    let files = if mixed {
        files.into_iter().take(1).collect()
    } else {
        files
    };
    (files, mixed)
}

fn child_directories(root: &Path) -> Vec<PathBuf> {
    let mut directories = fs::read_dir(root)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|entry| {
            entry
                .file_type()
                .ok()
                .filter(std::fs::FileType::is_dir)
                .map(|_| entry.path())
        })
        .collect::<Vec<_>>();
    directories.sort();
    directories
}

fn is_plain(path: &Path) -> bool {
    path.file_name().and_then(|name| name.to_str()) == Some("session.jsonl")
}

#[derive(Deserialize)]
struct Header {
    #[serde(rename = "type")]
    kind: String,
    version: i64,
    id: String,
    #[serde(rename = "createdAt")]
    created_at: i64,
    cwd: Option<String>,
    #[serde(rename = "seedLength")]
    seed_length: Option<i64>,
    #[serde(rename = "delegationDepth")]
    delegation_depth: i64,
    #[serde(rename = "parentSession")]
    _parent_session: Option<String>,
    origin: Option<String>,
    #[serde(rename = "agentPreset")]
    _agent_preset: Option<String>,
}

#[derive(Deserialize)]
struct Event {
    #[serde(rename = "type")]
    kind: String,
    seq: i64,
    time: i64,
    data: serde_json::Value,
}

#[derive(Deserialize)]
struct Route {
    provider: String,
    model: String,
}

#[derive(Deserialize)]
struct RequestHeaderData {
    header: RequestHeader,
}

#[derive(Deserialize)]
struct RequestHeader {
    config: Route,
}

#[derive(Deserialize)]
struct ChunkData {
    turn: i64,
    step: i64,
    chunk: Chunk,
}

#[derive(Deserialize)]
struct Chunk {
    #[serde(rename = "type")]
    kind: String,
    usage: Option<Usage>,
}

#[derive(Deserialize)]
struct AssistantData {
    turn: i64,
    step: i64,
    message: AssistantMessage,
    usage: Option<Usage>,
}

#[derive(Deserialize)]
struct AssistantMessage {
    id: String,
    role: String,
    content: Vec<MessageBlock>,
    source: MessageSource,
}

#[derive(Deserialize)]
struct MessageBlock {
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Deserialize)]
struct MessageSource {
    kind: String,
    provider: String,
    model: String,
    #[serde(rename = "replayState")]
    replay_state: Option<serde_json::Value>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RetryStarted {
    retry_id: String,
    turn: i64,
    step: i64,
    retry: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CompactionData {
    compaction_id: String,
    provider: String,
    model: String,
    usage: Option<Usage>,
}

struct LastAttempt {
    turn: i64,
    step: i64,
    entry_index: usize,
    identity: String,
}

fn parse_file(path: &Path, timezone: Timezone, debug: bool, root_is_mixed: bool) -> ParseOutput {
    if root_is_mixed || sibling_encoding(path).is_some_and(|sibling| sibling.is_file()) {
        return ParseOutput {
            entries: Vec::new(),
            errors: 1,
        };
    }
    let bytes = if path.extension().and_then(|value| value.to_str()) == Some("zstd") {
        dsh_zstd::decode_file(path)
    } else {
        dsh_zstd::read_stable(path)
    };
    match bytes {
        Ok(bytes) => parse_log(&bytes, path, timezone),
        Err(error) => {
            if debug {
                eprintln!("Failed to read DSH session {}: {error}", path.display());
            }
            ParseOutput {
                entries: Vec::new(),
                errors: 1,
            }
        }
    }
}

fn session_root(path: &Path) -> Option<&Path> {
    path.parent().and_then(Path::parent).and_then(Path::parent)
}

fn sibling_encoding(path: &Path) -> Option<PathBuf> {
    let parent = path.parent()?;
    match path.file_name()?.to_str()? {
        "session.jsonl" => Some(parent.join("session.jsonl.zstd")),
        "session.jsonl.zstd" => Some(parent.join("session.jsonl")),
        _ => None,
    }
}

#[allow(clippy::too_many_lines)]
fn parse_log(bytes: &[u8], path: &Path, timezone: Timezone) -> ParseOutput {
    let mut records = bytes.split_inclusive(|byte| *byte == b'\n');
    let Some(header_line) = records.next().filter(|line| line.ends_with(b"\n")) else {
        return ParseOutput {
            entries: Vec::new(),
            errors: 1,
        };
    };
    let header_bytes = &header_line[..header_line.len() - 1];
    let Ok(header) = serde_json::from_slice::<Header>(header_bytes) else {
        return ParseOutput {
            entries: Vec::new(),
            errors: 1,
        };
    };
    if !valid_header_shape(header_bytes)
        || !valid_header(&header)
        || !matches_storage_identity(path, &header.id, header.cwd.as_deref())
    {
        return ParseOutput {
            entries: Vec::new(),
            errors: 1,
        };
    }

    let seed_length = header.seed_length.unwrap_or(0);
    let mut output = ParseOutput::default();
    let mut route = None;
    let mut last_attempt: Option<LastAttempt> = None;
    let mut attempt_number = 0_usize;
    let mut expected_seq = 0_i64;
    for line in records {
        if !line.ends_with(b"\n") {
            break;
        }
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(&line[..line.len() - 1]) else {
            output.errors += 1;
            break;
        };
        let kind = value.get("type").and_then(serde_json::Value::as_str);
        if matches!(
            kind,
            Some("text-chunks" | "reasoning-chunks" | "tool-call-chunks")
        ) {
            let Some(packed_kind) = kind else {
                output.errors += 1;
                break;
            };
            let Some((seq0, item_count)) = packed_row_span(&value, packed_kind) else {
                output.errors += 1;
                break;
            };
            if seq0 != expected_seq {
                output.errors += 1;
                break;
            }
            let Some(next_seq) = expected_seq.checked_add(item_count) else {
                output.errors += 1;
                break;
            };
            expected_seq = next_seq;
            continue;
        }
        if !valid_event_envelope(&value)
            || kind.is_some_and(|kind| !supported_event(kind))
                && value.get("ignorable") != Some(&serde_json::Value::Bool(true))
        {
            output.errors += 1;
            break;
        }
        let Ok(event) = serde_json::from_value::<Event>(value) else {
            output.errors += 1;
            break;
        };
        if event.seq != expected_seq || event.time < 0 {
            output.errors += 1;
            break;
        }
        let Some(next_seq) = expected_seq.checked_add(1) else {
            output.errors += 1;
            break;
        };
        expected_seq = next_seq;
        if event.seq < seed_length {
            if event.kind == "request/header" && !replace_route(event.data, &mut route) {
                output.errors += 1;
            }
            continue;
        }
        match event.kind.as_str() {
            "request/header" => {
                if !replace_route(event.data, &mut route) {
                    output.errors += 1;
                }
            }
            "assistant/chunk" => {
                let Ok(data) = serde_json::from_value::<ChunkData>(event.data) else {
                    output.errors += 1;
                    continue;
                };
                if data.chunk.kind != "usage" {
                    continue;
                }
                let (Some(usage), Some(model)) = (
                    data.chunk.usage,
                    route.as_ref().and_then(|route| non_blank(&route.model)),
                ) else {
                    output.errors += 1;
                    continue;
                };
                record_attempt(
                    &header,
                    event.time,
                    data.turn,
                    data.step,
                    model,
                    usage,
                    timezone,
                    &mut output,
                    &mut last_attempt,
                    &mut attempt_number,
                );
            }
            "assistant/message" => {
                let attempt = event
                    .data
                    .get("turn")
                    .and_then(serde_json::Value::as_i64)
                    .zip(event.data.get("step").and_then(serde_json::Value::as_i64));
                let Ok(data) = serde_json::from_value::<AssistantData>(event.data) else {
                    if let Some((turn, step)) = attempt {
                        invalidate_attempt(turn, step, &mut output, &mut last_attempt);
                    }
                    output.errors += 1;
                    continue;
                };
                if data.message.id.is_empty() {
                    invalidate_attempt(data.turn, data.step, &mut output, &mut last_attempt);
                    output.errors += 1;
                    continue;
                }
                let Some(model) = message_model(&data.message) else {
                    invalidate_attempt(data.turn, data.step, &mut output, &mut last_attempt);
                    output.errors += 1;
                    continue;
                };
                if let Some(usage) = data.usage {
                    record_attempt(
                        &header,
                        event.time,
                        data.turn,
                        data.step,
                        model,
                        usage,
                        timezone,
                        &mut output,
                        &mut last_attempt,
                        &mut attempt_number,
                    );
                } else {
                    reattribute_attempt(
                        event.time,
                        data.turn,
                        data.step,
                        model,
                        timezone,
                        &mut output,
                        last_attempt.as_ref(),
                    );
                }
            }
            "llm/retry-started" => {
                let Ok(retry) = serde_json::from_value::<RetryStarted>(event.data) else {
                    output.errors += 1;
                    break;
                };
                if !valid_retry_boundary(&retry)
                    || last_attempt
                        .as_ref()
                        .is_some_and(|last| last.turn != retry.turn || last.step != retry.step)
                {
                    output.errors += 1;
                    break;
                }
                last_attempt = None;
            }
            "compaction/summary" => {
                let Ok(data) = serde_json::from_value::<CompactionData>(event.data) else {
                    output.errors += 1;
                    continue;
                };
                let Some(usage) = data.usage else { continue };
                let Some(model) = non_blank(&data.model) else {
                    output.errors += 1;
                    continue;
                };
                if non_blank(&data.provider).is_none() || non_blank(&data.compaction_id).is_none() {
                    output.errors += 1;
                    continue;
                }
                let identity = format!("{}:compaction:{}", header.id, data.compaction_id);
                match usage_entry(
                    &header.id,
                    header.cwd.as_deref(),
                    event.time,
                    model,
                    usage,
                    timezone,
                    &identity,
                ) {
                    Ok(entry) => output.entries.push(entry),
                    Err(_) => output.errors += 1,
                }
            }
            _ => {}
        }
    }
    output
}

fn valid_header(header: &Header) -> bool {
    header.kind == "session"
        && header.version == 0
        && (0..=MAX_SAFE_INTEGER).contains(&header.created_at)
        && (0..=MAX_SAFE_INTEGER).contains(&header.delegation_depth)
        && !header.id.is_empty()
        && header
            .seed_length
            .is_none_or(|value| (0..=MAX_SAFE_INTEGER).contains(&value))
        && header
            .origin
            .as_deref()
            .is_none_or(|origin| origin == "subagent")
        && header
            .cwd
            .as_deref()
            .is_none_or(|cwd| Path::new(cwd).is_absolute())
}

fn replace_route(data: serde_json::Value, route: &mut Option<Route>) -> bool {
    *route = None;
    let Ok(next) = serde_json::from_value::<RequestHeaderData>(data) else {
        return false;
    };
    if non_blank(&next.header.config.provider).is_none()
        || non_blank(&next.header.config.model).is_none()
    {
        return false;
    }
    *route = Some(next.header.config);
    true
}

fn valid_retry_boundary(retry: &RetryStarted) -> bool {
    non_blank(&retry.retry_id).is_some()
        && (0..=MAX_SAFE_INTEGER).contains(&retry.turn)
        && (0..=MAX_SAFE_INTEGER).contains(&retry.step)
        && (1..=MAX_SAFE_INTEGER).contains(&retry.retry)
}

fn invalidate_attempt(
    turn: i64,
    step: i64,
    output: &mut ParseOutput,
    last_attempt: &mut Option<LastAttempt>,
) {
    let Some(index) = last_attempt
        .as_ref()
        .filter(|last| last.turn == turn && last.step == step)
        .map(|last| last.entry_index)
    else {
        return;
    };
    output.entries.remove(index);
    *last_attempt = None;
}

fn reattribute_attempt(
    event_time: i64,
    turn: i64,
    step: i64,
    model: String,
    timezone: Timezone,
    output: &mut ParseOutput,
    last_attempt: Option<&LastAttempt>,
) {
    let Some(last) = last_attempt.filter(|last| last.turn == turn && last.step == step) else {
        return;
    };
    let Some(timestamp) = DateTime::<Utc>::from_timestamp_millis(event_time) else {
        output.errors += 1;
        return;
    };
    let entry = &mut output.entries[last.entry_index];
    entry.model = model;
    entry.timestamp = timestamp.to_rfc3339();
    entry.timestamp_ms = event_time;
    entry.date_str = timezone
        .to_fixed_offset(timestamp)
        .date_naive()
        .format(DATE_FORMAT)
        .to_string();
}

#[allow(clippy::too_many_arguments)]
fn record_attempt(
    header: &Header,
    event_time: i64,
    turn: i64,
    step: i64,
    model: String,
    usage: Usage,
    timezone: Timezone,
    output: &mut ParseOutput,
    last_attempt: &mut Option<LastAttempt>,
    attempt_number: &mut usize,
) {
    let replacement = last_attempt
        .as_ref()
        .filter(|last| last.turn == turn && last.step == step)
        .map(|last| (last.entry_index, last.identity.clone()));
    let identity = replacement.as_ref().map_or_else(
        || format!("{}:attempt:{}", header.id, *attempt_number),
        |(_, identity)| identity.clone(),
    );
    if let Ok(entry) = usage_entry(
        &header.id,
        header.cwd.as_deref(),
        event_time,
        model,
        usage,
        timezone,
        &identity,
    ) {
        if let Some((entry_index, _)) = replacement {
            output.entries[entry_index] = entry;
        } else {
            let entry_index = output.entries.len();
            output.entries.push(entry);
            *attempt_number += 1;
            *last_attempt = Some(LastAttempt {
                turn,
                step,
                entry_index,
                identity,
            });
        }
    } else {
        if replacement.is_some() {
            invalidate_attempt(turn, step, output, last_attempt);
        }
        output.errors += 1;
    }
}

fn message_model(message: &AssistantMessage) -> Option<String> {
    if message.role != "assistant" || message.source.kind != "model" {
        return None;
    }
    let source_model = non_blank(&message.source.model)?;
    let source_provider = non_blank(&message.source.provider)?;
    let response = message
        .source
        .replay_state
        .as_ref()
        .and_then(|state| state.get("response"));
    let blocks = message
        .source
        .replay_state
        .as_ref()
        .and_then(|state| state.get("blocks"));
    let response_model = response.and_then(|response| {
        (valid_pi_response(response, &source_provider, &source_model)
            && valid_pi_blocks(blocks?, &message.content))
        .then(|| response.get("responseModel")?.as_str())
        .flatten()
        .and_then(non_blank)
    });
    response_model.or(Some(source_model))
}

fn valid_pi_response(response: &serde_json::Value, provider: &str, model: &str) -> bool {
    response.get("kind").and_then(serde_json::Value::as_str) == Some("pi-ai")
        && response.get("version").and_then(serde_json::Value::as_i64) == Some(2)
        && response
            .get("api")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|value| !value.is_empty())
        && response.get("provider").and_then(serde_json::Value::as_str) == Some(provider)
        && response.get("model").and_then(serde_json::Value::as_str) == Some(model)
        && response
            .get("stopReason")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|reason| {
                matches!(reason, "stop" | "length" | "toolUse" | "error" | "aborted")
            })
        && response
            .get("responseModel")
            .is_none_or(serde_json::Value::is_string)
        && response
            .get("responseId")
            .is_none_or(serde_json::Value::is_string)
}

fn valid_pi_blocks(blocks: &serde_json::Value, content: &[MessageBlock]) -> bool {
    let Some(blocks) = blocks
        .as_array()
        .filter(|blocks| blocks.len() == content.len())
    else {
        return false;
    };
    blocks.iter().zip(content).all(|(block, content)| {
        let Some(block) = block.as_object() else {
            return false;
        };
        let Some(kind) = block.get("type").and_then(serde_json::Value::as_str) else {
            return false;
        };
        kind == content.kind
            && matches!(kind, "text" | "reasoning" | "tool-call")
            && ["textSignature", "thinkingSignature", "thoughtSignature"]
                .into_iter()
                .all(|key| block.get(key).is_none_or(serde_json::Value::is_string))
            && block
                .get("redacted")
                .is_none_or(serde_json::Value::is_boolean)
    })
}

fn non_blank(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

#[cfg(test)]
mod tests;
