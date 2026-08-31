//! Unsloth Studio's durable inference receipts.

use std::collections::{HashMap, HashSet};
use std::env;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use rusqlite::{Connection, OpenFlags};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::consts::{DATE_FORMAT, UNKNOWN};
use crate::core::{CostKind, Endpoint, RawEntry, source_wide_message_id};
use crate::utils::Timezone;

use super::{Capabilities, ParseOutput, Source};

const MAX_TOKEN_COUNT: i64 = 1_i64 << 40;

pub(crate) struct UnslothSource;

impl UnslothSource {
    pub(crate) const fn new() -> Self {
        Self
    }
}

impl Source for UnslothSource {
    fn name(&self) -> &'static str {
        "unsloth"
    }

    fn display_name(&self) -> &'static str {
        "Unsloth Studio"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            has_projects: true,
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
        studio_root()
            .map(|root| root.join("studio.db"))
            .filter(|path| path.is_file())
            .into_iter()
            .collect()
    }

    fn parse_file(&self, path: &Path, timezone: Timezone, debug: bool) -> ParseOutput {
        parse_database(path, timezone, debug)
    }
}

fn configured_root(name: &str) -> Option<PathBuf> {
    let value = env::var(name).ok()?;
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

fn studio_root() -> Option<PathBuf> {
    configured_root("UNSLOTH_STUDIO_HOME")
        .or_else(|| configured_root("STUDIO_HOME"))
        .or_else(|| dirs::home_dir().map(|home| home.join(".unsloth/studio")))
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct ForkKey {
    source_thread: String,
    created_at: i64,
    role: String,
}

struct ChatRow {
    id: String,
    thread_id: String,
    role: String,
    metadata_json: Option<String>,
    created_at: i64,
    pair_id: Option<String>,
    project_root: Option<String>,
    thread_created_at: i64,
    forked_from_thread_id: Option<String>,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Metadata {
    context_usage: Option<ContextUsage>,
    server_timings: Option<ServerTimings>,
    timing: Option<Timing>,
    response_details: Option<ResponseDetails>,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ContextUsage {
    prompt_tokens: Option<i64>,
    completion_tokens: Option<i64>,
    total_tokens: Option<i64>,
    cached_tokens: Option<i64>,
    cache_write_tokens: Option<i64>,
    model_id: Option<String>,
}

#[derive(Default, Deserialize)]
struct ServerTimings {
    #[serde(rename = "prompt_n")]
    prompt: Option<i64>,
    #[serde(rename = "predicted_n")]
    predicted: Option<i64>,
    #[serde(rename = "cache_n")]
    cache: Option<i64>,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Timing {
    token_count: Option<i64>,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResponseDetails {
    response_model_id: Option<String>,
}

#[derive(Clone, Copy)]
struct TokenRecord {
    input: i64,
    output: i64,
    cache_read: i64,
    cache_write: i64,
    total: i64,
}

fn non_blank(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let value = value.trim();
        (!value.is_empty()).then(|| value.to_string())
    })
}

fn checked_counter(value: i64) -> Result<i64, &'static str> {
    if (0..=MAX_TOKEN_COUNT).contains(&value) {
        Ok(value)
    } else {
        Err("invalid token counter")
    }
}

fn preferred(primary: Option<i64>, fallback: Option<i64>) -> Result<i64, &'static str> {
    if let Some(value) = primary {
        if value < 0 {
            return Err("negative token counter");
        }
        if value > 0 {
            return checked_counter(value);
        }
    }
    match fallback {
        Some(value) => checked_counter(value),
        None => Ok(0),
    }
}

fn chat_tokens(metadata: &Metadata) -> Result<Option<TokenRecord>, &'static str> {
    let usage = metadata.context_usage.as_ref();
    let server = metadata.server_timings.as_ref();
    let prompt = preferred(
        usage.and_then(|usage| usage.prompt_tokens),
        server.and_then(|server| server.prompt),
    )?;
    let mut completion = preferred(
        usage.and_then(|usage| usage.completion_tokens),
        server.and_then(|server| server.predicted),
    )?;
    if completion == 0 {
        completion = preferred(
            None,
            metadata
                .timing
                .as_ref()
                .and_then(|timing| timing.token_count),
        )?;
    }
    let cache_read = preferred(
        usage.and_then(|usage| usage.cached_tokens),
        server.and_then(|server| server.cache),
    )?;
    let cache_write = checked_counter(
        usage
            .and_then(|usage| usage.cache_write_tokens)
            .unwrap_or(0),
    )?;
    if cache_read
        .checked_add(cache_write)
        .ok_or("token overflow")?
        > prompt
    {
        return Err("cache tokens exceed prompt tokens");
    }
    let component_total = prompt.checked_add(completion).ok_or("token overflow")?;
    let total = match usage.and_then(|usage| usage.total_tokens) {
        Some(value) if value < 0 => return Err("negative token counter"),
        Some(value) if value > 0 => checked_counter(value)?,
        _ => component_total,
    };
    if total == 0 {
        return Ok(None);
    }
    Ok(Some(TokenRecord {
        input: prompt - cache_read - cache_write,
        output: completion,
        cache_read,
        cache_write,
        total,
    }))
}

fn timestamp(value: i64) -> Result<DateTime<Utc>, &'static str> {
    if value <= 0 {
        return Err("invalid timestamp");
    }
    DateTime::<Utc>::from_timestamp_millis(value).ok_or("timestamp outside supported range")
}

struct EntryMeta {
    message_id: String,
    session_id: String,
    project_path: String,
    model: String,
    stop_reason: String,
}

fn entry(
    timestamp: DateTime<Utc>,
    timezone: Timezone,
    meta: EntryMeta,
    tokens: TokenRecord,
) -> RawEntry {
    RawEntry {
        timestamp: timestamp.to_rfc3339(),
        timestamp_ms: timestamp.timestamp_millis(),
        date_str: timezone
            .to_fixed_offset(timestamp)
            .date_naive()
            .format(DATE_FORMAT)
            .to_string(),
        message_id: Some(meta.message_id),
        session_key: format!("unsloth::{}", meta.session_id),
        session_id: meta.session_id,
        project_path: meta.project_path,
        model: meta.model,
        input_tokens: tokens.input,
        output_tokens: tokens.output,
        cache_creation: tokens.cache_write,
        cache_creation_1h: 0,
        cache_read: tokens.cache_read,
        reasoning_tokens: 0,
        reported_total_tokens: Some(tokens.total),
        stop_reason: Some(meta.stop_reason),
        cost_kind: CostKind::Real,
        endpoint: Endpoint::Unknown,
        call_count: 1,
        recorded_cost_usd: None,
    }
}

fn read_chat_rows(connection: &Connection) -> rusqlite::Result<(Vec<ChatRow>, usize)> {
    let mut statement = connection.prepare(
        r"
        SELECT m.id, m.thread_id, m.role, m.metadata_json, m.created_at,
               t.pair_id, p.root_path, t.created_at, t.forked_from_thread_id
        FROM chat_messages m
        JOIN chat_threads t ON t.id = m.thread_id
        LEFT JOIN chat_projects p ON p.id = t.project_id
        ORDER BY m.thread_id, m.created_at, m.id
        ",
    )?;
    let rows = statement.query_map([], |row| {
        Ok(ChatRow {
            id: row.get(0)?,
            thread_id: row.get(1)?,
            role: row.get(2)?,
            metadata_json: row.get(3)?,
            created_at: row.get(4)?,
            pair_id: row.get(5)?,
            project_root: row.get(6)?,
            thread_created_at: row.get(7)?,
            forked_from_thread_id: row.get(8)?,
        })
    })?;
    let mut decoded = Vec::new();
    let mut errors = 0;
    for row in rows {
        match row {
            Ok(row) => decoded.push(row),
            Err(_) => errors += 1,
        }
    }
    Ok((decoded, errors))
}

fn fork_indexes(rows: &[ChatRow]) -> (HashMap<ForkKey, String>, HashSet<ForkKey>) {
    let referenced = rows
        .iter()
        .filter_map(|row| row.forked_from_thread_id.clone())
        .collect::<HashSet<_>>();
    let surviving = rows
        .iter()
        .filter(|row| referenced.contains(&row.thread_id))
        .map(|row| ForkKey {
            source_thread: row.thread_id.clone(),
            created_at: row.created_at,
            role: row.role.clone(),
        })
        .collect();
    let mut keepers = HashMap::<ForkKey, String>::new();
    for row in rows {
        let Some(source_thread) = row.forked_from_thread_id.as_ref() else {
            continue;
        };
        if row.created_at >= row.thread_created_at {
            continue;
        }
        let key = ForkKey {
            source_thread: source_thread.clone(),
            created_at: row.created_at,
            role: row.role.clone(),
        };
        keepers
            .entry(key)
            .and_modify(|keeper| {
                if row.thread_id < *keeper {
                    keeper.clone_from(&row.thread_id);
                }
            })
            .or_insert_with(|| row.thread_id.clone());
    }
    (keepers, surviving)
}

fn parse_chat(rows: Vec<ChatRow>, timezone: Timezone, output: &mut ParseOutput) {
    let (keepers, surviving) = fork_indexes(&rows);
    for row in rows {
        if let Some(source_thread) = row.forked_from_thread_id.as_ref()
            && row.created_at < row.thread_created_at
        {
            let key = ForkKey {
                source_thread: source_thread.clone(),
                created_at: row.created_at,
                role: row.role.clone(),
            };
            if surviving.contains(&key) || keepers.get(&key) != Some(&row.thread_id) {
                continue;
            }
        }
        if row.role != "assistant" {
            continue;
        }
        let Some(raw_metadata) = row.metadata_json.as_deref() else {
            continue;
        };
        let Ok(metadata) = serde_json::from_str::<Metadata>(raw_metadata) else {
            output.errors += 1;
            continue;
        };
        let tokens = match chat_tokens(&metadata) {
            Ok(Some(tokens)) => tokens,
            Ok(None) => continue,
            Err(_) => {
                output.errors += 1;
                continue;
            }
        };
        let Ok(timestamp) = timestamp(row.created_at) else {
            output.errors += 1;
            continue;
        };
        let model = non_blank(
            metadata
                .response_details
                .and_then(|details| details.response_model_id),
        )
        .or_else(|| non_blank(metadata.context_usage.and_then(|usage| usage.model_id)))
        .unwrap_or_else(|| UNKNOWN.to_string());
        let session_id = non_blank(row.pair_id).unwrap_or(row.thread_id);
        output.entries.push(entry(
            timestamp,
            timezone,
            EntryMeta {
                message_id: source_wide_message_id("unsloth", &format!("chat:{}", row.id)),
                session_id,
                project_path: non_blank(row.project_root).unwrap_or_else(|| UNKNOWN.to_string()),
                model,
                stop_reason: "complete".to_string(),
            },
            tokens,
        ));
    }
}

fn subject_session(subject: &str) -> String {
    let digest = Sha256::digest(subject.as_bytes());
    let mut suffix = String::with_capacity(16);
    for byte in &digest[..8] {
        write!(&mut suffix, "{byte:02x}").expect("writing to String cannot fail");
    }
    format!("api:{suffix}")
}

fn parse_api(connection: &Connection, timezone: Timezone, output: &mut ParseOutput) {
    let Ok(mut statement) = connection.prepare(
        r"
        SELECT id, subject, model, status, prompt_tokens,
               completion_tokens, total_tokens, created_at
        FROM api_usage_events
        ORDER BY created_at, id
        ",
    ) else {
        output.errors += 1;
        return;
    };
    let Ok(rows) = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, i64>(4)?,
            row.get::<_, i64>(5)?,
            row.get::<_, i64>(6)?,
            row.get::<_, i64>(7)?,
        ))
    }) else {
        output.errors += 1;
        return;
    };
    for row in rows {
        let Ok((id, subject, model, status, prompt, completion, total, created_at)) = row else {
            output.errors += 1;
            continue;
        };
        let parsed = (|| {
            if id.trim().is_empty() || subject.trim().is_empty() || model.trim().is_empty() {
                return Err("empty API identity");
            }
            let prompt = checked_counter(prompt)?;
            let completion = checked_counter(completion)?;
            let total = checked_counter(total)?;
            if prompt == 0 && completion == 0 && total == 0 {
                return Ok(None);
            }
            let timestamp = timestamp(created_at)?;
            Ok(Some(entry(
                timestamp,
                timezone,
                EntryMeta {
                    message_id: source_wide_message_id("unsloth", &format!("api:{id}")),
                    session_id: subject_session(&subject),
                    project_path: UNKNOWN.to_string(),
                    model: model.trim().to_string(),
                    stop_reason: non_blank(Some(status)).unwrap_or_else(|| "terminal".to_string()),
                },
                TokenRecord {
                    input: prompt,
                    output: completion,
                    cache_read: 0,
                    cache_write: 0,
                    total,
                },
            )))
        })();
        match parsed {
            Ok(Some(parsed)) => output.entries.push(parsed),
            Ok(None) => {}
            Err(_) => output.errors += 1,
        }
    }
}

fn parse_database(path: &Path, timezone: Timezone, debug: bool) -> ParseOutput {
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let connection = match Connection::open_with_flags(path, flags) {
        Ok(connection) => connection,
        Err(error) => {
            if debug {
                eprintln!(
                    "Failed to open Unsloth database {}: {error}",
                    path.display()
                );
            }
            return ParseOutput {
                entries: Vec::new(),
                errors: 1,
            };
        }
    };
    if let Err(error) = connection.execute_batch("BEGIN") {
        if debug {
            eprintln!(
                "Failed to snapshot Unsloth database {}: {error}",
                path.display()
            );
        }
        return ParseOutput {
            entries: Vec::new(),
            errors: 1,
        };
    }
    let mut output = ParseOutput::default();
    match read_chat_rows(&connection) {
        Ok((rows, errors)) => {
            output.errors += errors;
            parse_chat(rows, timezone, &mut output);
        }
        Err(error) => {
            if debug {
                eprintln!(
                    "Failed to query Unsloth chat usage {}: {error}",
                    path.display()
                );
            }
            output.errors += 1;
        }
    }
    parse_api(&connection, timezone, &mut output);
    if connection.execute_batch("COMMIT").is_err() {
        output.entries.clear();
        output.errors += 1;
    }
    output
}
