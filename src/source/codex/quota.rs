//! Provider-authoritative Codex weekly quota snapshots.
//!
//! Codex CLI writes rate-limit metadata alongside token-count events. This
//! module reads the newest 10,080-minute window and projects its current pace;
//! token totals are deliberately not used as a quota proxy.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Duration, NaiveDate, Utc};
use serde::Deserialize;

use super::parser::find_codex_files;

const WEEKLY_WINDOW_MINUTES: i64 = 7 * 24 * 60;
const DISCOVERY_MARGIN_MINUTES: i64 = 24 * 60;
const MAX_CLOCK_SKEW_SECONDS: i64 = 5 * 60;
const REVERSE_READ_CHUNK_SIZE: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QuotaStatus {
    OnTrack,
    Watch,
    LikelyExhausted,
    Exhausted,
}

impl QuotaStatus {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::OnTrack => "on_track",
            Self::Watch => "watch",
            Self::LikelyExhausted => "likely_exhausted",
            Self::Exhausted => "exhausted",
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct CodexQuotaReport {
    pub(crate) observed_at: DateTime<Utc>,
    pub(crate) resets_at: DateTime<Utc>,
    pub(crate) estimated_depletion_at: Option<DateTime<Utc>>,
    pub(crate) window_minutes: i64,
    pub(crate) used_pct: f64,
    pub(crate) remaining_pct: f64,
    pub(crate) projected_pct: f64,
    pub(crate) status: QuotaStatus,
}

#[derive(Debug, Clone)]
struct QuotaSnapshot {
    observed_at: DateTime<Utc>,
    resets_at: DateTime<Utc>,
    window_minutes: i64,
    used_pct: f64,
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

pub(crate) fn load_weekly_quota() -> Result<CodexQuotaReport, String> {
    load_weekly_quota_at(Utc::now())
}

fn load_weekly_quota_at(now: DateTime<Utc>) -> Result<CodexQuotaReport, String> {
    let files = recent_codex_files(find_codex_files(), now)?;
    let latest = latest_snapshot_in_files(files)?.ok_or_else(|| {
            "no Codex weekly quota snapshot was found. Start a Codex CLI session to refresh rate-limit data."
                .to_string()
        })?;

    build_report(&latest, now)
}

fn recent_codex_files(files: Vec<PathBuf>, now: DateTime<Utc>) -> Result<Vec<PathBuf>, String> {
    let cutoff = now - Duration::minutes(WEEKLY_WINDOW_MINUTES + DISCOVERY_MARGIN_MINUTES);
    files
        .into_iter()
        .filter_map(|path| {
            let path_is_recent =
                session_path_date(&path).is_some_and(|date| date >= cutoff.date_naive());
            let modified = match path.metadata().and_then(|metadata| metadata.modified()) {
                Ok(modified) => DateTime::<Utc>::from(modified),
                Err(error) => {
                    return Some(Err(format!(
                        "failed to inspect Codex session {}: {error}",
                        path.display()
                    )));
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

fn latest_snapshot_in_files(files: Vec<PathBuf>) -> Result<Option<QuotaSnapshot>, String> {
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

fn latest_snapshot_in_file(path: &Path) -> Result<Option<QuotaSnapshot>, String> {
    let mut file = File::open(path)
        .map_err(|error| format!("failed to open Codex session {}: {error}", path.display()))?;
    latest_snapshot_from_seekable(&mut file)
        .map_err(|error| format!("failed to read Codex session {}: {error}", path.display()))
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
                    Err(_) if trailing_segment && !ends_with_newline => {}
                    Err(error) => return Err(error),
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
        Err(_) if trailing_segment && !ends_with_newline => Ok(None),
        Err(error) => Err(error),
    }
}

fn snapshot_from_bytes(line: &[u8]) -> io::Result<Option<QuotaSnapshot>> {
    let line = std::str::from_utf8(line)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    snapshot_from_line(line).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

fn snapshot_from_line(line: &str) -> serde_json::Result<Option<QuotaSnapshot>> {
    if line.trim().is_empty() {
        return Ok(None);
    }
    let entry: LogEntry<'_> = serde_json::from_str(line)?;
    if entry.entry_type != Some("event_msg") {
        return Ok(None);
    }
    let Some(observed_at) = entry
        .timestamp
        .and_then(|timestamp| timestamp.parse::<DateTime<Utc>>().ok())
    else {
        return Ok(None);
    };
    let Some(payload) = entry.payload else {
        return Ok(None);
    };
    if payload.payload_type.as_deref() != Some("token_count") {
        return Ok(None);
    }
    let Some(limits) = payload.rate_limits else {
        return Ok(None);
    };

    Ok([limits.primary, limits.secondary]
        .into_iter()
        .flatten()
        .find_map(|window| snapshot_from_window(observed_at, &window)))
}

fn snapshot_from_window(
    observed_at: DateTime<Utc>,
    window: &RateLimitWindow,
) -> Option<QuotaSnapshot> {
    if window.window_minutes != WEEKLY_WINDOW_MINUTES
        || !window.used_percent.is_finite()
        || !(0.0..=100.0).contains(&window.used_percent)
    {
        return None;
    }
    let resets_at = DateTime::from_timestamp(window.resets_at, 0)?;
    Some(QuotaSnapshot {
        observed_at,
        resets_at,
        window_minutes: window.window_minutes,
        used_pct: window.used_percent,
    })
}

fn build_report(snapshot: &QuotaSnapshot, now: DateTime<Utc>) -> Result<CodexQuotaReport, String> {
    if snapshot.observed_at > now + Duration::seconds(MAX_CLOCK_SKEW_SECONDS) {
        return Err("the newest Codex weekly quota snapshot is dated in the future".to_string());
    }
    if now >= snapshot.resets_at {
        return Err(format!(
            "the newest Codex weekly quota snapshot expired at {}. Start a Codex CLI session to refresh it.",
            snapshot.resets_at.to_rfc3339()
        ));
    }

    let window_start = snapshot.resets_at - Duration::minutes(snapshot.window_minutes);
    let elapsed = snapshot.observed_at - window_start;
    if elapsed <= Duration::zero() || snapshot.observed_at >= snapshot.resets_at {
        return Err("the newest Codex weekly quota snapshot has invalid window timing".to_string());
    }
    let elapsed_seconds = elapsed
        .num_nanoseconds()
        .ok_or_else(|| "the newest Codex weekly quota snapshot has invalid timing".to_string())?
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
        QuotaStatus::Exhausted
    } else if projected_pct > 100.0 {
        QuotaStatus::LikelyExhausted
    } else if projected_pct >= 90.0 {
        QuotaStatus::Watch
    } else {
        QuotaStatus::OnTrack
    };

    Ok(CodexQuotaReport {
        observed_at: snapshot.observed_at,
        resets_at: snapshot.resets_at,
        estimated_depletion_at,
        window_minutes: snapshot.window_minutes,
        used_pct: snapshot.used_pct,
        remaining_pct: (100.0 - snapshot.used_pct).max(0.0),
        projected_pct,
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
    fn newest_valid_snapshot_wins_and_invalid_windows_are_ignored() {
        let input = r#"{"timestamp":"2026-08-21T08:00:00Z","type":"event_msg","payload":{"type":"token_count","rate_limits":{"primary":{"used_percent":20.0,"window_minutes":10080,"resets_at":1787801336}}}}
{"timestamp":"2026-08-21T09:00:00Z","type":"event_msg","payload":{"type":"token_count","rate_limits":{"primary":{"used_percent":30.0,"window_minutes":10080,"resets_at":1787801336}}}}
{"timestamp":"2026-08-21T10:00:00Z","type":"event_msg","payload":{"type":"token_count","rate_limits":{"primary":{"used_percent":999.0,"window_minutes":10080,"resets_at":1787801336}}}}"#;
        let snapshot = latest_snapshot_from_seekable(&mut Cursor::new(input))
            .unwrap()
            .unwrap();

        assert_eq!(snapshot.observed_at, utc("2026-08-21T09:00:00Z"));
        assert_close(snapshot.used_pct, 30.0);
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
    fn projection_uses_provider_window_boundaries() {
        let snapshot = QuotaSnapshot {
            observed_at: utc("2026-08-22T00:00:00Z"),
            resets_at: utc("2026-08-28T00:00:00Z"),
            window_minutes: WEEKLY_WINDOW_MINUTES,
            used_pct: 25.0,
        };

        let report = build_report(&snapshot, utc("2026-08-22T01:00:00Z")).unwrap();

        assert_close(report.remaining_pct, 75.0);
        assert_close(report.projected_pct, 175.0);
        assert_eq!(report.status, QuotaStatus::LikelyExhausted);
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

        assert!(error.contains("expired"));
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

        assert!(error.contains("future"));
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

        assert_close(report.projected_pct, 0.0);
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

        assert_close(report.projected_pct, 0.0);
        assert_eq!(report.status, QuotaStatus::OnTrack);
        assert_eq!(report.estimated_depletion_at, None);
    }
}
