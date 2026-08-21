//! Provider-authoritative Codex weekly quota snapshots.
//!
//! Codex CLI writes rate-limit metadata alongside token-count events. This
//! module reads the newest 10,080-minute window and projects its current pace;
//! token totals are deliberately not used as a quota proxy.

use std::cmp::Reverse;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;

use super::parser::find_codex_files;

const WEEKLY_WINDOW_MINUTES: i64 = 7 * 24 * 60;

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
    let latest = latest_snapshot_in_files(find_codex_files()).ok_or_else(|| {
            "no Codex weekly quota snapshot was found. Start a Codex CLI session to refresh rate-limit data."
                .to_string()
        })?;

    build_report(&latest, now)
}

fn latest_snapshot_in_files(files: Vec<PathBuf>) -> Option<QuotaSnapshot> {
    let mut candidates: Vec<_> = files
        .into_iter()
        .filter_map(|path| {
            let modified = path.metadata().ok()?.modified().ok()?;
            Some((DateTime::<Utc>::from(modified), path))
        })
        .collect();
    candidates.sort_unstable_by_key(|candidate| Reverse(candidate.0));

    let mut latest: Option<QuotaSnapshot> = None;
    for (modified_at, path) in candidates {
        if latest
            .as_ref()
            .is_some_and(|snapshot| modified_at <= snapshot.observed_at)
        {
            break;
        }
        if let Some(snapshot) = latest_snapshot_in_file(&path)
            && latest
                .as_ref()
                .is_none_or(|current| snapshot.observed_at > current.observed_at)
        {
            latest = Some(snapshot);
        }
    }
    latest
}

fn latest_snapshot_in_file(path: &Path) -> Option<QuotaSnapshot> {
    let file = File::open(path).ok()?;
    latest_snapshot_from_reader(BufReader::new(file))
}

fn latest_snapshot_from_reader<R: BufRead>(reader: R) -> Option<QuotaSnapshot> {
    reader
        .lines()
        .map_while(Result::ok)
        .filter_map(|line| snapshot_from_line(&line))
        .max_by_key(|snapshot| snapshot.observed_at)
}

fn snapshot_from_line(line: &str) -> Option<QuotaSnapshot> {
    let entry: LogEntry<'_> = serde_json::from_str(line).ok()?;
    if entry.entry_type != Some("event_msg") {
        return None;
    }
    let observed_at = entry.timestamp?.parse::<DateTime<Utc>>().ok()?;
    let payload = entry.payload?;
    if payload.payload_type.as_deref() != Some("token_count") {
        return None;
    }
    let limits = payload.rate_limits?;

    [limits.primary, limits.secondary]
        .into_iter()
        .flatten()
        .find_map(|window| snapshot_from_window(observed_at, &window))
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
    if now >= snapshot.resets_at {
        return Err(format!(
            "the newest Codex weekly quota snapshot expired at {}. Start a Codex CLI session to refresh it.",
            snapshot.resets_at.to_rfc3339()
        ));
    }

    let window_start = snapshot.resets_at - Duration::minutes(snapshot.window_minutes);
    let elapsed_seconds = (snapshot.observed_at - window_start).num_seconds();
    if elapsed_seconds <= 0 || snapshot.observed_at >= snapshot.resets_at {
        return Err("the newest Codex weekly quota snapshot has invalid window timing".to_string());
    }

    let window_seconds = snapshot.window_minutes as f64 * 60.0;
    let projected_pct = if snapshot.used_pct == 0.0 {
        0.0
    } else {
        snapshot.used_pct * window_seconds / elapsed_seconds as f64
    };
    let estimated_depletion_at = (snapshot.used_pct > 0.0 && projected_pct > 100.0).then(|| {
        let seconds_to_limit = elapsed_seconds as f64 * 100.0 / snapshot.used_pct;
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
    use std::io::Cursor;

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
        let snapshot = latest_snapshot_from_reader(Cursor::new(input)).unwrap();

        assert_eq!(snapshot.observed_at, utc("2026-08-21T09:22:20Z"));
        assert_eq!(snapshot.window_minutes, WEEKLY_WINDOW_MINUTES);
        assert_close(snapshot.used_pct, 25.0);
        assert_eq!(snapshot.resets_at.timestamp(), 1_787_801_336);
    }

    #[test]
    fn reads_weekly_window_from_secondary_slot() {
        let input = r#"{"timestamp":"2026-08-21T09:22:20Z","type":"event_msg","payload":{"type":"token_count","rate_limits":{"primary":{"used_percent":10.0,"window_minutes":300,"resets_at":1787290000},"secondary":{"used_percent":40.0,"window_minutes":10080,"resets_at":1787801336}}}}"#;
        let snapshot = latest_snapshot_from_reader(Cursor::new(input)).unwrap();

        assert_close(snapshot.used_pct, 40.0);
        assert_eq!(snapshot.window_minutes, WEEKLY_WINDOW_MINUTES);
    }

    #[test]
    fn newest_valid_snapshot_wins_and_invalid_lines_are_ignored() {
        let input = r#"not-json
{"timestamp":"2026-08-21T08:00:00Z","type":"event_msg","payload":{"type":"token_count","rate_limits":{"primary":{"used_percent":20.0,"window_minutes":10080,"resets_at":1787801336}}}}
{"timestamp":"2026-08-21T09:00:00Z","type":"event_msg","payload":{"type":"token_count","rate_limits":{"primary":{"used_percent":30.0,"window_minutes":10080,"resets_at":1787801336}}}}
{"timestamp":"2026-08-21T10:00:00Z","type":"event_msg","payload":{"type":"token_count","rate_limits":{"primary":{"used_percent":999.0,"window_minutes":10080,"resets_at":1787801336}}}}"#;
        let snapshot = latest_snapshot_from_reader(Cursor::new(input)).unwrap();

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
