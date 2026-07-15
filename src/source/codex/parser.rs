//! `OpenAI` Codex CLI JSONL parser
//!
//! Parses JSONL logs from ~/.codex/sessions/ directory.
//! Codex log format uses cumulative token counts that need delta computation.

use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::env;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use crate::consts::{DATE_FORMAT, UNKNOWN};
use crate::core::RawEntry;
use crate::source::ParseOutput;
use crate::utils::Timezone;

const DEFAULT_CODEX_DIR: &str = ".codex";
const CODEX_HOME_ENV: &str = "CODEX_HOME";
const SESSION_SUBDIR: &str = "sessions";
const CODEX_USAGE_MESSAGE_PREFIX: &str = "source-wide:codex-token-count";
const SESSION_USAGE_MESSAGE_PREFIX: &str = "codex-token-count";

// ============================================================================
// Internal types for JSONL parsing
// ============================================================================

#[derive(Debug, Deserialize)]
struct RawJsonEntry<'a> {
    timestamp: Option<&'a str>,
    #[serde(rename = "type")]
    entry_type: Option<&'a str>,
    payload: Option<Payload<'a>>,
}

#[derive(Debug, Deserialize)]
#[allow(clippy::struct_field_names)] // field names match JSON schema
struct Payload<'a> {
    #[serde(rename = "type")]
    payload_type: Option<&'a str>,
    id: Option<&'a str>,
    info: Option<TokenInfo<'a>>,
    model: Option<&'a str>,
}

#[derive(Debug, Deserialize)]
struct TokenInfo<'a> {
    total_token_usage: Option<TokenUsage>,
    last_token_usage: Option<TokenUsage>,
    model: Option<&'a str>,
    model_name: Option<&'a str>,
    metadata: Option<Metadata<'a>>,
}

#[derive(Debug, Deserialize)]
struct Metadata<'a> {
    model: Option<&'a str>,
}

#[derive(Debug, Deserialize, Clone, Default)]
#[allow(clippy::struct_field_names)] // field names match JSON schema
struct TokenUsage {
    input_tokens: Option<i64>,
    cached_input_tokens: Option<i64>,
    #[serde(alias = "cache_read_input_tokens")]
    alt_cache_read_input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    reasoning_output_tokens: Option<i64>,
    total_tokens: Option<i64>,
}

impl TokenUsage {
    fn cached_input(&self) -> i64 {
        self.cached_input_tokens
            .or(self.alt_cache_read_input_tokens)
            .unwrap_or(0)
    }

    #[cfg(test)]
    fn subtract(&self, prev: &TokenUsage) -> TokenUsage {
        TokenUsage {
            input_tokens: Some(
                (self.input_tokens.unwrap_or(0) - prev.input_tokens.unwrap_or(0)).max(0),
            ),
            cached_input_tokens: Some((self.cached_input() - prev.cached_input()).max(0)),
            alt_cache_read_input_tokens: None,
            output_tokens: Some(
                (self.output_tokens.unwrap_or(0) - prev.output_tokens.unwrap_or(0)).max(0),
            ),
            reasoning_output_tokens: Some(
                (self.reasoning_output_tokens.unwrap_or(0)
                    - prev.reasoning_output_tokens.unwrap_or(0))
                .max(0),
            ),
            total_tokens: Some(
                (self.total_tokens.unwrap_or(0) - prev.total_tokens.unwrap_or(0)).max(0),
            ),
        }
    }

    #[cfg(test)]
    fn is_empty(&self) -> bool {
        self.input_tokens.unwrap_or(0) == 0
            && self.cached_input() == 0
            && self.output_tokens.unwrap_or(0) == 0
            && self.reasoning_output_tokens.unwrap_or(0) == 0
    }
}

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
#[allow(clippy::struct_field_names)] // field names mirror normalized token fields
struct UsageTotals {
    input_tokens: i64,
    cached_input_tokens: i64,
    output_tokens: i64,
    reasoning_output_tokens: i64,
    total_tokens: i64,
}

impl UsageTotals {
    fn from_usage(usage: &TokenUsage) -> Self {
        Self {
            input_tokens: usage.input_tokens.unwrap_or(0),
            cached_input_tokens: usage.cached_input(),
            output_tokens: usage.output_tokens.unwrap_or(0),
            reasoning_output_tokens: usage.reasoning_output_tokens.unwrap_or(0),
            total_tokens: usage.total_tokens.unwrap_or(0),
        }
    }

    fn subtract(self, prev: Self) -> Self {
        Self {
            input_tokens: (self.input_tokens - prev.input_tokens).max(0),
            cached_input_tokens: (self.cached_input_tokens - prev.cached_input_tokens).max(0),
            output_tokens: (self.output_tokens - prev.output_tokens).max(0),
            reasoning_output_tokens: (self.reasoning_output_tokens - prev.reasoning_output_tokens)
                .max(0),
            total_tokens: (self.total_tokens - prev.total_tokens).max(0),
        }
    }

    fn is_duplicate_of(&self, prev: &Self) -> bool {
        self == prev
    }

    fn is_empty(self) -> bool {
        self.input_tokens == 0
            && self.cached_input_tokens == 0
            && self.output_tokens == 0
            && self.reasoning_output_tokens == 0
    }
}

// ============================================================================
// File discovery
// ============================================================================

fn get_codex_sessions_dir() -> Option<PathBuf> {
    // Check CODEX_HOME env var first
    if let Ok(codex_home) = env::var(CODEX_HOME_ENV) {
        let path = PathBuf::from(codex_home).join(SESSION_SUBDIR);
        if path.is_dir() {
            return Some(path);
        }
    }

    // Fall back to ~/.codex/sessions
    let home = dirs::home_dir()?;
    let path = home.join(DEFAULT_CODEX_DIR).join(SESSION_SUBDIR);
    if path.is_dir() { Some(path) } else { None }
}

pub(super) fn find_codex_files() -> Vec<PathBuf> {
    let Some(sessions_dir) = get_codex_sessions_dir() else {
        return Vec::new();
    };

    let mut files = Vec::new();
    if let Ok(entries) = glob::glob(&format!("{}/**/*.jsonl", sessions_dir.display())) {
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

fn non_empty_model(model: Option<&str>) -> Option<&str> {
    model.filter(|m| !m.trim().is_empty())
}

fn extract_model_ref<'a>(payload: &'a Payload<'a>) -> Option<&'a str> {
    if let Some(info) = &payload.info
        && let Some(model) = non_empty_model(info.model)
            .or_else(|| non_empty_model(info.model_name))
            .or_else(|| non_empty_model(info.metadata.as_ref().and_then(|metadata| metadata.model)))
    {
        return Some(model);
    }

    non_empty_model(payload.model)
}

fn usage_message_id(
    model: &str,
    logical_session_key: &str,
    total: UsageTotals,
    delta: UsageTotals,
) -> String {
    let prefix = if total == delta {
        SESSION_USAGE_MESSAGE_PREFIX
    } else {
        CODEX_USAGE_MESSAGE_PREFIX
    };
    format!(
        "{prefix}:{logical_session_key}:{model}:total={},{},{},{},{}:delta={},{},{},{},{}",
        total.input_tokens,
        total.cached_input_tokens,
        total.output_tokens,
        total.reasoning_output_tokens,
        total.total_tokens,
        delta.input_tokens,
        delta.cached_input_tokens,
        delta.output_tokens,
        delta.reasoning_output_tokens,
        delta.total_tokens
    )
}

#[cfg(test)]
fn extract_model(payload: &Payload<'_>) -> Option<String> {
    extract_model_ref(payload).map(std::string::ToString::to_string)
}

struct CodexFileIdentity {
    session_key: String,
    session_id: String,
}

impl CodexFileIdentity {
    fn from_path(path: &Path) -> Self {
        Self {
            session_key: path.display().to_string(),
            session_id: path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(UNKNOWN)
                .to_string(),
        }
    }
}

struct CodexParseContext<'a> {
    path: &'a Path,
    timezone: Timezone,
    debug: bool,
    identity: CodexFileIdentity,
}

struct CodexParseState {
    entries: Vec<RawEntry>,
    parse_errors: usize,
    previous_totals: Option<UsageTotals>,
    current_model: Option<String>,
    logical_session_key: String,
}

impl CodexParseState {
    fn new(capacity: usize, session_key: String) -> Self {
        Self {
            entries: Vec::with_capacity(capacity),
            parse_errors: 0,
            previous_totals: None,
            current_model: None,
            logical_session_key: session_key,
        }
    }

    fn finish(self) -> ParseOutput {
        ParseOutput {
            entries: self.entries,
            errors: self.parse_errors,
        }
    }
}

pub(super) fn parse_codex_file_with_debug(
    path: &Path,
    timezone: Timezone,
    debug: bool,
) -> ParseOutput {
    let identity = CodexFileIdentity::from_path(path);
    let context = CodexParseContext {
        path,
        timezone,
        debug,
        identity,
    };
    let file = match open_codex_file(&context) {
        Ok(file) => file,
        Err(output) => return output,
    };
    let estimated_capacity = estimate_entry_capacity(&file, 260);
    let mut state = CodexParseState::new(estimated_capacity, context.identity.session_key.clone());

    parse_codex_reader(BufReader::new(file), &context, &mut state);
    state.finish()
}

fn open_codex_file(context: &CodexParseContext<'_>) -> Result<File, ParseOutput> {
    File::open(context.path).map_err(|err| {
        if context.debug {
            eprintln!("Failed to open {}: {}", context.path.display(), err);
        }
        ParseOutput {
            entries: Vec::new(),
            errors: 1,
        }
    })
}

fn parse_codex_reader<R: BufRead>(
    mut reader: R,
    context: &CodexParseContext<'_>,
    state: &mut CodexParseState,
) {
    let mut line = String::new();
    let mut line_no = 0usize;
    loop {
        line.clear();
        let bytes_read = match reader.read_line(&mut line) {
            Ok(n) => n,
            Err(err) => {
                line_no += 1;
                if context.debug {
                    eprintln!(
                        "Failed to read line {} in {}: {}",
                        line_no,
                        context.path.display(),
                        err
                    );
                }
                state.parse_errors += 1;
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

        process_codex_line(line, line_no, context, state);
    }
}

fn process_codex_line(
    line: &str,
    line_no: usize,
    context: &CodexParseContext<'_>,
    state: &mut CodexParseState,
) {
    let raw_entry: RawJsonEntry<'_> = match serde_json::from_str(line) {
        Ok(entry) => entry,
        Err(err) => {
            if context.debug {
                eprintln!(
                    "Invalid JSON at {}:{}: {}",
                    context.path.display(),
                    line_no,
                    err
                );
            }
            state.parse_errors += 1;
            return;
        }
    };

    match raw_entry.entry_type {
        Some("session_meta") => update_logical_session_key(raw_entry.payload.as_ref(), state),
        Some("turn_context") => update_current_model(raw_entry.payload.as_ref(), state),
        Some("event_msg") => process_event_message(&raw_entry, line_no, context, state),
        _ => {}
    }
}

fn update_logical_session_key(payload: Option<&Payload<'_>>, state: &mut CodexParseState) {
    if let Some(payload) = payload
        && let Some(id) = non_empty_model(payload.id)
    {
        state.logical_session_key = format!("codex-session:{id}");
    }
}

fn update_current_model(payload: Option<&Payload<'_>>, state: &mut CodexParseState) {
    if let Some(payload) = payload
        && let Some(model) = extract_model_ref(payload)
    {
        state.current_model = Some(model.to_string());
    }
}

fn process_event_message(
    raw_entry: &RawJsonEntry<'_>,
    line_no: usize,
    context: &CodexParseContext<'_>,
    state: &mut CodexParseState,
) {
    let Some(payload) = &raw_entry.payload else {
        return;
    };
    let Some("token_count") = payload.payload_type else {
        return;
    };
    let Some(timestamp) = raw_entry.timestamp else {
        return;
    };
    let Some(info) = &payload.info else { return };
    let Some((total, delta)) = next_usage_delta(info, state) else {
        return;
    };
    let Some(utc_dt) = parse_entry_timestamp(timestamp, line_no, context, state) else {
        return;
    };

    let model = resolve_entry_model(payload, state);
    push_codex_entry(timestamp, utc_dt, total, delta, model, context, state);
}

fn next_usage_delta(
    info: &TokenInfo<'_>,
    state: &mut CodexParseState,
) -> Option<(UsageTotals, UsageTotals)> {
    let total = UsageTotals::from_usage(info.total_token_usage.as_ref()?);

    // Skip only when the complete normalized cumulative usage vector is unchanged.
    if let Some(prev) = &state.previous_totals
        && total.is_duplicate_of(prev)
    {
        return None;
    }

    // Use last_token_usage if available, otherwise compute delta.
    let delta = if let Some(last) = &info.last_token_usage {
        UsageTotals::from_usage(last)
    } else {
        state
            .previous_totals
            .map_or(total, |prev| total.subtract(prev))
    };

    state.previous_totals = Some(total);
    if delta.is_empty() {
        return None;
    }

    Some((total, delta))
}

fn parse_entry_timestamp(
    timestamp: &str,
    line_no: usize,
    context: &CodexParseContext<'_>,
    state: &mut CodexParseState,
) -> Option<DateTime<Utc>> {
    match timestamp.parse::<DateTime<Utc>>() {
        Ok(dt) => Some(dt),
        Err(err) => {
            if context.debug {
                eprintln!(
                    "Invalid timestamp at {}:{}: {} ({})",
                    context.path.display(),
                    line_no,
                    timestamp,
                    err
                );
            }
            state.parse_errors += 1;
            None
        }
    }
}

fn resolve_entry_model(payload: &Payload<'_>, state: &mut CodexParseState) -> String {
    if let Some(parsed_model) = extract_model_ref(payload) {
        let parsed_model = parsed_model.to_string();
        state.current_model = Some(parsed_model.clone());
        parsed_model
    } else {
        state
            .current_model
            .clone()
            .unwrap_or_else(|| "gpt-5".to_string())
    }
}

fn push_codex_entry(
    timestamp: &str,
    utc_dt: DateTime<Utc>,
    total: UsageTotals,
    delta: UsageTotals,
    model: String,
    context: &CodexParseContext<'_>,
    state: &mut CodexParseState,
) {
    let local_dt = context.timezone.to_fixed_offset(utc_dt);
    let date = local_dt.date_naive();
    let (input_tokens, output_tokens, cache_read, reasoning_tokens) = split_codex_usage(delta);
    let message_id = usage_message_id(&model, &state.logical_session_key, total, delta);

    state.entries.push(RawEntry {
        timestamp: timestamp.to_string(),
        timestamp_ms: utc_dt.timestamp_millis(),
        date_str: date.format(DATE_FORMAT).to_string(),
        message_id: Some(message_id),
        session_key: context.identity.session_key.clone(),
        session_id: context.identity.session_id.clone(),
        project_path: String::new(), // Codex doesn't track projects
        model,
        input_tokens,
        output_tokens,
        cache_creation: 0, // Codex doesn't have cache creation
        cache_creation_1h: 0,
        cache_read,
        reasoning_tokens,
        stop_reason: Some("complete".to_string()), // Codex events are always complete
        cost_kind: crate::core::CostKind::Real,
        endpoint: crate::core::Endpoint::Unknown,
    });
}

fn split_codex_usage(delta: UsageTotals) -> (i64, i64, i64, i64) {
    // Codex's input_tokens includes cached_input_tokens.
    let input_tokens = (delta.input_tokens - delta.cached_input_tokens).max(0);

    // OpenAI's output_tokens includes reasoning_output_tokens as a subset.
    // Separate them so total_tokens() and calculate_cost() don't double-count.
    let output_tokens = (delta.output_tokens - delta.reasoning_output_tokens).max(0);

    (
        input_tokens,
        output_tokens,
        delta.cached_input_tokens,
        delta.reasoning_output_tokens,
    )
}

#[cfg(test)]
#[path = "parser_tests.rs"]
mod parser_tests;
