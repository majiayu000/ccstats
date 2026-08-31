//! Exact-window Codex usage input for weekly value estimation.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use rayon::prelude::*;

use crate::core::{DedupAccumulator, Stats};
use crate::utils::Timezone;

use super::parser::{codex_sessions_dir_candidate, parse_codex_file_for_quota};
use super::quota::{
    CodexQuotaError, codex_files_since, discover_quota_files, validate_sessions_dir,
};

#[derive(Debug)]
pub(crate) struct CodexWindowUsage {
    pub(crate) stats: Stats,
    pub(crate) models: HashMap<String, Stats>,
    pub(crate) valid_entries: i64,
    pub(crate) dedup_skipped_entries: i64,
}

pub(crate) fn load_weekly_window_usage_from_home(
    observed_at: DateTime<Utc>,
    window_started_at: DateTime<Utc>,
    codex_home: Option<&Path>,
) -> Result<CodexWindowUsage, CodexQuotaError> {
    let sessions_dir = resolve_sessions_dir(codex_home)?;
    validate_sessions_dir(&sessions_dir)?;
    let mut files = discover_quota_files(&sessions_dir)?;
    let archived_dir = sessions_dir
        .parent()
        .unwrap_or_else(|| Path::new(""))
        .join("archived_sessions");
    match validate_sessions_dir(&archived_dir) {
        Ok(()) => files.extend(discover_quota_files(&archived_dir)?),
        Err(CodexQuotaError::SessionsDirectoryNotFound { .. }) if !archived_dir.exists() => {}
        Err(error) => return Err(error),
    }
    let files = codex_files_since(files, window_started_at)?;
    load_weekly_window_usage_from_files(observed_at, window_started_at, &files)
}

fn resolve_sessions_dir(codex_home: Option<&Path>) -> Result<PathBuf, CodexQuotaError> {
    codex_home.map_or_else(
        || codex_sessions_dir_candidate().ok_or(CodexQuotaError::SnapshotNotFound),
        |home| Ok(home.join("sessions")),
    )
}

fn load_weekly_window_usage_from_files(
    observed_at: DateTime<Utc>,
    window_started_at: DateTime<Utc>,
    files: &[PathBuf],
) -> Result<CodexWindowUsage, CodexQuotaError> {
    let utc = Timezone::Named(chrono_tz::UTC);

    let (accumulator, parse_errors) = files
        .par_iter()
        .map(|path| {
            let parsed = parse_codex_file_for_quota(path, utc);
            let mut partial = DedupAccumulator::new();
            partial.extend(parsed.entries.into_iter().filter(|entry| {
                entry
                    .timestamp
                    .parse::<DateTime<Utc>>()
                    .is_ok_and(|timestamp| {
                        timestamp >= window_started_at && timestamp <= observed_at
                    })
            }));
            (partial, parsed.errors)
        })
        .reduce(
            || (DedupAccumulator::new(), 0usize),
            |(mut accumulator, errors), (partial, partial_errors)| {
                accumulator.merge(partial);
                (accumulator, errors.saturating_add(partial_errors))
            },
        );

    if parse_errors > 0 {
        return Err(CodexQuotaError::UsageParse {
            count: parse_errors,
        });
    }

    let (entries, dedup_skipped_entries) = accumulator.finalize();
    let valid_entries = entries.len() as i64;
    let mut stats = Stats::default();
    let mut models = HashMap::new();
    for entry in entries {
        let entry_stats = entry.to_stats();
        stats.add(&entry_stats);
        models
            .entry(entry.model)
            .or_insert_with(Stats::default)
            .add(&entry_stats);
    }

    Ok(CodexWindowUsage {
        stats,
        models,
        valid_entries,
        dedup_skipped_entries,
    })
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::Write as _;

    use chrono::{DateTime, Duration};
    use serde_json::json;
    use tempfile::{NamedTempFile, tempdir};

    use super::*;
    use crate::source::{CodexQuotaStatus, CodexWeeklyQuota};

    fn quota() -> CodexWeeklyQuota {
        CodexWeeklyQuota {
            observed_at: "2026-08-22T00:00:00Z".parse::<DateTime<_>>().unwrap(),
            resets_at: "2026-08-27T00:00:00Z".parse::<DateTime<_>>().unwrap(),
            estimated_depletion_at: None,
            window_minutes: 10_080,
            used_pct: 25.0,
            remaining_pct: 75.0,
            projected_pct_at_reset: 80.0,
            status: CodexQuotaStatus::OnTrack,
        }
    }

    fn usage_event(timestamp: &str, total_input: i64, delta_input: i64) -> String {
        json!({
            "timestamp": timestamp,
            "type": "event_msg",
            "payload": {
                "type": "token_count",
                "model": "gpt-5",
                "info": {
                    "total_token_usage": {
                        "input_tokens": total_input,
                        "cached_input_tokens": 0,
                        "output_tokens": 0,
                        "reasoning_output_tokens": 0,
                        "total_tokens": total_input,
                    },
                    "last_token_usage": {
                        "input_tokens": delta_input,
                        "cached_input_tokens": 0,
                        "output_tokens": 0,
                        "reasoning_output_tokens": 0,
                        "total_tokens": delta_input,
                    }
                }
            }
        })
        .to_string()
    }

    fn write_log(lines: &[&str]) -> NamedTempFile {
        let mut file = NamedTempFile::new().unwrap();
        for line in lines {
            writeln!(file, "{line}").unwrap();
        }
        file
    }

    #[test]
    fn usage_is_aligned_to_exact_provider_window_and_observation() {
        let before = usage_event("2026-08-19T23:59:59.999999500Z", 100, 100);
        let at_start = usage_event("2026-08-20T00:00:00Z", 300, 200);
        let at_observation = usage_event("2026-08-22T00:00:00Z", 600, 300);
        let after = usage_event("2026-08-22T00:00:00.000000500Z", 1_000, 400);
        let file = write_log(&[&before, &at_start, &at_observation, &after]);

        let quota = quota();
        let usage = load_weekly_window_usage_from_files(
            quota.observed_at,
            quota.resets_at - Duration::minutes(quota.window_minutes),
            &[file.path().to_path_buf()],
        )
        .unwrap();

        assert_eq!(usage.valid_entries, 2);
        assert_eq!(usage.stats.total_tokens(), 500);
        assert_eq!(usage.models["gpt-5"].total_tokens(), 500);
    }

    #[test]
    fn home_window_usage_includes_archived_sessions() {
        let home = tempdir().unwrap();
        fs::create_dir_all(home.path().join("sessions")).unwrap();
        let archived = home.path().join("archived_sessions/archived.jsonl");
        fs::create_dir_all(archived.parent().unwrap()).unwrap();
        fs::write(&archived, usage_event("2026-08-21T00:00:00Z", 100, 100)).unwrap();

        let usage = load_weekly_window_usage_from_home(
            "2026-08-22T00:00:00Z".parse().unwrap(),
            "2026-08-20T00:00:00Z".parse().unwrap(),
            Some(home.path()),
        )
        .unwrap();

        assert_eq!(usage.valid_entries, 1);
        assert_eq!(usage.stats.total_tokens(), 100);
    }

    #[test]
    fn malformed_usage_fails_value_estimate_closed() {
        let valid = usage_event("2026-08-21T00:00:00Z", 100, 100);
        let file = write_log(&[&valid, "{malformed"]);

        let quota = quota();
        let error = load_weekly_window_usage_from_files(
            quota.observed_at,
            quota.resets_at - Duration::minutes(quota.window_minutes),
            &[file.path().to_path_buf()],
        )
        .unwrap_err();

        assert!(matches!(error, CodexQuotaError::UsageParse { count: 1 }));
    }

    #[test]
    fn missing_model_metadata_stays_unpriceable() {
        let line =
            usage_event("2026-08-21T00:00:00Z", 100, 100).replace("\"model\":\"gpt-5\",", "");
        let file = write_log(&[&line]);

        let quota = quota();
        let usage = load_weekly_window_usage_from_files(
            quota.observed_at,
            quota.resets_at - Duration::minutes(quota.window_minutes),
            &[file.path().to_path_buf()],
        )
        .unwrap();

        assert!(usage.models.contains_key("unknown-model"));
        assert!(!usage.models.contains_key("gpt-5"));
    }
}
