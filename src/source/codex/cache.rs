//! Reusable usage facts. Source JSONL files remain authoritative.

use std::error::Error;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::Duration;

use chrono::{DateTime, NaiveDate, Utc};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Deserialize;

use crate::consts::DATE_FORMAT;
use crate::core::{CostKind, DateFilter, Endpoint, RawEntry};
use crate::source::{ParseOutput, loader::DataLoader};
use crate::utils::Timezone;

use super::config::CodexScope;
use super::parser::parse_codex_file_with_scope;

type CacheResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

// Version the filename when the parser's usage semantics or stored fields change.
const CACHE_FILE: &str = "codex-usage-v3.sqlite3";

#[derive(Default)]
pub(super) struct CodexCache {
    connection: OnceLock<Result<Mutex<Connection>, String>>,
    hits: AtomicUsize,
    reported_error: AtomicBool,
}

// Timestamp, milliseconds, dedup identity, model, input, output, cache, reasoning.
// Followed by dedup session identity, display ID, working directory and cache writes.
#[derive(Deserialize)]
struct StoredEntry(
    String,
    i64,
    Option<String>,
    String,
    i64,
    i64,
    i64,
    i64,
    String,
    String,
    String,
    i64,
);

impl StoredEntry {
    fn expand(self, _path: &Path, date: NaiveDate) -> RawEntry {
        RawEntry {
            timestamp: self.0,
            timestamp_ms: self.1,
            date_str: date.format(DATE_FORMAT).to_string(),
            message_id: self.2,
            session_key: self.8,
            session_id: self.9,
            project_path: self.10,
            model: self.3,
            input_tokens: self.4,
            output_tokens: self.5,
            cache_creation: self.11,
            cache_creation_1h: 0,
            cache_read: self.6,
            reasoning_tokens: self.7,
            stop_reason: Some("complete".to_string()),
            cost_kind: CostKind::Real,
            endpoint: Endpoint::Unknown,
            call_count: 1,
            reported_total_tokens: None,
            recorded_cost_usd: None,
            api_equivalent_priced_tokens: 0,
            api_equivalent_coverage_tokens: 0,
        }
    }
}

fn open_cache(path: &Path) -> CacheResult<Connection> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let connection = Connection::open(path)?;
    connection.busy_timeout(Duration::from_secs(5))?;
    connection.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA synchronous=NORMAL;
         CREATE TABLE IF NOT EXISTS files (
             path BLOB NOT NULL,
             scope TEXT NOT NULL,
             stamp TEXT NOT NULL,
             first_ms INTEGER,
             last_ms INTEGER,
             payload BLOB NOT NULL,
             PRIMARY KEY (path, scope)
         ) WITHOUT ROWID;",
    )?;
    Ok(connection)
}

fn fingerprint(path: &Path) -> CacheResult<String> {
    let metadata = fs::metadata(path)?;
    let mut stamp = format!("{}:{:?}", metadata.len(), metadata.modified()?);
    // Detect same-size rewrites with restored mtime, and atomic replacement.
    #[cfg(unix)]
    {
        use std::fmt::Write;
        use std::os::unix::fs::MetadataExt;
        write!(
            stamp,
            ":{}:{}:{}:{}",
            metadata.dev(),
            metadata.ino(),
            metadata.ctime(),
            metadata.ctime_nsec()
        )?;
    }
    #[cfg(not(unix))]
    {
        use std::fmt::Write;
        write!(stamp, ":{:?}", metadata.created()?)?;
    }
    Ok(stamp)
}

fn overlaps(
    first: Option<i64>,
    last: Option<i64>,
    filter: &DateFilter,
    timezone: Timezone,
) -> CacheResult<bool> {
    let (Some(first), Some(last)) = (first, last) else {
        return Ok(false);
    };
    if filter.has_timestamp_range() {
        return Ok(filter.since_timestamp_ms.is_none_or(|since| last >= since)
            && filter.until_timestamp_ms.is_none_or(|until| first <= until));
    }
    let first =
        DateTime::<Utc>::from_timestamp_millis(first).ok_or("invalid first cached timestamp")?;
    let last =
        DateTime::<Utc>::from_timestamp_millis(last).ok_or("invalid last cached timestamp")?;
    let first = timezone.to_fixed_offset(first).date_naive();
    let last = timezone.to_fixed_offset(last).date_naive();
    Ok(filter.since.is_none_or(|since| last >= since)
        && filter.until.is_none_or(|until| first <= until))
}

impl CodexCache {
    fn connection(&self) -> CacheResult<MutexGuard<'_, Connection>> {
        let connection = self.connection.get_or_init(|| {
            let result = dirs::cache_dir()
                .ok_or_else(|| "cannot locate the platform cache directory".to_string())
                .and_then(|root| {
                    open_cache(&root.join("ccstats").join(CACHE_FILE)).map_err(|e| e.to_string())
                });
            result.map(Mutex::new)
        });
        let connection = connection.as_ref().map_err(Clone::clone)?;
        connection.lock().map_err(|e| e.to_string().into())
    }

    pub(super) fn hits(&self) -> usize {
        self.hits.load(Ordering::Relaxed)
    }

    fn report_error(&self, error: &dyn std::fmt::Display) {
        if !self.reported_error.swap(true, Ordering::Relaxed) {
            eprintln!("Error using Codex usage cache: {error}. Rebuilding usage from source logs.");
        }
    }

    fn load(
        &self,
        path: &Path,
        scope: CodexScope,
        stamp: &str,
        filter: &DateFilter,
        timezone: Timezone,
    ) -> CacheResult<Option<Vec<RawEntry>>> {
        let key = std::path::absolute(path)?;
        let payload: Vec<u8> = {
            let connection = self.connection()?;
            if filter.since.is_none() && filter.until.is_none() && !filter.has_timestamp_range() {
                let payload = connection
                    .prepare_cached(
                        "SELECT payload FROM files WHERE path=?1 AND scope=?2 AND stamp=?3",
                    )?
                    .query_row(
                        params![key.as_os_str().as_encoded_bytes(), scope.as_str(), stamp],
                        |row| row.get(0),
                    )
                    .optional()?;
                let Some(payload) = payload else {
                    return Ok(None);
                };
                payload
            } else {
                let header = connection
                    .prepare_cached(
                        "SELECT stamp, first_ms, last_ms FROM files WHERE path=?1 AND scope=?2",
                    )?
                    .query_row(
                        params![key.as_os_str().as_encoded_bytes(), scope.as_str()],
                        |row| {
                            Ok((
                                row.get::<_, String>(0)?,
                                row.get::<_, Option<i64>>(1)?,
                                row.get::<_, Option<i64>>(2)?,
                            ))
                        },
                    )
                    .optional()?;
                let Some((cached_stamp, first, last)) = header else {
                    return Ok(None);
                };
                if cached_stamp != stamp {
                    return Ok(None);
                }
                if !overlaps(first, last, filter, timezone)? {
                    return Ok(Some(Vec::new()));
                }
                // Match the fingerprint again in SQL: another process may replace this row.
                let payload = connection
                    .prepare_cached(
                        "SELECT payload FROM files WHERE path=?1 AND scope=?2 AND stamp=?3",
                    )?
                    .query_row(
                        params![key.as_os_str().as_encoded_bytes(), scope.as_str(), stamp],
                        |row| row.get(0),
                    )
                    .optional()?;
                let Some(payload) = payload else {
                    return Ok(None);
                };
                payload
            }
        };
        let bytes = zstd::stream::decode_all(payload.as_slice())?;
        let stored: Vec<StoredEntry> = serde_json::from_slice(&bytes)?;
        let mut entries = Vec::new();
        for entry in stored {
            let timestamp = DateTime::<Utc>::from_timestamp_millis(entry.1)
                .ok_or("invalid cached timestamp")?;
            let date = timezone.to_fixed_offset(timestamp).date_naive();
            let included = if filter.has_timestamp_range() {
                filter.contains_entry_timestamp(&entry.0, entry.1)
            } else {
                filter.contains(date)
            };
            if included {
                entries.push(entry.expand(path, date));
            }
        }
        Ok(Some(entries))
    }

    fn save(
        &self,
        path: &Path,
        scope: CodexScope,
        stamp: &str,
        entries: &[RawEntry],
    ) -> CacheResult<()> {
        let key = std::path::absolute(path)?;
        let stored: Vec<_> = entries
            .iter()
            .map(|entry| {
                (
                    &entry.timestamp,
                    entry.timestamp_ms,
                    &entry.message_id,
                    &entry.model,
                    entry.input_tokens,
                    entry.output_tokens,
                    entry.cache_read,
                    entry.reasoning_tokens,
                    &entry.session_key,
                    &entry.session_id,
                    &entry.project_path,
                    entry.cache_creation,
                )
            })
            .collect();
        let bytes = serde_json::to_vec(&stored)?;
        let payload = zstd::stream::encode_all(bytes.as_slice(), 1)?;
        let first = entries.iter().map(|e| e.timestamp_ms).min();
        let last = entries.iter().map(|e| e.timestamp_ms).max();
        self.connection()?.prepare_cached(
            "INSERT INTO files (path,scope,stamp,first_ms,last_ms,payload) VALUES (?1,?2,?3,?4,?5,?6)
             ON CONFLICT(path,scope) DO UPDATE SET stamp=excluded.stamp, first_ms=excluded.first_ms,
             last_ms=excluded.last_ms, payload=excluded.payload"
        )?.execute(params![key.as_os_str().as_encoded_bytes(), scope.as_str(), stamp, first, last, payload])?;
        Ok(())
    }

    pub(super) fn parse(
        &self,
        path: &Path,
        scope: CodexScope,
        filter: &DateFilter,
        timezone: Timezone,
        debug: bool,
    ) -> ParseOutput {
        let before = fingerprint(path);
        match &before {
            Ok(stamp) => match self.load(path, scope, stamp, filter, timezone) {
                Ok(Some(entries)) => {
                    self.hits.fetch_add(1, Ordering::Relaxed);
                    return ParseOutput { entries, errors: 0 };
                }
                Ok(None) => {}
                Err(error) => self.report_error(&error),
            },
            Err(error) => self.report_error(error),
        }
        let parsed = parse_codex_file_with_scope(path, timezone, debug, scope);
        // Never cache malformed or unstable reads. A growing file is parsed again next time.
        if parsed.errors == 0
            && let Ok(before) = before
        {
            match fingerprint(path) {
                Ok(after) if before == after => {
                    if let Err(error) = self.save(path, scope, &before, &parsed.entries) {
                        self.report_error(&error);
                    }
                }
                Ok(_) => {}
                Err(error) => self.report_error(&error),
            }
        }
        ParseOutput {
            entries: DataLoader::filter_entries(parsed.entries, filter, timezone),
            errors: parsed.errors,
        }
    }
}

#[cfg(test)]
#[path = "cache_tests.rs"]
mod tests;
