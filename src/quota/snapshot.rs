//! Append-only Claude quota snapshots under the platform data directory.

use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Once;
use std::time::Duration;

use chrono::{DateTime, Utc};
use fs4::FileExt;
use serde::{Deserialize, Serialize};

use super::forecast::QuotaSample;
use super::hook::{ClaudeHook, RateWindow};
use crate::utils::paths as dirs;

pub(crate) const STALE_AFTER: Duration = Duration::from_secs(10 * 60);

const LOCK_FILE: &str = "claude.lock";
const SNAPSHOT_FILE: &str = "claude.jsonl";
static SNAPSHOT_WRITE_WARNING: Once = Once::new();

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct ClaudeQuotaSnapshot {
    pub captured_at: DateTime<Utc>,
    pub version: Option<String>,
    pub five_hour: Option<RateWindow>,
    pub seven_day: Option<RateWindow>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ClaudeQuotaState {
    pub snapshots: Vec<ClaudeQuotaSnapshot>,
}

impl ClaudeQuotaState {
    pub(crate) fn latest(&self) -> Option<&ClaudeQuotaSnapshot> {
        self.snapshots.last()
    }

    pub(crate) fn is_stale(&self, now: DateTime<Utc>) -> bool {
        let Some(latest) = self.latest() else {
            return true;
        };
        now.signed_duration_since(latest.captured_at)
            .to_std()
            .unwrap_or(Duration::ZERO)
            > STALE_AFTER
    }

    /// Recorded official observations of one window, each tagged with the
    /// reset time it reported, for burn-rate forecasting.
    pub(crate) fn samples(&self, five_hour: bool) -> Vec<QuotaSample> {
        self.snapshots
            .iter()
            .filter_map(|snap| {
                let window = if five_hour {
                    snap.five_hour.as_ref()
                } else {
                    snap.seven_day.as_ref()
                }?;
                Some(QuotaSample::new(
                    snap.captured_at,
                    window.used_percentage?,
                    window
                        .resets_at
                        .and_then(|reset| DateTime::from_timestamp(reset, 0)),
                ))
            })
            .collect()
    }
}

fn quota_dir() -> Option<PathBuf> {
    dirs::data_local_dir().map(|root| root.join("ccstats").join("quota"))
}

fn snapshot_path() -> Option<PathBuf> {
    quota_dir().map(|dir| dir.join(SNAPSHOT_FILE))
}

fn acquire_lock(dir: &Path) -> Result<File, String> {
    fs::create_dir_all(dir).map_err(|error| error.to_string())?;
    let lock_path = dir.join(LOCK_FILE);
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .map_err(|error| error.to_string())?;
    FileExt::lock(&lock).map_err(|error| error.to_string())?;
    Ok(lock)
}

pub(crate) fn append_claude_snapshot(hook: &ClaudeHook, captured_at: DateTime<Utc>) {
    if !hook.has_official_windows() {
        return;
    }
    let result = quota_dir()
        .ok_or_else(|| "cannot locate the platform data directory".to_string())
        .and_then(|dir| append_claude_snapshot_to(&dir, hook, captured_at));
    if let Err(error) = result {
        SNAPSHOT_WRITE_WARNING.call_once(|| {
            eprintln!("Warning: could not save Claude quota snapshot: {error}");
        });
    }
}

fn append_claude_snapshot_to(
    dir: &Path,
    hook: &ClaudeHook,
    captured_at: DateTime<Utc>,
) -> Result<(), String> {
    let path = dir.join(SNAPSHOT_FILE);
    let _lock = acquire_lock(dir)?;
    let snapshot = ClaudeQuotaSnapshot {
        captured_at,
        version: hook.version.clone(),
        five_hour: hook.five_hour().cloned(),
        seven_day: hook.seven_day().cloned(),
    };
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|error| error.to_string())?;
    serde_json::to_writer(&mut file, &snapshot).map_err(|error| error.to_string())?;
    file.write_all(b"\n").map_err(|error| error.to_string())?;
    file.flush().map_err(|error| error.to_string())
}

pub(crate) fn load_claude_quota_state() -> ClaudeQuotaState {
    let Some(path) = snapshot_path() else {
        return ClaudeQuotaState::default();
    };
    load_claude_quota_state_from(&path)
}

pub(crate) fn load_claude_quota_state_from(path: &Path) -> ClaudeQuotaState {
    let Ok(file) = File::open(path) else {
        return ClaudeQuotaState::default();
    };
    let mut snapshots = Vec::new();
    for line in BufReader::new(file).lines() {
        let Ok(line) = line else {
            continue;
        };
        if let Ok(snapshot) = serde_json::from_str::<ClaudeQuotaSnapshot>(&line) {
            snapshots.push(snapshot);
        }
    }
    ClaudeQuotaState { snapshots }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quota::hook::parse_claude_hook;

    fn snapshot(at: DateTime<Utc>, five: Option<RateWindow>) -> ClaudeQuotaSnapshot {
        ClaudeQuotaSnapshot {
            captured_at: at,
            version: None,
            five_hour: five,
            seven_day: None,
        }
    }

    #[test]
    fn samples_and_stale() {
        let t0 = DateTime::from_timestamp(1_000_000, 0).unwrap();
        let t1 = t0 + chrono::Duration::hours(2);
        let state = ClaudeQuotaState {
            snapshots: vec![
                snapshot(
                    t0,
                    Some(RateWindow {
                        used_percentage: Some(10.0),
                        resets_at: Some(1_020_000),
                    }),
                ),
                snapshot(
                    t1,
                    Some(RateWindow {
                        used_percentage: None,
                        resets_at: Some(1_020_000),
                    }),
                ),
                snapshot(t1, None),
            ],
        };
        let samples = state.samples(true);
        assert_eq!(
            samples,
            vec![QuotaSample::new(
                t0,
                10.0,
                DateTime::from_timestamp(1_020_000, 0)
            )]
        );
        assert!(state.samples(false).is_empty());
        assert!(!state.is_stale(t1 + chrono::Duration::minutes(1)));
        assert!(state.is_stale(t1 + chrono::Duration::minutes(11)));
    }

    #[test]
    fn append_round_trip() {
        let hook = parse_claude_hook(
            r#"{"version":"2.1.80","rate_limits":{"five_hour":{"used_percentage":10.0,"resets_at":1}}}"#,
        )
        .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(SNAPSHOT_FILE);
        append_claude_snapshot_to(dir.path(), &hook, Utc::now()).unwrap();
        let loaded = load_claude_quota_state_from(&path);
        assert_eq!(loaded.snapshots.len(), 1);
        assert_eq!(
            loaded
                .latest()
                .unwrap()
                .five_hour
                .as_ref()
                .unwrap()
                .used_percentage,
            Some(10.0)
        );
    }
}
