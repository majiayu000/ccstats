//! Goose per-call usage ledger source.

use std::env;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use rusqlite::{Connection, OpenFlags};

use crate::consts::{DATE_FORMAT, UNKNOWN};
use crate::core::{CostKind, Endpoint, RawEntry};
use crate::source::{Capabilities, ParseOutput, Source};
use crate::utils::Timezone;

const GOOSE_PATH_ROOT_ENV: &str = "GOOSE_PATH_ROOT";
const XDG_DATA_HOME_ENV: &str = "XDG_DATA_HOME";

pub(crate) struct GooseSource;

impl GooseSource {
    pub(crate) const fn new() -> Self {
        Self
    }
}

impl Source for GooseSource {
    fn name(&self) -> &'static str {
        "goose"
    }

    fn display_name(&self) -> &'static str {
        "Goose"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &[]
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
        goose_database()
            .filter(|path| path.is_file())
            .into_iter()
            .collect()
    }

    fn parse_file(&self, path: &Path, timezone: Timezone, debug: bool) -> ParseOutput {
        parse_goose_database(path, timezone, debug)
    }
}

fn non_empty_env(name: &str) -> Option<PathBuf> {
    env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

#[cfg(not(target_os = "windows"))]
fn default_goose_data_dir() -> Option<PathBuf> {
    non_empty_env(XDG_DATA_HOME_ENV)
        .filter(|root| root.is_absolute())
        .or_else(|| dirs::home_dir().map(|home| home.join(".local/share")))
        .map(|root| root.join("goose"))
}

#[cfg(target_os = "windows")]
fn default_goose_data_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|root| root.join("Block/goose/data"))
}

fn goose_database() -> Option<PathBuf> {
    let data_dir = match non_empty_env(GOOSE_PATH_ROOT_ENV) {
        Some(root) if root.is_absolute() => Some(root.join("data")),
        Some(_) | None => default_goose_data_dir(),
    }?;
    Some(data_dir.join("sessions/sessions.db"))
}

#[derive(Debug)]
struct GooseUsageRow {
    id: i64,
    session_id: String,
    created_timestamp: i64,
    model: Option<String>,
    project_path: String,
    input: i64,
    output: i64,
    total: i64,
    cache_read: i64,
    cache_write: i64,
    cost: Option<f64>,
    cost_source: Option<String>,
    is_compaction: bool,
}

fn goose_entry(row: GooseUsageRow, timezone: Timezone) -> Result<Option<RawEntry>, &'static str> {
    if [
        row.input,
        row.output,
        row.total,
        row.cache_read,
        row.cache_write,
    ]
    .into_iter()
    .any(|value| value < 0)
    {
        return Err("negative token count");
    }
    if row.cache_read.saturating_add(row.cache_write) > row.input {
        return Err("cache tokens exceed total input");
    }
    if row.total > 0 && row.input == 0 && row.output == 0 {
        return Err("total tokens cannot be allocated to input or output");
    }
    if row.cost.is_some_and(|cost| !cost.is_finite() || cost < 0.0) {
        return Err("invalid cost");
    }
    let recorded_cost_usd = match row.cost_source.as_deref() {
        Some("provider_reported") => row.cost.filter(|cost| *cost > 0.0),
        Some("estimated" | "carried_forward") | None => None,
        Some(_) => return Err("unknown cost source"),
    };
    if row.input == 0 && row.output == 0 && recorded_cost_usd.is_none() {
        return Ok(None);
    }

    let timestamp_ms = row
        .created_timestamp
        .checked_mul(1_000)
        .filter(|timestamp| *timestamp > 0)
        .ok_or("invalid created timestamp")?;
    let timestamp = DateTime::<Utc>::from_timestamp_millis(timestamp_ms)
        .ok_or("created timestamp is outside supported range")?;
    let model = row
        .model
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(UNKNOWN)
        .to_string();

    Ok(Some(RawEntry {
        timestamp: timestamp.to_rfc3339(),
        timestamp_ms,
        date_str: timezone
            .to_fixed_offset(timestamp)
            .date_naive()
            .format(DATE_FORMAT)
            .to_string(),
        message_id: Some(format!("goose:{}:{}", row.session_id, row.id)),
        session_key: format!("goose::{}", row.session_id),
        session_id: row.session_id,
        project_path: row.project_path,
        model,
        input_tokens: row.input - row.cache_read - row.cache_write,
        output_tokens: row.output,
        cache_creation: row.cache_write,
        cache_creation_1h: 0,
        cache_read: row.cache_read,
        reasoning_tokens: 0,
        stop_reason: Some(if row.is_compaction {
            "compaction".to_string()
        } else {
            "completed".to_string()
        }),
        cost_kind: CostKind::Real,
        endpoint: Endpoint::Unknown,
        call_count: 1,
        reported_total_tokens: None,
        recorded_cost_usd,
    }))
}

fn parse_goose_database(path: &Path, timezone: Timezone, debug: bool) -> ParseOutput {
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let connection = match Connection::open_with_flags(path, flags) {
        Ok(connection) => connection,
        Err(error) => {
            if debug {
                eprintln!("Failed to open Goose database {}: {error}", path.display());
            }
            return ParseOutput {
                entries: Vec::new(),
                errors: 1,
            };
        }
    };
    let query = "
        SELECT u.id, u.session_id, u.created_timestamp,
               COALESCE(
                   NULLIF(TRIM(u.model), ''),
                   CASE WHEN json_valid(s.model_config_json)
                        THEN NULLIF(TRIM(json_extract(s.model_config_json, '$.model_name')), '')
                   END
               ),
               s.working_dir,
               COALESCE(u.input_tokens, 0), COALESCE(u.output_tokens, 0),
               COALESCE(u.total_tokens, 0), COALESCE(u.cache_read_tokens, 0),
               COALESCE(u.cache_write_tokens, 0), u.cost, u.cost_source,
               COALESCE(u.is_compaction, 0)
        FROM usage_ledger u
        JOIN sessions s ON s.id = u.session_id
        ORDER BY u.id
    ";
    let mut statement = match connection.prepare(query) {
        Ok(statement) => statement,
        Err(error) => {
            if debug {
                eprintln!("Failed to query Goose database {}: {error}", path.display());
            }
            return ParseOutput {
                entries: Vec::new(),
                errors: 1,
            };
        }
    };
    let rows = match statement.query_map([], |row| {
        Ok(GooseUsageRow {
            id: row.get(0)?,
            session_id: row.get(1)?,
            created_timestamp: row.get(2)?,
            model: row.get(3)?,
            project_path: row.get(4)?,
            input: row.get(5)?,
            output: row.get(6)?,
            total: row.get(7)?,
            cache_read: row.get(8)?,
            cache_write: row.get(9)?,
            cost: row.get(10)?,
            cost_source: row.get(11)?,
            is_compaction: row.get::<_, i64>(12)? != 0,
        })
    }) {
        Ok(rows) => rows,
        Err(error) => {
            if debug {
                eprintln!("Failed to read Goose database {}: {error}", path.display());
            }
            return ParseOutput {
                entries: Vec::new(),
                errors: 1,
            };
        }
    };

    let mut output = ParseOutput::default();
    for row in rows {
        match row {
            Ok(row) => match goose_entry(row, timezone) {
                Ok(Some(entry)) => output.entries.push(entry),
                Ok(None) => {}
                Err(error) => {
                    output.errors += 1;
                    if debug {
                        eprintln!("Invalid Goose usage in {}: {error}", path.display());
                    }
                }
            },
            Err(error) => {
                output.errors += 1;
                if debug {
                    eprintln!("Invalid Goose row in {}: {error}", path.display());
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
    fn ledger_input_is_normalized_and_only_provider_cost_is_preserved() {
        let row = GooseUsageRow {
            id: 1,
            session_id: "session-1".to_string(),
            created_timestamp: 1_788_145_445,
            model: Some("claude-sonnet-4".to_string()),
            project_path: "/tmp/project".to_string(),
            input: 100,
            output: 20,
            total: 120,
            cache_read: 30,
            cache_write: 10,
            cost: Some(0.25),
            cost_source: Some("provider_reported".to_string()),
            is_compaction: false,
        };
        let entry = goose_entry(row, Timezone::Named(chrono_tz::UTC))
            .unwrap()
            .unwrap();
        assert_eq!(entry.input_tokens, 60);
        assert_eq!(entry.cache_read, 30);
        assert_eq!(entry.cache_creation, 10);
        assert_eq!(entry.recorded_cost_usd, Some(0.25));
    }

    #[test]
    fn cache_breakdown_cannot_exceed_inclusive_input() {
        let row = GooseUsageRow {
            id: 1,
            session_id: "session-1".to_string(),
            created_timestamp: 1_788_145_445,
            model: None,
            project_path: String::new(),
            input: 10,
            output: 1,
            total: 11,
            cache_read: 8,
            cache_write: 3,
            cost: None,
            cost_source: None,
            is_compaction: false,
        };
        assert!(matches!(
            goose_entry(row, Timezone::Named(chrono_tz::UTC)),
            Err("cache tokens exceed total input")
        ));
    }

    #[test]
    fn unknown_cost_provenance_is_an_error() {
        let row = GooseUsageRow {
            id: 1,
            session_id: "session-1".to_string(),
            created_timestamp: 1_788_145_445,
            model: None,
            project_path: String::new(),
            input: 10,
            output: 1,
            total: 11,
            cache_read: 0,
            cache_write: 0,
            cost: Some(0.1),
            cost_source: Some("future_source".to_string()),
            is_compaction: false,
        };
        assert!(matches!(
            goose_entry(row, Timezone::Named(chrono_tz::UTC)),
            Err("unknown cost source")
        ));
    }
}
