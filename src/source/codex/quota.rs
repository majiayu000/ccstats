//! Provider-authoritative Codex weekly quota snapshots.
//!
//! Codex CLI writes rate-limit metadata alongside token-count events. This
//! module reads the newest 10,080-minute window and projects its current pace;
//! token totals are deliberately not used as a quota proxy.

use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Duration, NaiveDate, Utc};
use serde::de::Error as _;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::parser::codex_sessions_dir_candidate;

const WEEKLY_WINDOW_MINUTES: i64 = 7 * 24 * 60;
const DISCOVERY_MARGIN_MINUTES: i64 = 24 * 60;
const MAX_CLOCK_SKEW_SECONDS: i64 = 5 * 60;
const REVERSE_READ_CHUNK_SIZE: usize = 64 * 1024;

/// Projected risk for the current Codex weekly quota window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum CodexQuotaStatus {
    OnTrack,
    Watch,
    LikelyExhausted,
    Exhausted,
}

impl CodexQuotaStatus {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OnTrack => "on_track",
            Self::Watch => "watch",
            Self::LikelyExhausted => "likely_exhausted",
            Self::Exhausted => "exhausted",
        }
    }
}

/// Provider-authoritative Codex weekly quota usage with a pace projection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct CodexWeeklyQuota {
    pub observed_at: DateTime<Utc>,
    pub resets_at: DateTime<Utc>,
    pub estimated_depletion_at: Option<DateTime<Utc>>,
    pub window_minutes: i64,
    pub used_pct: f64,
    pub remaining_pct: f64,
    pub projected_pct_at_reset: f64,
    pub status: CodexQuotaStatus,
}

/// Errors returned while discovering or reading a Codex weekly quota snapshot.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum CodexQuotaError {
    #[error("Codex sessions directory was not found at {}", path.display())]
    SessionsDirectoryNotFound { path: PathBuf },

    #[error(
        "no Codex weekly quota snapshot was found. Start a Codex CLI session to refresh rate-limit data."
    )]
    SnapshotNotFound,

    #[error("failed to {action} Codex session {}: {source}", path.display())]
    SessionFile {
        action: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("the newest Codex weekly quota snapshot is dated in the future")]
    SnapshotInFuture,

    #[error(
        "the newest Codex weekly quota snapshot expired at {reset}. Start a Codex CLI session to refresh it.",
        reset = resets_at.to_rfc3339()
    )]
    SnapshotExpired { resets_at: DateTime<Utc> },

    #[error("the newest Codex weekly quota snapshot has invalid {reason}")]
    InvalidSnapshotTiming { reason: &'static str },

    #[error("failed to discover Codex sessions under {}: {source}", path.display())]
    SessionDiscovery {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error(
        "failed to estimate weekly value because {count} Codex usage records could not be parsed"
    )]
    UsageParse { count: usize },
}

#[derive(Debug, Clone)]
struct QuotaSnapshot {
    observed_at: DateTime<Utc>,
    resets_at: DateTime<Utc>,
    window_minutes: i64,
    used_pct: f64,
}

#[derive(Debug)]
enum SnapshotLineError {
    Incomplete,
    Invalid(io::Error),
}

impl SnapshotLineError {
    fn into_io_error(self) -> io::Error {
        match self {
            Self::Incomplete => io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "incomplete trailing Codex session record",
            ),
            Self::Invalid(error) => error,
        }
    }
}

#[derive(Debug, Deserialize)]
struct LogEntry<'a> {
    timestamp: Option<&'a str>,
    #[serde(rename = "type")]
    entry_type: Option<&'a str>,
    payload: Option<EventPayload>,
}

#[derive(Debug, Deserialize)]
struct EventPayload {
    #[serde(rename = "type")]
    payload_type: Option<String>,
    rate_limits: Option<RateLimits>,
}

#[derive(Debug, Deserialize)]
struct RateLimits {
    primary: Option<RateLimitWindow>,
    secondary: Option<RateLimitWindow>,
}

#[derive(Debug, Deserialize)]
struct RateLimitWindow {
    used_percent: f64,
    window_minutes: i64,
    resets_at: i64,
}

pub(crate) fn load_weekly_quota() -> Result<CodexWeeklyQuota, CodexQuotaError> {
    load_weekly_quota_from_home(None)
}

pub(crate) fn load_weekly_quota_from_home(
    codex_home: Option<&Path>,
) -> Result<CodexWeeklyQuota, CodexQuotaError> {
    let explicit_home = codex_home.is_some();
    let sessions_dir = if let Some(codex_home) = codex_home {
        codex_home.join("sessions")
    } else {
        codex_sessions_dir_candidate().ok_or(CodexQuotaError::SnapshotNotFound)?
    };
    if let Err(error) = validate_sessions_dir(&sessions_dir) {
        return match (explicit_home, error) {
            (false, CodexQuotaError::SessionsDirectoryNotFound { .. }) => {
                Err(CodexQuotaError::SnapshotNotFound)
            }
            (_, error) => Err(error),
        };
    }
    let files = discover_quota_files(&sessions_dir)?;
    load_weekly_quota_from_files_at(files, Utc::now())
}

pub(super) fn validate_sessions_dir(sessions_dir: &Path) -> Result<(), CodexQuotaError> {
    match fs::symlink_metadata(sessions_dir) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err(CodexQuotaError::SessionDiscovery {
                path: sessions_dir.to_path_buf(),
                source: io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "Codex sessions directory must not be a symbolic link",
                ),
            })
        }
        Ok(metadata) if metadata.is_dir() => Ok(()),
        Ok(_) => Err(CodexQuotaError::SessionsDirectoryNotFound {
            path: sessions_dir.to_path_buf(),
        }),
        Err(source) if source.kind() == io::ErrorKind::NotFound => {
            Err(CodexQuotaError::SessionsDirectoryNotFound {
                path: sessions_dir.to_path_buf(),
            })
        }
        Err(source) => Err(CodexQuotaError::SessionDiscovery {
            path: sessions_dir.to_path_buf(),
            source,
        }),
    }
}

pub(super) fn discover_quota_files(sessions_dir: &Path) -> Result<Vec<PathBuf>, CodexQuotaError> {
    let mut pending = vec![sessions_dir.to_path_buf()];
    let mut files = Vec::new();
    while let Some(directory) = pending.pop() {
        let entries =
            fs::read_dir(&directory).map_err(|source| CodexQuotaError::SessionDiscovery {
                path: directory.clone(),
                source,
            })?;
        for entry in entries {
            let entry = entry.map_err(|source| CodexQuotaError::SessionDiscovery {
                path: directory.clone(),
                source,
            })?;
            let path = entry.path();
            let file_type =
                entry
                    .file_type()
                    .map_err(|source| CodexQuotaError::SessionDiscovery {
                        path: path.clone(),
                        source,
                    })?;
            if file_type.is_dir() {
                pending.push(path);
            } else if file_type.is_file() && path.extension() == Some(OsStr::new("jsonl")) {
                files.push(path);
            }
        }
    }
    Ok(files)
}

fn load_weekly_quota_from_files_at(
    files: Vec<PathBuf>,
    now: DateTime<Utc>,
) -> Result<CodexWeeklyQuota, CodexQuotaError> {
    let files = recent_codex_files(files, now)?;
    let latest = latest_snapshot_in_files(files)?.ok_or(CodexQuotaError::SnapshotNotFound)?;
    build_report(&latest, now)
}

pub(super) fn recent_codex_files(
    files: Vec<PathBuf>,
    now: DateTime<Utc>,
) -> Result<Vec<PathBuf>, CodexQuotaError> {
    let cutoff = now - Duration::minutes(WEEKLY_WINDOW_MINUTES + DISCOVERY_MARGIN_MINUTES);
    files
        .into_iter()
        .filter_map(|path| {
            let path_is_recent =
                session_path_date(&path).is_some_and(|date| date >= cutoff.date_naive());
            let modified = match path.metadata().and_then(|metadata| metadata.modified()) {
                Ok(modified) => DateTime::<Utc>::from(modified),
                Err(error) => {
                    return Some(Err(CodexQuotaError::SessionFile {
                        action: "inspect",
                        path,
                        source: error,
                    }));
                }
            };
            (path_is_recent || modified >= cutoff).then_some(Ok(path))
        })
        .collect()
}

fn session_path_date(path: &Path) -> Option<NaiveDate> {
    let day = path.parent()?.file_name()?.to_str()?.parse().ok()?;
    let month = path
        .parent()?
        .parent()?
        .file_name()?
        .to_str()?
        .parse()
        .ok()?;
    let year = path
        .parent()?
        .parent()?
        .parent()?
        .file_name()?
        .to_str()?
        .parse()
        .ok()?;
    NaiveDate::from_ymd_opt(year, month, day)
}

fn latest_snapshot_in_files(files: Vec<PathBuf>) -> Result<Option<QuotaSnapshot>, CodexQuotaError> {
    let mut latest: Option<QuotaSnapshot> = None;
    for path in files {
        if let Some(snapshot) = latest_snapshot_in_file(&path)?
            && latest
                .as_ref()
                .is_none_or(|current| snapshot.observed_at > current.observed_at)
        {
            latest = Some(snapshot);
        }
    }
    Ok(latest)
}

fn latest_snapshot_in_file(path: &Path) -> Result<Option<QuotaSnapshot>, CodexQuotaError> {
    let mut file = File::open(path).map_err(|source| CodexQuotaError::SessionFile {
        action: "open",
        path: path.to_path_buf(),
        source,
    })?;
    latest_snapshot_from_seekable(&mut file).map_err(|source| CodexQuotaError::SessionFile {
        action: "read",
        path: path.to_path_buf(),
        source,
    })
}

fn latest_snapshot_from_seekable<R: Read + Seek>(
    reader: &mut R,
) -> io::Result<Option<QuotaSnapshot>> {
    let length = reader.seek(SeekFrom::End(0))?;
    if length == 0 {
        return Ok(None);
    }

    reader.seek(SeekFrom::Start(length - 1))?;
    let mut last_byte = [0_u8; 1];
    reader.read_exact(&mut last_byte)?;
    let ends_with_newline = last_byte[0] == b'\n';

    let mut remaining = length;
    let mut suffix = Vec::new();
    let mut trailing_segment = true;

    while remaining > 0 {
        let chunk_len = remaining.min(REVERSE_READ_CHUNK_SIZE as u64) as usize;
        remaining -= chunk_len as u64;
        reader.seek(SeekFrom::Start(remaining))?;

        let mut buffer = vec![0_u8; chunk_len];
        reader.read_exact(&mut buffer)?;
        buffer.extend_from_slice(&suffix);

        let mut line_end = buffer.len();
        while let Some(newline) = buffer[..line_end].iter().rposition(|byte| *byte == b'\n') {
            let line = &buffer[newline + 1..line_end];
            if !line.is_empty() {
                match snapshot_from_bytes(line) {
                    Ok(Some(snapshot)) => return Ok(Some(snapshot)),
                    Ok(None) => {}
                    Err(SnapshotLineError::Incomplete)
                        if trailing_segment && !ends_with_newline => {}
                    Err(error) => return Err(error.into_io_error()),
                }
            }
            trailing_segment = false;
            line_end = newline;
        }
        suffix = buffer[..line_end].to_vec();
    }

    if suffix.is_empty() {
        return Ok(None);
    }
    match snapshot_from_bytes(&suffix) {
        Ok(snapshot) => Ok(snapshot),
        Err(SnapshotLineError::Incomplete) if trailing_segment && !ends_with_newline => Ok(None),
        Err(error) => Err(error.into_io_error()),
    }
}

fn snapshot_from_bytes(line: &[u8]) -> Result<Option<QuotaSnapshot>, SnapshotLineError> {
    let line = std::str::from_utf8(line).map_err(|error| {
        if error.error_len().is_none() {
            SnapshotLineError::Incomplete
        } else {
            SnapshotLineError::Invalid(io::Error::new(io::ErrorKind::InvalidData, error))
        }
    })?;
    snapshot_from_line(line).map_err(|error| {
        if error.is_eof() {
            SnapshotLineError::Incomplete
        } else {
            SnapshotLineError::Invalid(io::Error::new(io::ErrorKind::InvalidData, error))
        }
    })
}

fn snapshot_from_line(line: &str) -> serde_json::Result<Option<QuotaSnapshot>> {
    if line.trim().is_empty() {
        return Ok(None);
    }
    let entry: LogEntry<'_> = serde_json::from_str(line)?;
    if entry.entry_type != Some("event_msg") {
        return Ok(None);
    }
    let Some(payload) = entry.payload else {
        return Ok(None);
    };
    if payload.payload_type.as_deref() != Some("token_count") {
        return Ok(None);
    }
    let Some(limits) = payload.rate_limits else {
        return Ok(None);
    };
    let Some(window) = [limits.primary, limits.secondary]
        .into_iter()
        .flatten()
        .find(|window| window.window_minutes == WEEKLY_WINDOW_MINUTES)
    else {
        return Ok(None);
    };
    let timestamp = entry
        .timestamp
        .ok_or_else(|| serde_json::Error::custom("weekly quota snapshot has no timestamp"))?;
    let observed_at = timestamp.parse::<DateTime<Utc>>().map_err(|error| {
        serde_json::Error::custom(format!(
            "weekly quota snapshot has invalid timestamp: {error}"
        ))
    })?;

    snapshot_from_window(observed_at, &window).map(Some)
}

fn snapshot_from_window(
    observed_at: DateTime<Utc>,
    window: &RateLimitWindow,
) -> serde_json::Result<QuotaSnapshot> {
    if !window.used_percent.is_finite() || !(0.0..=100.0).contains(&window.used_percent) {
        return Err(serde_json::Error::custom(
            "weekly quota snapshot has invalid used percentage",
        ));
    }
    let resets_at = DateTime::from_timestamp(window.resets_at, 0).ok_or_else(|| {
        serde_json::Error::custom("weekly quota snapshot has invalid reset timestamp")
    })?;
    Ok(QuotaSnapshot {
        observed_at,
        resets_at,
        window_minutes: window.window_minutes,
        used_pct: window.used_percent,
    })
}

fn build_report(
    snapshot: &QuotaSnapshot,
    now: DateTime<Utc>,
) -> Result<CodexWeeklyQuota, CodexQuotaError> {
    if snapshot.observed_at > now + Duration::seconds(MAX_CLOCK_SKEW_SECONDS) {
        return Err(CodexQuotaError::SnapshotInFuture);
    }
    if now >= snapshot.resets_at {
        return Err(CodexQuotaError::SnapshotExpired {
            resets_at: snapshot.resets_at,
        });
    }

    let window_start = snapshot.resets_at - Duration::minutes(snapshot.window_minutes);
    let elapsed = snapshot.observed_at - window_start;
    if elapsed <= Duration::zero() || snapshot.observed_at >= snapshot.resets_at {
        return Err(CodexQuotaError::InvalidSnapshotTiming {
            reason: "window timing",
        });
    }
    let elapsed_seconds = elapsed
        .num_nanoseconds()
        .ok_or(CodexQuotaError::InvalidSnapshotTiming { reason: "timing" })?
        as f64
        / 1_000_000_000.0;

    let window_seconds = snapshot.window_minutes as f64 * 60.0;
    let projected_pct = if snapshot.used_pct == 0.0 {
        0.0
    } else {
        snapshot.used_pct * window_seconds / elapsed_seconds
    };
    let estimated_depletion_at = (snapshot.used_pct > 0.0 && projected_pct > 100.0).then(|| {
        let seconds_to_limit = elapsed_seconds * 100.0 / snapshot.used_pct;
        window_start + Duration::seconds(seconds_to_limit.round() as i64)
    });
    let status = if snapshot.used_pct >= 100.0 {
        CodexQuotaStatus::Exhausted
    } else if projected_pct > 100.0 {
        CodexQuotaStatus::LikelyExhausted
    } else if projected_pct >= 90.0 {
        CodexQuotaStatus::Watch
    } else {
        CodexQuotaStatus::OnTrack
    };

    Ok(CodexWeeklyQuota {
        observed_at: snapshot.observed_at,
        resets_at: snapshot.resets_at,
        estimated_depletion_at,
        window_minutes: snapshot.window_minutes,
        used_pct: snapshot.used_pct,
        remaining_pct: (100.0 - snapshot.used_pct).max(0.0),
        projected_pct_at_reset: projected_pct,
        status,
    })
}

#[cfg(test)]
mod tests {
    use std::fs::{self, FileTimes};
    use std::io::Cursor;
    use std::time::{Duration as StdDuration, SystemTime};

    use super::*;

    fn utc(value: &str) -> DateTime<Utc> {
        value.parse().unwrap()
    }

    fn assert_close(actual: f64, expected: f64) {
        assert!((actual - expected).abs() < 1e-9, "{actual} != {expected}");
    }

    #[test]
    fn reads_weekly_window_from_primary_slot() {
        let input = r#"{"timestamp":"2026-08-21T09:22:20Z","type":"event_msg","payload":{"type":"token_count","rate_limits":{"primary":{"used_percent":25.0,"window_minutes":10080,"resets_at":1787801336},"secondary":null}}}"#;
        let snapshot = snapshot_from_line(input).unwrap().unwrap();

        assert_eq!(snapshot.observed_at, utc("2026-08-21T09:22:20Z"));
        assert_eq!(snapshot.window_minutes, WEEKLY_WINDOW_MINUTES);
        assert_close(snapshot.used_pct, 25.0);
        assert_eq!(snapshot.resets_at.timestamp(), 1_787_801_336);
    }

    #[test]
    fn reads_weekly_window_from_secondary_slot() {
        let input = r#"{"timestamp":"2026-08-21T09:22:20Z","type":"event_msg","payload":{"type":"token_count","rate_limits":{"primary":{"used_percent":10.0,"window_minutes":300,"resets_at":1787290000},"secondary":{"used_percent":40.0,"window_minutes":10080,"resets_at":1787801336}}}}"#;
        let snapshot = snapshot_from_line(input).unwrap().unwrap();

        assert_close(snapshot.used_pct, 40.0);
        assert_eq!(snapshot.window_minutes, WEEKLY_WINDOW_MINUTES);
    }

    #[test]
    fn malformed_newest_weekly_snapshot_fails_closed() {
        let input = r#"{"timestamp":"2026-08-21T08:00:00Z","type":"event_msg","payload":{"type":"token_count","rate_limits":{"primary":{"used_percent":20.0,"window_minutes":10080,"resets_at":1787801336}}}}
{"timestamp":"2026-08-21T09:00:00Z","type":"event_msg","payload":{"type":"token_count","rate_limits":{"primary":{"used_percent":30.0,"window_minutes":10080,"resets_at":1787801336}}}}
{"timestamp":"2026-08-21T10:00:00Z","type":"event_msg","payload":{"type":"token_count","rate_limits":{"primary":{"used_percent":999.0,"window_minutes":10080,"resets_at":1787801336}}}}"#;
        let error = latest_snapshot_from_seekable(&mut Cursor::new(input)).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn newest_embedded_timestamp_wins_even_when_mtime_is_older() {
        let root = tempfile::tempdir().unwrap();
        let newer_snapshot = root.path().join("sessions/2026/08/21/restored-newer.jsonl");
        let older_snapshot = root.path().join("recently-touched-older.jsonl");
        fs::create_dir_all(newer_snapshot.parent().unwrap()).unwrap();
        fs::write(
            &newer_snapshot,
            r#"{"timestamp":"2026-08-21T10:00:00Z","type":"event_msg","payload":{"type":"token_count","rate_limits":{"primary":{"used_percent":40.0,"window_minutes":10080,"resets_at":1787801336}}}}"#,
        )
        .unwrap();
        fs::write(
            &older_snapshot,
            r#"{"timestamp":"2026-08-21T09:00:00Z","type":"event_msg","payload":{"type":"token_count","rate_limits":{"primary":{"used_percent":30.0,"window_minutes":10080,"resets_at":1787801336}}}}"#,
        )
        .unwrap();
        File::options()
            .write(true)
            .open(&newer_snapshot)
            .unwrap()
            .set_times(
                FileTimes::new()
                    .set_modified(SystemTime::UNIX_EPOCH + StdDuration::from_secs(1_700_000_000)),
            )
            .unwrap();

        let files = recent_codex_files(
            vec![older_snapshot, newer_snapshot],
            utc("2026-08-22T00:00:00Z"),
        )
        .unwrap();
        let snapshot = latest_snapshot_in_files(files).unwrap().unwrap();

        assert_eq!(snapshot.observed_at, utc("2026-08-21T10:00:00Z"));
        assert_close(snapshot.used_pct, 40.0);
    }

    #[test]
    fn reader_errors_are_not_silently_ignored() {
        let mut input = br#"{"timestamp":"2026-08-21T08:00:00Z","type":"event_msg","payload":{"type":"token_count","rate_limits":{"primary":{"used_percent":20.0,"window_minutes":10080,"resets_at":1787801336}}}}
"#
        .to_vec();
        input.extend_from_slice(&[0xff, b'\n']);

        let error = latest_snapshot_from_seekable(&mut Cursor::new(input)).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn malformed_complete_json_line_is_an_error() {
        let mut input = Cursor::new(b"{not-json}\n");

        let error = latest_snapshot_from_seekable(&mut input).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn incomplete_trailing_write_does_not_hide_latest_complete_snapshot() {
        let input = format!(
            "{}\n{{\"timestamp\":",
            r#"{"timestamp":"2026-08-21T09:00:00Z","type":"event_msg","payload":{"type":"token_count","rate_limits":{"primary":{"used_percent":30.0,"window_minutes":10080,"resets_at":1787801336}}}}"#
        );

        let snapshot = latest_snapshot_from_seekable(&mut Cursor::new(input))
            .unwrap()
            .unwrap();

        assert_eq!(snapshot.observed_at, utc("2026-08-21T09:00:00Z"));
        assert_close(snapshot.used_pct, 30.0);
    }

    #[test]
    fn incomplete_trailing_utf8_does_not_hide_latest_complete_snapshot() {
        let mut input = br#"{"timestamp":"2026-08-21T09:00:00Z","type":"event_msg","payload":{"type":"token_count","rate_limits":{"primary":{"used_percent":30.0,"window_minutes":10080,"resets_at":1787801336}}}}
"#
        .to_vec();
        input.extend_from_slice(&[0xe2, 0x82]);

        let snapshot = latest_snapshot_from_seekable(&mut Cursor::new(input))
            .unwrap()
            .unwrap();

        assert_close(snapshot.used_pct, 30.0);
    }

    #[test]
    fn projection_uses_provider_window_boundaries() {
        let snapshot = QuotaSnapshot {
            observed_at: utc("2026-08-22T00:00:00Z"),
            resets_at: utc("2026-08-28T00:00:00Z"),
            window_minutes: WEEKLY_WINDOW_MINUTES,
            used_pct: 25.0,
        };

        let report = build_report(&snapshot, utc("2026-08-22T01:00:00Z")).unwrap();

        assert_close(report.remaining_pct, 75.0);
        assert_close(report.projected_pct_at_reset, 175.0);
        assert_eq!(report.status, CodexQuotaStatus::LikelyExhausted);
        assert_eq!(
            report.estimated_depletion_at,
            Some(utc("2026-08-25T00:00:00Z"))
        );
    }

    #[test]
    fn expired_snapshot_fails_closed() {
        let snapshot = QuotaSnapshot {
            observed_at: utc("2026-08-21T23:00:00Z"),
            resets_at: utc("2026-08-22T00:00:00Z"),
            window_minutes: WEEKLY_WINDOW_MINUTES,
            used_pct: 25.0,
        };

        let error = build_report(&snapshot, utc("2026-08-22T00:00:00Z")).unwrap_err();

        assert!(matches!(
            error,
            CodexQuotaError::SnapshotExpired { resets_at }
                if resets_at == utc("2026-08-22T00:00:00Z")
        ));
    }

    #[test]
    fn future_dated_snapshot_fails_closed() {
        let snapshot = QuotaSnapshot {
            observed_at: utc("2026-08-22T00:10:00Z"),
            resets_at: utc("2026-08-28T00:00:00Z"),
            window_minutes: WEEKLY_WINDOW_MINUTES,
            used_pct: 25.0,
        };

        let error = build_report(&snapshot, utc("2026-08-22T00:00:00Z")).unwrap_err();

        assert!(matches!(error, CodexQuotaError::SnapshotInFuture));
    }

    #[test]
    fn small_clock_skew_is_tolerated() {
        let snapshot = QuotaSnapshot {
            observed_at: utc("2026-08-22T00:00:01Z"),
            resets_at: utc("2026-08-28T00:00:00Z"),
            window_minutes: WEEKLY_WINDOW_MINUTES,
            used_pct: 25.0,
        };

        let report = build_report(&snapshot, utc("2026-08-22T00:00:00Z")).unwrap();

        assert_close(report.used_pct, 25.0);
    }

    #[test]
    fn subsecond_elapsed_window_is_valid() {
        let snapshot = QuotaSnapshot {
            observed_at: utc("2026-08-21T00:00:00.500Z"),
            resets_at: utc("2026-08-28T00:00:00Z"),
            window_minutes: WEEKLY_WINDOW_MINUTES,
            used_pct: 0.0,
        };

        let report = build_report(&snapshot, utc("2026-08-21T00:00:01Z")).unwrap();

        assert_close(report.projected_pct_at_reset, 0.0);
    }

    #[test]
    fn zero_usage_has_no_depletion_estimate() {
        let snapshot = QuotaSnapshot {
            observed_at: utc("2026-08-22T00:00:00Z"),
            resets_at: utc("2026-08-28T00:00:00Z"),
            window_minutes: WEEKLY_WINDOW_MINUTES,
            used_pct: 0.0,
        };

        let report = build_report(&snapshot, utc("2026-08-22T01:00:00Z")).unwrap();

        assert_close(report.projected_pct_at_reset, 0.0);
        assert_eq!(report.status, CodexQuotaStatus::OnTrack);
        assert_eq!(report.estimated_depletion_at, None);
    }
}
