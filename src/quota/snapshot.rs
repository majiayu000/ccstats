//! Append-only Claude quota snapshots under the platform data directory.

use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Once;
use std::time::Duration;

use chrono::{DateTime, Utc};
use fs4::FileExt;
use serde::{Deserialize, Serialize};

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

    pub(crate) fn burn_pct_per_hour(&self, five_hour: bool) -> Option<f64> {
        let latest = self.latest()?;
        let current_window = if five_hour {
            latest.five_hour.as_ref()
        } else {
            latest.seven_day.as_ref()
        }?;
        let points: Vec<(DateTime<Utc>, f64)> = self
            .snapshots
            .iter()
            .filter_map(|snap| {
                let window = if five_hour {
                    snap.five_hour.as_ref()
                } else {
                    snap.seven_day.as_ref()
                }?;
                if window.resets_at != current_window.resets_at {
                    return None;
                }
                Some((snap.captured_at, window.used_percentage?))
            })
            .collect();
        if points.len() < 2 {
            return None;
        }
        let (t0, p0) = points[0];
        let (t1, p1) = points[points.len() - 1];
        let hours = t1.signed_duration_since(t0).num_seconds() as f64 / 3600.0;
        if hours <= 0.0 {
            return None;
        }
        let rate = (p1 - p0) / hours;
        rate.is_finite().then_some(rate)
    }

    pub(crate) fn time_to_exhaustion(&self, five_hour: bool) -> Option<Duration> {
        let latest = self.latest()?;
        let window = if five_hour {
            latest.five_hour.as_ref()
        } else {
            latest.seven_day.as_ref()
        }?;
        let used = window.used_percentage?;
        let burn = self.burn_pct_per_hour(five_hour)?;
        if burn <= 0.0 || used >= 100.0 {
            return None;
        }
        let hours = (100.0 - used) / burn;
        if !hours.is_finite() || hours < 0.0 {
            return None;
        }
        Some(Duration::from_secs_f64(hours * 3600.0))
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

    #[test]
    fn burn_rate_and_stale() {
        let t0 = DateTime::from_timestamp(1_000_000, 0).unwrap();
        let t1 = t0 + chrono::Duration::hours(2);
        let state = ClaudeQuotaState {
            snapshots: vec![
                ClaudeQuotaSnapshot {
                    captured_at: t0,
                    version: None,
                    five_hour: Some(RateWindow {
                        used_percentage: Some(10.0),
                        resets_at: None,
                    }),
                    seven_day: None,
                },
                ClaudeQuotaSnapshot {
                    captured_at: t1,
                    version: None,
                    five_hour: Some(RateWindow {
                        used_percentage: Some(20.0),
                        resets_at: None,
                    }),
                    seven_day: None,
                },
            ],
        };
        assert_eq!(state.burn_pct_per_hour(true), Some(5.0));
        assert!(!state.is_stale(t1 + chrono::Duration::minutes(1)));
        assert!(state.is_stale(t1 + chrono::Duration::minutes(11)));
    }

    #[test]
    fn burn_rate_and_exhaustion_use_each_current_reset_window() {
        let t0 = DateTime::from_timestamp(1_000_000, 0).unwrap();
        for reset_five_hour in [true, false] {
            let snapshots = [80.0, 10.0, 20.0]
                .into_iter()
                .enumerate()
                .map(|(index, used)| {
                    let current = index > 0;
                    let reset_window = RateWindow {
                        used_percentage: Some(used),
                        resets_at: Some(if current { 1_020_000 } else { 1_002_000 }),
                    };
                    let unchanged_window = RateWindow {
                        used_percentage: Some(40.0 + index as f64 * 2.0),
                        resets_at: Some(1_600_000),
                    };
                    let (five_hour, seven_day) = if reset_five_hour {
                        (reset_window, unchanged_window)
                    } else {
                        (unchanged_window, reset_window)
                    };
                    ClaudeQuotaSnapshot {
                        captured_at: t0 + chrono::Duration::hours(index as i64),
                        version: None,
                        five_hour: Some(five_hour),
                        seven_day: Some(seven_day),
                    }
                })
                .collect();
            let mut state = ClaudeQuotaState { snapshots };
            assert_eq!(state.burn_pct_per_hour(reset_five_hour), Some(10.0));
            assert_eq!(state.burn_pct_per_hour(!reset_five_hour), Some(2.0));
            assert_eq!(
                state.time_to_exhaustion(reset_five_hour),
                Some(Duration::from_secs(8 * 3600))
            );
            state.snapshots.pop();
            assert_eq!(state.burn_pct_per_hour(reset_five_hour), None);
            assert_eq!(state.time_to_exhaustion(reset_five_hour), None);
        }
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
