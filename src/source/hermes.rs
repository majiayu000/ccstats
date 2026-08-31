//! Hermes Agent current `SQLite` usage ledger source.

use std::collections::HashMap;
use std::env;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use rusqlite::{Connection, OpenFlags};

use crate::consts::{DATE_FORMAT, UNKNOWN};
use crate::core::{CostKind, Endpoint, RawEntry, source_wide_message_id};
use crate::source::{Capabilities, ParseOutput, Source};
use crate::utils::Timezone;

const HERMES_HOME_ENV: &str = "HERMES_HOME";

pub(crate) struct HermesSource;

impl HermesSource {
    pub(crate) const fn new() -> Self {
        Self
    }
}

impl Source for HermesSource {
    fn name(&self) -> &'static str {
        "hermes"
    }

    fn display_name(&self) -> &'static str {
        "Hermes Agent"
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
        hermes_database()
            .filter(|path| path.is_file())
            .into_iter()
            .collect()
    }

    fn parse_file(&self, path: &Path, timezone: Timezone, debug: bool) -> ParseOutput {
        parse_database(path, timezone, debug)
    }
}

fn hermes_database() -> Option<PathBuf> {
    env::var_os(HERMES_HOME_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".hermes")))
        .map(|home| home.join("state.db"))
}

struct HermesRow {
    session_id: String,
    model: String,
    provider: String,
    base_url: String,
    billing_mode: String,
    task: String,
    call_count: i64,
    input: i64,
    output: i64,
    cache_read: i64,
    cache_write: i64,
    reasoning: i64,
    estimated_cost: f64,
    actual_cost: f64,
    cost_status: Option<String>,
    timestamp_seconds: f64,
    project_path: Option<String>,
}

#[derive(Clone, Default)]
struct UsageTotals {
    call_count: i64,
    input: i64,
    output: i64,
    cache_read: i64,
    cache_write: i64,
    reasoning: i64,
    estimated_cost: f64,
    actual_cost: f64,
}

impl UsageTotals {
    fn add(&mut self, row: &HermesRow) -> Result<(), &'static str> {
        let values = [
            (&mut self.call_count, row.call_count),
            (&mut self.input, row.input),
            (&mut self.output, row.output),
            (&mut self.cache_read, row.cache_read),
            (&mut self.cache_write, row.cache_write),
            (&mut self.reasoning, row.reasoning),
        ];
        for (total, value) in values {
            if value < 0 {
                return Err("negative usage value");
            }
            *total = total.checked_add(value).ok_or("usage total overflow")?;
        }
        if row.estimated_cost < 0.0 || row.actual_cost < 0.0 {
            return Err("negative cost");
        }
        self.estimated_cost += row.estimated_cost;
        self.actual_cost += row.actual_cost;
        if !self.estimated_cost.is_finite() || !self.actual_cost.is_finite() {
            return Err("cost total overflow");
        }
        Ok(())
    }
}

fn entry_from_row(row: &HermesRow, timezone: Timezone) -> Result<Option<RawEntry>, &'static str> {
    if row.model.trim().is_empty() {
        return Err("empty model");
    }
    if [
        row.call_count,
        row.input,
        row.output,
        row.cache_read,
        row.cache_write,
        row.reasoning,
    ]
    .into_iter()
    .any(|value| value < 0)
    {
        return Err("negative usage value");
    }
    if row.reasoning > row.output {
        return Err("reasoning tokens exceed output tokens");
    }
    if !row.estimated_cost.is_finite()
        || row.estimated_cost < 0.0
        || !row.actual_cost.is_finite()
        || row.actual_cost < 0.0
    {
        return Err("invalid cost");
    }
    let recorded_cost_usd = match row.cost_status.as_deref() {
        Some("actual") => Some(row.actual_cost),
        Some("included") => Some(0.0),
        Some("estimated" | "unknown") | None => None,
        Some(_) => return Err("unknown cost status"),
    };
    if row.call_count == 0
        && row.input == 0
        && row.output == 0
        && row.cache_read == 0
        && row.cache_write == 0
        && row.reasoning == 0
        && recorded_cost_usd.is_none()
    {
        return Ok(None);
    }
    let timestamp_ms = (row.timestamp_seconds * 1_000.0).round();
    if !timestamp_ms.is_finite() || timestamp_ms <= 0.0 || timestamp_ms > i64::MAX as f64 {
        return Err("invalid timestamp");
    }
    let timestamp = DateTime::<Utc>::from_timestamp_millis(timestamp_ms as i64)
        .ok_or("timestamp outside supported range")?;
    let identity = format!(
        "{}:{}:{}:{}:{}:{}",
        row.session_id, row.model, row.provider, row.base_url, row.billing_mode, row.task
    );

    Ok(Some(RawEntry {
        timestamp: timestamp.to_rfc3339(),
        timestamp_ms: timestamp.timestamp_millis(),
        date_str: timezone
            .to_fixed_offset(timestamp)
            .date_naive()
            .format(DATE_FORMAT)
            .to_string(),
        message_id: Some(source_wide_message_id("hermes", &identity)),
        session_key: format!("hermes::{}", row.session_id),
        session_id: row.session_id.clone(),
        project_path: row
            .project_path
            .clone()
            .unwrap_or_else(|| UNKNOWN.to_string()),
        model: row.model.clone(),
        input_tokens: row.input,
        output_tokens: row.output - row.reasoning,
        cache_creation: row.cache_write,
        cache_creation_1h: 0,
        cache_read: row.cache_read,
        reasoning_tokens: row.reasoning,
        stop_reason: Some("aggregate".to_string()),
        cost_kind: CostKind::Real,
        endpoint: Endpoint::Unknown,
        call_count: row.call_count,
        reported_total_tokens: None,
        recorded_cost_usd,
    }))
}

fn parse_database(path: &Path, timezone: Timezone, debug: bool) -> ParseOutput {
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let connection = match Connection::open_with_flags(path, flags) {
        Ok(connection) => connection,
        Err(error) => {
            if debug {
                eprintln!("Failed to open Hermes database {}: {error}", path.display());
            }
            return ParseOutput {
                entries: Vec::new(),
                errors: 1,
            };
        }
    };
    let query = r"
        SELECT u.session_id, u.model, u.billing_provider, u.billing_base_url,
               u.billing_mode, u.task, u.api_call_count, u.input_tokens,
               u.output_tokens, u.cache_read_tokens, u.cache_write_tokens,
               u.reasoning_tokens, u.estimated_cost_usd, u.actual_cost_usd,
               u.cost_status, COALESCE(u.first_seen, s.started_at), s.cwd
        FROM session_model_usage u
        JOIN sessions s ON s.id = u.session_id
        ORDER BY u.session_id, u.model, u.billing_provider, u.billing_base_url,
                 u.billing_mode, u.task
    ";
    let mut statement = match connection.prepare(query) {
        Ok(statement) => statement,
        Err(error) => {
            if debug {
                eprintln!(
                    "Failed to query current Hermes schema {}: {error}",
                    path.display()
                );
            }
            return ParseOutput {
                entries: Vec::new(),
                errors: 1,
            };
        }
    };
    let rows = match statement.query_map([], |row| {
        Ok(HermesRow {
            session_id: row.get(0)?,
            model: row.get(1)?,
            provider: row.get(2)?,
            base_url: row.get(3)?,
            billing_mode: row.get(4)?,
            task: row.get(5)?,
            call_count: row.get(6)?,
            input: row.get(7)?,
            output: row.get(8)?,
            cache_read: row.get(9)?,
            cache_write: row.get(10)?,
            reasoning: row.get(11)?,
            estimated_cost: row.get(12)?,
            actual_cost: row.get(13)?,
            cost_status: row.get(14)?,
            timestamp_seconds: row.get(15)?,
            project_path: row.get(16)?,
        })
    }) {
        Ok(rows) => rows,
        Err(error) => {
            if debug {
                eprintln!("Failed to read Hermes rows {}: {error}", path.display());
            }
            return ParseOutput {
                entries: Vec::new(),
                errors: 1,
            };
        }
    };
    let mut output = ParseOutput::default();
    let mut totals = HashMap::<String, UsageTotals>::new();
    for row in rows {
        match row {
            Ok(row) => match entry_from_row(&row, timezone) {
                Ok(Some(entry)) => {
                    let subtotal = totals.entry(row.session_id.clone()).or_default();
                    let mut updated = subtotal.clone();
                    if updated.add(&row).is_err() {
                        output.errors += 1;
                        continue;
                    }
                    *subtotal = updated;
                    output.entries.push(entry);
                }
                Ok(None) => {}
                Err(_) => output.errors += 1,
            },
            Err(_) => output.errors += 1,
        }
    }
    drop(statement);
    append_session_residuals(&connection, &totals, timezone, debug, path, &mut output);
    output
}

fn append_session_residuals(
    connection: &Connection,
    totals: &HashMap<String, UsageTotals>,
    timezone: Timezone,
    debug: bool,
    path: &Path,
    output: &mut ParseOutput,
) {
    let query = r"
        SELECT id, model, COALESCE(billing_provider, ''),
               COALESCE(billing_base_url, ''), COALESCE(billing_mode, ''),
               api_call_count, input_tokens, output_tokens, cache_read_tokens,
               cache_write_tokens, reasoning_tokens,
               COALESCE(estimated_cost_usd, 0), COALESCE(actual_cost_usd, 0),
               cost_status, started_at, cwd
        FROM sessions
        ORDER BY id
    ";
    let mut statement = match connection.prepare(query) {
        Ok(statement) => statement,
        Err(error) => {
            if debug {
                eprintln!(
                    "Failed to query Hermes session totals {}: {error}",
                    path.display()
                );
            }
            output.errors += 1;
            return;
        }
    };
    let rows = match statement.query_map([], |row| {
        Ok(HermesRow {
            session_id: row.get(0)?,
            model: row
                .get::<_, Option<String>>(1)?
                .filter(|model| !model.trim().is_empty())
                .unwrap_or_else(|| UNKNOWN.to_string()),
            provider: row.get(2)?,
            base_url: row.get(3)?,
            billing_mode: row.get(4)?,
            task: "<session-residual>".to_string(),
            call_count: row.get(5)?,
            input: row.get(6)?,
            output: row.get(7)?,
            cache_read: row.get(8)?,
            cache_write: row.get(9)?,
            reasoning: row.get(10)?,
            estimated_cost: row.get(11)?,
            actual_cost: row.get(12)?,
            cost_status: row.get(13)?,
            timestamp_seconds: row.get(14)?,
            project_path: row.get(15)?,
        })
    }) {
        Ok(rows) => rows,
        Err(error) => {
            if debug {
                eprintln!(
                    "Failed to read Hermes session totals {}: {error}",
                    path.display()
                );
            }
            output.errors += 1;
            return;
        }
    };
    for row in rows {
        match row {
            Ok(row) => {
                let subtotal = totals.get(&row.session_id);
                match residual_from_session(row, subtotal)
                    .and_then(|row| entry_from_row(&row, timezone))
                {
                    Ok(Some(entry)) => output.entries.push(entry),
                    Ok(None) => {}
                    Err(_) => output.errors += 1,
                }
            }
            Err(_) => output.errors += 1,
        }
    }
}

fn residual_from_session(
    mut row: HermesRow,
    subtotal: Option<&UsageTotals>,
) -> Result<HermesRow, &'static str> {
    if [
        row.call_count,
        row.input,
        row.output,
        row.cache_read,
        row.cache_write,
        row.reasoning,
    ]
    .into_iter()
    .any(|value| value < 0)
        || !row.estimated_cost.is_finite()
        || row.estimated_cost < 0.0
        || !row.actual_cost.is_finite()
        || row.actual_cost < 0.0
    {
        return Err("invalid session aggregate");
    }
    if subtotal.is_some_and(|value| value.output > row.output || value.reasoning > row.reasoning) {
        return Err("session aggregate is below detail subtotal");
    }
    row.call_count = (row.call_count - subtotal.map_or(0, |v| v.call_count)).max(0);
    row.input = (row.input - subtotal.map_or(0, |v| v.input)).max(0);
    if row.reasoning > row.output || subtotal.is_some_and(|value| value.reasoning > value.output) {
        return Err("reasoning tokens exceed output tokens");
    }
    let visible_output = row.output - row.reasoning;
    let subtotal_visible = subtotal.map_or(0, |value| value.output - value.reasoning);
    let residual_visible = (visible_output - subtotal_visible).max(0);
    row.cache_read = (row.cache_read - subtotal.map_or(0, |v| v.cache_read)).max(0);
    row.cache_write = (row.cache_write - subtotal.map_or(0, |v| v.cache_write)).max(0);
    row.reasoning = (row.reasoning - subtotal.map_or(0, |v| v.reasoning)).max(0);
    row.output = residual_visible
        .checked_add(row.reasoning)
        .ok_or("output total overflow")?;
    row.estimated_cost = (row.estimated_cost - subtotal.map_or(0.0, |v| v.estimated_cost)).max(0.0);
    row.actual_cost = (row.actual_cost - subtotal.map_or(0.0, |v| v.actual_cost)).max(0.0);
    Ok(row)
}

#[cfg(test)]
#[path = "hermes_tests.rs"]
mod tests;
