//! `OpenCode` local `SQLite` usage source.

use std::env;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde::Deserialize;

use crate::consts::DATE_FORMAT;
use crate::core::{CostKind, Endpoint, RawEntry, source_wide_message_id};
use crate::source::{Capabilities, ParseOutput, Source};
use crate::utils::Timezone;

use super::opencode_fork::{read_session_creation_times, reconcile_fork_copies};

const OPENCODE_DB_ENV: &str = "OPENCODE_DB";
const MIMOCODE_DB_ENV: &str = "MIMOCODE_DB";
const MIMOCODE_HOME_ENV: &str = "MIMOCODE_HOME";
const KILO_DB_ENV: &str = "KILO_DB";
const XDG_DATA_HOME_ENV: &str = "XDG_DATA_HOME";

pub(crate) struct OpenCodeSource;
pub(crate) struct MiMoCodeSource;
pub(crate) struct KiloCliSource;

impl OpenCodeSource {
    pub(crate) const fn new() -> Self {
        Self
    }
}

impl MiMoCodeSource {
    pub(crate) const fn new() -> Self {
        Self
    }
}

impl KiloCliSource {
    pub(crate) const fn new() -> Self {
        Self
    }
}

fn opencode_capabilities() -> Capabilities {
    Capabilities {
        has_projects: true,
        has_billing_blocks: false,
        has_reasoning_tokens: true,
        has_cache_creation: true,
        has_cache_read: true,
        needs_dedup: true,
        has_tool_calls: false,
        has_endpoints: false,
    }
}

impl Source for OpenCodeSource {
    fn name(&self) -> &'static str {
        "opencode"
    }

    fn display_name(&self) -> &'static str {
        "OpenCode"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["oc"]
    }

    fn capabilities(&self) -> Capabilities {
        opencode_capabilities()
    }

    fn find_files(&self) -> Vec<PathBuf> {
        find_opencode_databases()
    }

    fn parse_file(&self, path: &Path, timezone: Timezone, debug: bool) -> ParseOutput {
        parse_opencode_database(path, timezone, debug, ParseProfile::opencode())
    }
}

impl Source for MiMoCodeSource {
    fn name(&self) -> &'static str {
        "mimocode"
    }

    fn display_name(&self) -> &'static str {
        "MiMo Code"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["micode"]
    }

    fn capabilities(&self) -> Capabilities {
        opencode_capabilities()
    }

    fn find_files(&self) -> Vec<PathBuf> {
        find_mimocode_databases()
    }

    fn parse_file(&self, path: &Path, timezone: Timezone, debug: bool) -> ParseOutput {
        if mimocode_home_is_invalid() {
            if debug {
                eprintln!("MIMOCODE_HOME must be an absolute path");
            }
            return ParseOutput {
                entries: Vec::new(),
                errors: 1,
            };
        }
        parse_opencode_database(path, timezone, debug, ParseProfile::mimocode())
    }
}

impl Source for KiloCliSource {
    fn name(&self) -> &'static str {
        "kilo"
    }

    fn display_name(&self) -> &'static str {
        "Kilo CLI"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["kilo-cli"]
    }

    fn capabilities(&self) -> Capabilities {
        opencode_capabilities()
    }

    fn find_files(&self) -> Vec<PathBuf> {
        find_kilo_databases()
    }

    fn parse_file(&self, path: &Path, timezone: Timezone, debug: bool) -> ParseOutput {
        parse_opencode_database(path, timezone, debug, ParseProfile::kilo())
    }
}

fn opencode_data_dir() -> Option<PathBuf> {
    match env::var_os(XDG_DATA_HOME_ENV) {
        Some(value) if !value.is_empty() => Some(PathBuf::from(value).join("opencode")),
        Some(_) | None => dirs::data_dir().map(|path| path.join("opencode")),
    }
}

fn find_opencode_databases() -> Vec<PathBuf> {
    if let Some(configured) = env::var_os(OPENCODE_DB_ENV).filter(|value| !value.is_empty()) {
        let configured = PathBuf::from(configured);
        let path = if configured.is_absolute() {
            configured
        } else if let Some(data_dir) = opencode_data_dir() {
            data_dir.join(configured)
        } else {
            return Vec::new();
        };
        return path.is_file().then_some(path).into_iter().collect();
    }

    let Some(data_dir) = opencode_data_dir() else {
        return Vec::new();
    };
    let pattern = data_dir.join("opencode*.db");
    let mut databases = glob::glob(&pattern.to_string_lossy())
        .into_iter()
        .flatten()
        .flatten()
        .filter(|path| path.is_file())
        .collect::<Vec<_>>();
    databases.sort();
    databases.dedup();
    databases
}

fn xdg_data_dir(application: &str) -> Option<PathBuf> {
    env::var_os(XDG_DATA_HOME_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| dirs::home_dir().map(|home| home.join(".local/share")))
        .map(|root| root.join(application))
}

fn find_family_databases(
    db_env: &str,
    data_dir: Option<PathBuf>,
    patterns: &[&str],
) -> Vec<PathBuf> {
    if let Some(configured) = env::var_os(db_env).filter(|value| !value.is_empty()) {
        let configured = PathBuf::from(configured);
        let path = if configured.is_absolute() {
            configured
        } else if let Some(data_dir) = data_dir {
            data_dir.join(configured)
        } else {
            return Vec::new();
        };
        return path.is_file().then_some(path).into_iter().collect();
    }

    let Some(data_dir) = data_dir else {
        return Vec::new();
    };
    let mut databases = Vec::new();
    for pattern in patterns {
        let pattern = data_dir.join(pattern);
        if let Ok(matches) = glob::glob(&pattern.to_string_lossy()) {
            databases.extend(matches.flatten().filter(|path| path.is_file()));
        }
    }
    databases.sort();
    databases.dedup();
    databases
}

fn find_mimocode_databases() -> Vec<PathBuf> {
    if env::var_os(MIMOCODE_DB_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .is_some_and(|path| path.is_absolute())
    {
        return find_family_databases(MIMOCODE_DB_ENV, None, &["mimocode*.db"]);
    }
    let configured_home = env::var_os(MIMOCODE_HOME_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    let data_dir = match configured_home {
        Some(root) if root.is_absolute() => Some(root.join("data")),
        // Return a path that the parser will reject instead of silently reading
        // an unrelated XDG database after an invalid explicit override.
        Some(root) => return vec![root.join("data/mimocode.db")],
        None => xdg_data_dir("mimocode"),
    };
    find_family_databases(MIMOCODE_DB_ENV, data_dir, &["mimocode*.db"])
}

fn mimocode_home_is_invalid() -> bool {
    let database_is_absolute = env::var_os(MIMOCODE_DB_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .is_some_and(|path| path.is_absolute());
    !database_is_absolute
        && env::var_os(MIMOCODE_HOME_ENV)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .is_some_and(|path| !path.is_absolute())
}

fn find_kilo_databases() -> Vec<PathBuf> {
    find_family_databases(
        KILO_DB_ENV,
        xdg_data_dir("kilo"),
        &["kilo*.db", "opencode-*.db"],
    )
}

#[derive(Debug, Clone, Copy)]
enum RecordedCostPolicy {
    PositiveOnly,
    AnyReported,
}

#[derive(Debug, Clone, Copy)]
struct ParseProfile {
    source: &'static str,
    display_name: &'static str,
    cost_policy: RecordedCostPolicy,
    reconcile_fork_copies: bool,
}

impl ParseProfile {
    const fn opencode() -> Self {
        Self {
            source: "opencode",
            display_name: "OpenCode",
            cost_policy: RecordedCostPolicy::PositiveOnly,
            reconcile_fork_copies: false,
        }
    }

    const fn mimocode() -> Self {
        Self {
            source: "mimocode",
            display_name: "MiMo Code",
            cost_policy: RecordedCostPolicy::AnyReported,
            reconcile_fork_copies: true,
        }
    }

    const fn kilo() -> Self {
        Self {
            source: "kilo",
            display_name: "Kilo CLI",
            cost_policy: RecordedCostPolicy::AnyReported,
            reconcile_fork_copies: true,
        }
    }
}

#[derive(Debug)]
struct DatabaseMessage {
    id: String,
    session_id: String,
    data: String,
    project_path: Option<String>,
    schema: MessageSchema,
}

#[derive(Debug, Clone, Copy)]
enum MessageSchema {
    V1,
    V2,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OpenCodeMessage {
    role: Option<String>,
    #[serde(rename = "modelID")]
    model_id: Option<String>,
    model: Option<OpenCodeModel>,
    finish: Option<String>,
    cost: Option<f64>,
    tokens: OpenCodeTokens,
    time: OpenCodeTime,
    path: Option<OpenCodePath>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OpenCodeModel {
    id: String,
}

#[derive(Debug, Deserialize)]
struct OpenCodeTokens {
    input: i64,
    output: i64,
    reasoning: i64,
    cache: OpenCodeCache,
}

#[derive(Debug, Deserialize)]
struct OpenCodeCache {
    read: i64,
    write: i64,
}

#[derive(Debug, Deserialize)]
struct OpenCodeTime {
    created: f64,
    completed: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct OpenCodePath {
    root: Option<String>,
}

fn table_exists(connection: &Connection, table: &str) -> rusqlite::Result<bool> {
    connection
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1",
            params![table],
            |_| Ok(()),
        )
        .optional()
        .map(|row| row.is_some())
}

fn report_invalid_v1_json(
    connection: &Connection,
    output: &mut ParseOutput,
    debug: bool,
    path: &Path,
) {
    match connection.query_row(
        "SELECT COUNT(*) FROM message WHERE NOT json_valid(data)",
        [],
        |row| row.get::<_, i64>(0),
    ) {
        Ok(invalid) if invalid > 0 => {
            output.errors = output
                .errors
                .saturating_add(usize::try_from(invalid).unwrap_or(usize::MAX));
            if debug {
                eprintln!(
                    "Ignored {invalid} invalid JSON rows in {} table message",
                    path.display()
                );
            }
        }
        Ok(_) => {}
        Err(error) => {
            output.errors += 1;
            if debug {
                eprintln!(
                    "Failed to validate {} table message: {error}",
                    path.display()
                );
            }
        }
    }
}

fn read_table(
    connection: &Connection,
    schema: MessageSchema,
    output: &mut ParseOutput,
    debug: bool,
    path: &Path,
    timezone: Timezone,
    profile: ParseProfile,
) -> bool {
    let (table, query) = match schema {
        MessageSchema::V1 => (
            "message",
            "SELECT m.id, m.session_id, m.data, NULLIF(s.directory, '')
             FROM message m
             LEFT JOIN session s ON s.id = m.session_id
             WHERE json_valid(m.data)
               AND json_extract(m.data, '$.role') = 'assistant'",
        ),
        MessageSchema::V2 => (
            "session_message",
            "SELECT sm.id, sm.session_id, sm.data, NULLIF(s.directory, '')
             FROM session_message sm
             LEFT JOIN session s ON s.id = sm.session_id
             WHERE sm.type = 'assistant'",
        ),
    };

    match table_exists(connection, table) {
        Ok(false) => return false,
        Err(error) => {
            output.errors += 1;
            if debug {
                eprintln!(
                    "Failed to inspect {} table {table}: {error}",
                    path.display()
                );
            }
            return false;
        }
        Ok(true) => {}
    }

    if matches!(schema, MessageSchema::V1) {
        report_invalid_v1_json(connection, output, debug, path);
    }

    let mut statement = match connection.prepare(query) {
        Ok(statement) => statement,
        Err(error) => {
            output.errors += 1;
            if debug {
                eprintln!("Failed to query {} table {table}: {error}", path.display());
            }
            return true;
        }
    };
    let rows = match statement.query_map([], |row| {
        Ok(DatabaseMessage {
            id: row.get(0)?,
            session_id: row.get(1)?,
            data: row.get(2)?,
            project_path: row.get(3)?,
            schema,
        })
    }) {
        Ok(rows) => rows,
        Err(error) => {
            output.errors += 1;
            if debug {
                eprintln!("Failed to read {} table {table}: {error}", path.display());
            }
            return true;
        }
    };

    for row in rows {
        match row {
            Ok(row) => match entry_from_database_message(&row, timezone, profile) {
                Ok(Some(entry)) => output.entries.push(entry),
                Ok(None) => {}
                Err(error) => {
                    output.errors += 1;
                    if debug {
                        eprintln!(
                            "Invalid {} message {} in {}: {error}",
                            profile.display_name,
                            row.id,
                            path.display()
                        );
                    }
                }
            },
            Err(error) => {
                output.errors += 1;
                if debug {
                    eprintln!("Invalid row in {} table {table}: {error}", path.display());
                }
            }
        }
    }
    true
}

fn finite_millis(value: f64) -> Result<i64, &'static str> {
    if !value.is_finite() || value <= 0.0 || value > i64::MAX as f64 {
        return Err("invalid time.created");
    }
    Ok(value as i64)
}

fn entry_from_database_message(
    row: &DatabaseMessage,
    timezone: Timezone,
    profile: ParseProfile,
) -> Result<Option<RawEntry>, &'static str> {
    let message: OpenCodeMessage =
        serde_json::from_str(&row.data).map_err(|_| "invalid message JSON")?;
    if matches!(row.schema, MessageSchema::V1) && message.role.as_deref() != Some("assistant") {
        return Ok(None);
    }

    let model = message
        .model_id
        .as_deref()
        .or_else(|| message.model.as_ref().map(|model| model.id.as_str()))
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .ok_or("missing model")?
        .to_string();
    let tokens = message.tokens;
    if [
        tokens.input,
        tokens.output,
        tokens.reasoning,
        tokens.cache.read,
        tokens.cache.write,
    ]
    .into_iter()
    .any(|value| value < 0)
    {
        return Err("negative token count");
    }
    if message
        .cost
        .is_some_and(|cost| !cost.is_finite() || cost < 0.0)
    {
        return Err("invalid cost");
    }
    let recorded_cost_usd = match profile.cost_policy {
        RecordedCostPolicy::PositiveOnly => message.cost.filter(|cost| *cost > 0.0),
        RecordedCostPolicy::AnyReported => message.cost,
    };
    if tokens.input == 0
        && tokens.output == 0
        && tokens.reasoning == 0
        && tokens.cache.read == 0
        && tokens.cache.write == 0
        && recorded_cost_usd.is_none()
    {
        return Ok(None);
    }

    let timestamp_ms = finite_millis(message.time.created)?;
    let timestamp =
        DateTime::<Utc>::from_timestamp_millis(timestamp_ms).ok_or("invalid time.created")?;
    let project_path = row
        .project_path
        .as_deref()
        .or_else(|| message.path.as_ref().and_then(|path| path.root.as_deref()))
        .unwrap_or_default()
        .to_string();
    let completed = message
        .time
        .completed
        .is_some_and(|completed| completed.is_finite() && completed >= message.time.created);
    let stop_reason = message
        .finish
        .filter(|finish| !finish.trim().is_empty())
        .or_else(|| completed.then_some("completed".to_string()));
    Ok(Some(RawEntry {
        timestamp: timestamp.to_rfc3339(),
        timestamp_ms,
        date_str: timezone
            .to_fixed_offset(timestamp)
            .date_naive()
            .format(DATE_FORMAT)
            .to_string(),
        message_id: Some(source_wide_message_id(profile.source, &row.id)),
        session_key: format!("{}::{}", profile.source, row.session_id),
        session_id: row.session_id.clone(),
        project_path,
        model,
        input_tokens: tokens.input,
        output_tokens: tokens.output,
        cache_creation: tokens.cache.write,
        cache_creation_1h: 0,
        cache_read: tokens.cache.read,
        reasoning_tokens: tokens.reasoning,
        stop_reason,
        cost_kind: CostKind::Real,
        endpoint: Endpoint::Unknown,
        call_count: 1,
        recorded_cost_usd,
        api_equivalent_priced_tokens: 0,
        api_equivalent_coverage_tokens: 0,
    }))
}

fn parse_opencode_database(
    path: &Path,
    timezone: Timezone,
    debug: bool,
    profile: ParseProfile,
) -> ParseOutput {
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let connection = match Connection::open_with_flags(path, flags) {
        Ok(connection) => connection,
        Err(error) => {
            if debug {
                eprintln!(
                    "Failed to open {} database {}: {error}",
                    profile.display_name,
                    path.display()
                );
            }
            return ParseOutput {
                entries: Vec::new(),
                errors: 1,
            };
        }
    };

    let creation_times = if profile.reconcile_fork_copies {
        match read_session_creation_times(&connection) {
            Ok(creation_times) => creation_times,
            Err(error) => {
                if debug {
                    eprintln!(
                        "Failed to read {} session creation times {}: {error}",
                        profile.display_name,
                        path.display()
                    );
                }
                return ParseOutput {
                    entries: Vec::new(),
                    errors: 1,
                };
            }
        }
    } else {
        std::collections::HashMap::default()
    };

    let mut output = ParseOutput::default();
    let has_v2 = read_table(
        &connection,
        MessageSchema::V2,
        &mut output,
        debug,
        path,
        timezone,
        profile,
    );
    let has_v1 = read_table(
        &connection,
        MessageSchema::V1,
        &mut output,
        debug,
        path,
        timezone,
        profile,
    );
    if !has_v1 && !has_v2 && output.errors == 0 {
        output.errors = 1;
        if debug {
            eprintln!(
                "{} database {} has neither message table",
                profile.display_name,
                path.display()
            );
        }
    }
    if profile.reconcile_fork_copies {
        reconcile_fork_copies(&mut output.entries, &creation_times);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_v2_message_preserves_independent_buckets_and_recorded_cost() {
        let row = DatabaseMessage {
            id: "msg_1".to_string(),
            session_id: "ses_1".to_string(),
            project_path: Some("/tmp/project".to_string()),
            schema: MessageSchema::V2,
            data: r#"{"agent":"build","model":{"id":"gpt-5","providerID":"openai"},"finish":"stop","cost":0.25,"tokens":{"input":100,"output":20,"reasoning":5,"cache":{"read":30,"write":10}},"time":{"created":1788131045000,"completed":1788131046000}}"#.to_string(),
        };

        let entry = entry_from_database_message(
            &row,
            Timezone::Named(chrono_tz::UTC),
            ParseProfile::opencode(),
        )
        .unwrap()
        .unwrap();

        assert_eq!(entry.input_tokens, 100);
        assert_eq!(entry.output_tokens, 20);
        assert_eq!(entry.reasoning_tokens, 5);
        assert_eq!(entry.cache_read, 30);
        assert_eq!(entry.cache_creation, 10);
        assert_eq!(entry.recorded_cost_usd, Some(0.25));
        assert_eq!(entry.project_path, "/tmp/project");
    }

    #[test]
    fn malformed_usage_is_reported_instead_of_clamped() {
        let row = DatabaseMessage {
            id: "msg_bad".to_string(),
            session_id: "ses_1".to_string(),
            project_path: None,
            schema: MessageSchema::V2,
            data: r#"{"model":{"id":"gpt-5","providerID":"openai"},"tokens":{"input":-1,"output":2,"reasoning":0,"cache":{"read":0,"write":0}},"time":{"created":1788131045000}}"#.to_string(),
        };

        assert!(matches!(
            entry_from_database_message(
                &row,
                Timezone::Named(chrono_tz::UTC),
                ParseProfile::opencode(),
            ),
            Err("negative token count")
        ));
    }
}
