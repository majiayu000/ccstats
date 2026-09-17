//! Shared usage-facts cache. Source files remain authoritative.
//!
//! Filename carries the schema version (`usage-facts-v1.sqlite3`). Bump
//! `CACHE_VERSION` (and the filename) when parser semantics or stored fields
//! change so old rows cannot silently mix with a new algorithm.

use crate::utils::paths as dirs;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::Duration;
#[cfg(windows)]
use std::{fs::File, os::windows::io::AsRawHandle};
#[cfg(windows)]
use windows_sys::Win32::Storage::FileSystem::{
    FILE_BASIC_INFO, FileBasicInfo, GetFileInformationByHandleEx,
};

use chrono::{DateTime, NaiveDate, Utc};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::consts::DATE_FORMAT;
use crate::core::{DateFilter, RawEntry};
use crate::source::{ParseOutput, Source, loader::DataLoader};
use crate::utils::Timezone;

type CacheResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

/// Bump together with [`CACHE_FILE`] when stored facts or parser meaning change.
pub(crate) const CACHE_VERSION: u32 = 1;
pub(crate) const CACHE_FILE: &str = "usage-facts-v1.sqlite3";
const _: () = assert!(CACHE_VERSION >= 1);

static DISABLED: AtomicBool = AtomicBool::new(false);
static GLOBAL: OnceLock<UsageFactCache> = OnceLock::new();

/// How a source participates in the usage-facts cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CachePolicy {
    /// Do not cache (remote APIs, ephemeral files).
    None,
    /// Reuse a file when identity `(path, mtime, size, …)` is unchanged.
    PerFile,
    /// `SQLite` sources: currently the same as [`CachePolicy::PerFile`] because
    /// fork reconciliation still needs a full read when the file changes.
    /// Declared separately so rowid watermarks can land without a trait break.
    Watermark,
}

#[derive(Default)]
pub(crate) struct UsageFactCache {
    path: Option<PathBuf>,
    connection: OnceLock<Result<Mutex<Connection>, String>>,
    hits: AtomicUsize,
    reported_error: AtomicBool,
}

#[derive(Serialize, Deserialize)]
struct CacheRecord {
    session_key: String,
    #[serde(flatten)]
    entry: RawEntry,
}

impl CacheRecord {
    fn from_entry(entry: &RawEntry) -> Self {
        Self {
            session_key: entry.session_key.clone(),
            entry: entry.clone(),
        }
    }

    fn into_entry(self) -> RawEntry {
        let mut entry = self.entry;
        entry.session_key = self.session_key;
        entry
    }
}

pub(crate) fn set_disabled(disabled: bool) {
    DISABLED.store(disabled, Ordering::Relaxed);
}

pub(crate) fn is_disabled() -> bool {
    DISABLED.load(Ordering::Relaxed)
}

fn global() -> &'static UsageFactCache {
    GLOBAL.get_or_init(UsageFactCache::default)
}

pub(crate) fn hits() -> usize {
    global().hits()
}

/// Parse one file through the process-wide cache (or skip it when disabled).
pub(crate) fn parse_cached(
    source: &dyn Source,
    path: &Path,
    filter: &DateFilter,
    timezone: Timezone,
    debug: bool,
) -> ParseOutput {
    if is_disabled() || source.cache_policy() == CachePolicy::None {
        let parsed = source.parse_file(path, timezone, debug);
        return ParseOutput {
            entries: DataLoader::filter_entries(parsed.entries, filter, timezone),
            errors: parsed.errors,
        };
    }
    global().parse(source, path, filter, timezone, debug)
}

/// Drop files whose mtime (or dated filename) cannot overlap `filter.since`.
pub(crate) fn prune_discovered_files(
    files: Vec<PathBuf>,
    filter: &DateFilter,
    timezone: Timezone,
) -> Vec<PathBuf> {
    if filter.since.is_none() && filter.until.is_none() {
        return files;
    }
    files
        .into_iter()
        .filter(|path| file_might_overlap(path, filter, timezone))
        .collect()
}

fn file_might_overlap(path: &Path, filter: &DateFilter, timezone: Timezone) -> bool {
    if let Some(date) = filename_date(path) {
        if filter.since.is_some_and(|since| date < since) {
            return false;
        }
        if filter.until.is_some_and(|until| date > until) {
            return false;
        }
    }
    let Some(since) = filter.since else {
        return true;
    };
    let Ok(mtime) = fs::metadata(path).and_then(|meta| meta.modified()) else {
        return true;
    };
    let dt = DateTime::<Utc>::from(mtime);
    timezone.to_fixed_offset(dt).date_naive() >= since
}

fn filename_date(path: &Path) -> Option<NaiveDate> {
    let stem = path.file_stem()?.to_str()?;
    NaiveDate::parse_from_str(stem, "%Y-%m-%d").ok()
}

/// `date_str` is a local calendar day, so cached facts must be re-dated for
/// the timezone of this request instead of reusing the parse-time value.
fn apply_local_date(mut entry: RawEntry, timezone: Timezone) -> CacheResult<(RawEntry, NaiveDate)> {
    let utc = DateTime::<Utc>::from_timestamp_millis(entry.timestamp_ms)
        .or_else(|| entry.timestamp.parse::<DateTime<Utc>>().ok())
        .ok_or("invalid cached timestamp")?;
    if entry.timestamp_ms == 0 {
        entry.timestamp_ms = utc.timestamp_millis();
    }
    let date = timezone.to_fixed_offset(utc).date_naive();
    entry.date_str = date.format(DATE_FORMAT).to_string();
    Ok((entry, date))
}

fn table_ident(source: &str) -> String {
    let mut name = String::from("facts_");
    for ch in source.chars() {
        if ch.is_ascii_alphanumeric() {
            name.push(ch.to_ascii_lowercase());
        }
    }
    if name == "facts_" {
        name.push_str("unknown");
    }
    name
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
         CREATE TABLE IF NOT EXISTS meta (
             key TEXT PRIMARY KEY,
             value TEXT NOT NULL
         ) WITHOUT ROWID;
         INSERT OR IGNORE INTO meta(key, value) VALUES ('schema_version', '1');",
    )?;
    Ok(connection)
}

fn ensure_table(connection: &Connection, source: &str) -> CacheResult<()> {
    let table = table_ident(source);
    connection.execute_batch(&format!(
        "CREATE TABLE IF NOT EXISTS {table} (
             partition TEXT NOT NULL,
             path BLOB NOT NULL,
             stamp TEXT NOT NULL,
             first_ms INTEGER,
             last_ms INTEGER,
             payload BLOB NOT NULL,
             PRIMARY KEY (partition, path)
         ) WITHOUT ROWID;"
    ))?;
    Ok(())
}

fn fingerprint(path: &Path) -> CacheResult<String> {
    let metadata = fs::metadata(path)?;
    let mut stamp = format!("{}:{:?}", metadata.len(), metadata.modified()?);
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
    #[cfg(not(any(unix, windows)))]
    {
        use std::fmt::Write;
        write!(stamp, ":{:?}", metadata.created()?)?;
    }
    #[cfg(windows)]
    {
        use std::fmt::Write;
        let file = File::open(path)?;
        let mut info = FILE_BASIC_INFO::default();
        // SAFETY: file owns a live handle and info is the correctly sized, writable
        // buffer for FileBasicInfo. The API does not retain either pointer.
        let success = unsafe {
            GetFileInformationByHandleEx(
                file.as_raw_handle(),
                FileBasicInfo,
                std::ptr::from_mut(&mut info).cast(),
                u32::try_from(std::mem::size_of::<FILE_BASIC_INFO>())?,
            )
        };
        if success == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        write!(stamp, ":{}:{}", info.CreationTime, info.ChangeTime)?;
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

impl UsageFactCache {
    #[cfg(test)]
    fn open_at(path: &Path) -> Self {
        let cache = Self {
            path: Some(path.to_path_buf()),
            ..Self::default()
        };
        cache
            .connection
            .set(Ok(Mutex::new(open_cache(path).unwrap())))
            .unwrap();
        cache
    }

    fn connection(&self) -> CacheResult<MutexGuard<'_, Connection>> {
        let connection = self.connection.get_or_init(|| {
            let path = self
                .path
                .clone()
                .or_else(|| dirs::cache_dir().map(|root| root.join("ccstats").join(CACHE_FILE)));
            let result = path
                .ok_or_else(|| "cannot locate the platform cache directory".to_string())
                .and_then(|path| open_cache(&path).map_err(|e| e.to_string()));
            result.map(Mutex::new)
        });
        let connection = connection.as_ref().map_err(Clone::clone)?;
        connection.lock().map_err(|e| e.to_string().into())
    }

    pub(crate) fn hits(&self) -> usize {
        self.hits.load(Ordering::Relaxed)
    }

    fn report_error(&self, error: &dyn std::fmt::Display) {
        if !self.reported_error.swap(true, Ordering::Relaxed) {
            eprintln!("Error using usage-facts cache: {error}. Rebuilding usage from source logs.");
        }
    }

    fn load(
        &self,
        source: &str,
        partition: &str,
        path: &Path,
        stamp: &str,
        filter: &DateFilter,
        timezone: Timezone,
    ) -> CacheResult<Option<Vec<RawEntry>>> {
        let key = std::path::absolute(path)?;
        let table = table_ident(source);
        let payload: Vec<u8> = {
            let connection = self.connection()?;
            ensure_table(&connection, source)?;
            if filter.since.is_none() && filter.until.is_none() && !filter.has_timestamp_range() {
                let payload = connection
                    .prepare(&format!(
                        "SELECT payload FROM {table} WHERE partition=?1 AND path=?2 AND stamp=?3"
                    ))?
                    .query_row(
                        params![partition, key.as_os_str().as_encoded_bytes(), stamp],
                        |row| row.get(0),
                    )
                    .optional()?;
                let Some(payload) = payload else {
                    return Ok(None);
                };
                payload
            } else {
                let header = connection
                    .prepare(&format!(
                        "SELECT stamp, first_ms, last_ms FROM {table} WHERE partition=?1 AND path=?2"
                    ))?
                    .query_row(
                        params![partition, key.as_os_str().as_encoded_bytes()],
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
                let payload = connection
                    .prepare(&format!(
                        "SELECT payload FROM {table} WHERE partition=?1 AND path=?2 AND stamp=?3"
                    ))?
                    .query_row(
                        params![partition, key.as_os_str().as_encoded_bytes(), stamp],
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
        let stored: Vec<CacheRecord> = serde_json::from_slice(&bytes)?;
        let mut entries = Vec::new();
        for record in stored {
            let (entry, date) = apply_local_date(record.into_entry(), timezone)?;
            let included = if filter.has_timestamp_range() {
                filter.contains_entry_timestamp(&entry.timestamp, entry.timestamp_ms)
            } else {
                filter.contains(date)
            };
            if included {
                entries.push(entry);
            }
        }
        Ok(Some(entries))
    }

    fn save(
        &self,
        source: &str,
        partition: &str,
        path: &Path,
        stamp: &str,
        entries: &[RawEntry],
    ) -> CacheResult<()> {
        let key = std::path::absolute(path)?;
        let stored: Vec<CacheRecord> = entries.iter().map(CacheRecord::from_entry).collect();
        let bytes = serde_json::to_vec(&stored)?;
        let payload = zstd::stream::encode_all(bytes.as_slice(), 1)?;
        let first = entries.iter().map(|e| e.timestamp_ms).min();
        let last = entries.iter().map(|e| e.timestamp_ms).max();
        let table = table_ident(source);
        let connection = self.connection()?;
        ensure_table(&connection, source)?;
        connection
            .prepare(&format!(
                "INSERT INTO {table} (partition,path,stamp,first_ms,last_ms,payload)
                 VALUES (?1,?2,?3,?4,?5,?6)
                 ON CONFLICT(partition,path) DO UPDATE SET
                   stamp=excluded.stamp, first_ms=excluded.first_ms,
                   last_ms=excluded.last_ms, payload=excluded.payload"
            ))?
            .execute(params![
                partition,
                key.as_os_str().as_encoded_bytes(),
                stamp,
                first,
                last,
                payload
            ])?;
        Ok(())
    }

    pub(crate) fn parse(
        &self,
        source: &dyn Source,
        path: &Path,
        filter: &DateFilter,
        timezone: Timezone,
        debug: bool,
    ) -> ParseOutput {
        if source.cache_policy() == CachePolicy::None {
            let parsed = source.parse_file(path, timezone, debug);
            return ParseOutput {
                entries: DataLoader::filter_entries(parsed.entries, filter, timezone),
                errors: parsed.errors,
            };
        }
        let name = source.name();
        let partition = source.cache_partition();
        let before = fingerprint(path);
        match &before {
            Ok(stamp) => match self.load(name, partition, path, stamp, filter, timezone) {
                Ok(Some(entries)) => {
                    self.hits.fetch_add(1, Ordering::Relaxed);
                    if debug {
                        eprintln!("[DEBUG] cache hit {name} {}", path.display());
                    }
                    return ParseOutput { entries, errors: 0 };
                }
                Ok(None) => {
                    if debug {
                        eprintln!("[DEBUG] cache miss {name} {}", path.display());
                    }
                }
                Err(error) => self.report_error(&error),
            },
            Err(error) => self.report_error(error),
        }
        let parsed = source.parse_file(path, timezone, debug);
        if parsed.errors == 0
            && let Ok(before) = before
        {
            match fingerprint(path) {
                Ok(after) if before == after => {
                    if let Err(error) = self.save(name, partition, path, &before, &parsed.entries) {
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

    #[cfg(test)]
    fn corrupt_payloads(&self, source: &str) {
        let table = table_ident(source);
        self.connection()
            .unwrap()
            .execute(&format!("UPDATE {table} SET payload = x'00'"), [])
            .unwrap();
    }
}

#[cfg(test)]
#[path = "cache_tests.rs"]
mod cache_tests;

#[cfg(test)]
mod version_tests {
    use super::{CACHE_FILE, CACHE_VERSION};

    #[test]
    fn cache_filename_embeds_schema_version() {
        assert_eq!(
            CACHE_FILE,
            format!("usage-facts-v{CACHE_VERSION}.sqlite3").as_str()
        );
    }
}
