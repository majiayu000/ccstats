//! Exact-window Codex usage input for weekly value estimation.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use rayon::prelude::*;

use crate::core::{DedupAccumulator, RawEntry, Stats};
use crate::utils::Timezone;

use super::parser::{codex_sessions_dir_candidate, parse_codex_file_for_quota};
use super::quota::{
    CodexQuotaError, QuotaSnapshot, codex_files_since, discover_quota_files, snapshot_from_line,
    validate_sessions_dir,
};

#[derive(Debug)]
pub(crate) struct CodexWindowUsage {
    pub(crate) stats: Stats,
    pub(crate) models: HashMap<String, Stats>,
    pub(crate) model_samples: HashMap<String, ModelTokenSample>,
    pub(crate) valid_entries: i64,
    pub(crate) dedup_skipped_entries: i64,
}

#[derive(Debug, Default)]
pub(crate) struct ModelTokenSample {
    pub(crate) tokens: i64,
    pub(crate) used_pct: f64,
}

pub(crate) fn load_weekly_window_usage_from_home(
    observed_at: DateTime<Utc>,
    window_started_at: DateTime<Utc>,
    resets_at: DateTime<Utc>,
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
    load_weekly_window_usage_from_files(observed_at, window_started_at, resets_at, &files)
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
    resets_at: DateTime<Utc>,
    files: &[PathBuf],
) -> Result<CodexWindowUsage, CodexQuotaError> {
    let utc = Timezone::Named(chrono_tz::UTC);

    let (accumulator, parse_errors) = files
        .par_iter()
        .map(|path| {
            let parsed = parse_codex_file_for_quota(path, utc);
            let mut partial = DedupAccumulator::new();
            partial.extend(parsed.entries.into_iter().filter(|entry| {
                // GPT-Reserve uses a complimentary pool, not the subscription week.
                // Filter after parsing so cumulative deltas across model switches
                // remain correct; general usage analytics keep these entries.
                entry.model != "gpt-reserve"
                    && entry
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
    let mut snapshots = Vec::new();
    for path in files {
        let file = File::open(path).map_err(|source| CodexQuotaError::SessionFile {
            path: path.clone(),
            action: "read",
            source,
        })?;
        for line in BufReader::new(file).lines() {
            let line = line.map_err(|source| CodexQuotaError::SessionFile {
                path: path.clone(),
                action: "read",
                source,
            })?;
            let snapshot =
                snapshot_from_line(&line).map_err(|source| CodexQuotaError::SessionFile {
                    path: path.clone(),
                    action: "parse weekly snapshot",
                    source: std::io::Error::new(std::io::ErrorKind::InvalidData, source),
                })?;
            if let Some(snapshot) = snapshot
                && snapshot.resets_at == resets_at
                && snapshot.observed_at >= window_started_at
                && snapshot.observed_at <= observed_at
            {
                snapshots.push(snapshot);
            }
        }
    }
    let model_samples = model_token_samples(&entries, snapshots)?;
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
        model_samples,
        valid_entries,
        dedup_skipped_entries,
    })
}

/// Only attribute quota changes when all locally observed tokens between the
/// bounding snapshots belong to one model. Other devices/cloud usage remains
/// unobservable, so these are local estimates, never provider allowances.
fn model_token_samples(
    entries: &[RawEntry],
    mut snapshots: Vec<QuotaSnapshot>,
) -> Result<HashMap<String, ModelTokenSample>, CodexQuotaError> {
    // Provider percentages are rounded. Require a continuous span of at least
    // five percentage points instead of extrapolating a single rounded tick.
    const MIN_SAMPLE_PCT: f64 = 5.0;
    let mut samples: HashMap<String, ModelTokenSample> = HashMap::new();
    let mut timed_entries: Vec<_> = entries
        .iter()
        .map(|entry| {
            entry
                .timestamp
                .parse::<DateTime<Utc>>()
                .map(|at| (at, entry))
                .map_err(|_| CodexQuotaError::UsageParse { count: 1 })
        })
        .collect::<Result<Vec<_>, _>>()?;
    timed_entries.sort_by_key(|(timestamp, _)| *timestamp);
    snapshots.sort_by_key(|snapshot| snapshot.observed_at);
    snapshots.dedup_by(|a, b| {
        a.observed_at == b.observed_at
            && a.used_pct.partial_cmp(&b.used_pct) == Some(std::cmp::Ordering::Equal)
    });

    let mut entry_index = 0;
    let mut model: Option<&str> = None;
    let mut tokens = 0i64;
    let mut used_pct = 0.0;
    let save =
        |samples: &mut HashMap<String, ModelTokenSample>, model: Option<&str>, tokens, used_pct| {
            if let Some(model) = model
                && used_pct >= MIN_SAMPLE_PCT
                && tokens > 0
            {
                let sample = samples.entry(model.to_string()).or_default();
                sample.tokens += tokens;
                sample.used_pct += used_pct;
            }
        };
    for (index, pair) in snapshots.windows(2).enumerate() {
        let (before, after) = (&pair[0], &pair[1]);
        let mut interval_model: Option<&str> = None;
        let mut interval_tokens = 0;
        let mut mixed = false;
        while let Some((timestamp, entry)) = timed_entries.get(entry_index) {
            if *timestamp > after.observed_at {
                break;
            }
            entry_index += 1;
            if *timestamp <= before.observed_at {
                continue;
            }
            if interval_model.is_some_and(|model| model != entry.model) {
                mixed = true;
            }
            interval_model = Some(&entry.model);
            interval_tokens += entry.to_stats().total_tokens();
        }
        let delta = after.used_pct - before.used_pct;
        let conflicting_timestamp = after.observed_at == before.observed_at
            || (index > 0 && snapshots[index - 1].observed_at == before.observed_at)
            || snapshots
                .get(index + 2)
                .is_some_and(|next| next.observed_at == after.observed_at);
        // Never bridge ambiguous/contradictory snapshots, resets, saturation,
        // mixed-model intervals, or quota consumed without local token records.
        if conflicting_timestamp
            || delta < 0.0
            || after.used_pct >= 100.0
            || mixed
            || (interval_tokens == 0 && delta > 0.0)
        {
            if !conflicting_timestamp && delta >= 0.0 {
                save(&mut samples, model, tokens, used_pct);
            }
            model = None;
            tokens = 0;
            used_pct = 0.0;
            continue;
        }
        if interval_model.is_some() && interval_model != model {
            save(&mut samples, model, tokens, used_pct);
            model = interval_model;
            tokens = 0;
            used_pct = 0.0;
        }
        tokens += interval_tokens;
        used_pct += delta;
    }
    save(&mut samples, model, tokens, used_pct);
    Ok(samples)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::Write as _;

    use chrono::{DateTime, Duration};
    use serde_json::json;
    use tempfile::{NamedTempFile, tempdir};

    use super::*;
    use crate::pricing::PricingDb;
    use crate::sdk::{CodexWeeklyValueError, estimate_codex_weekly_value_with_pricing};
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
            quota.resets_at,
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
            quota().resets_at,
            Some(home.path()),
        )
        .unwrap();

        assert_eq!(usage.valid_entries, 1);
        assert_eq!(usage.stats.total_tokens(), 100);
    }

    #[test]
    fn reserve_is_excluded_from_weekly_value_but_kept_in_general_usage() {
        let home = tempdir().unwrap();
        let sessions = home.path().join("sessions");
        fs::create_dir_all(&sessions).unwrap();
        let log = sessions.join("mixed.jsonl");
        let before = usage_event("2026-08-21T08:00:00Z", 100, 100);
        let reserve =
            usage_event("2026-08-21T09:00:00Z", 1100, 1000).replace("gpt-5", "gpt-reserve");
        let after = usage_event("2026-08-21T10:00:00Z", 1300, 200);
        fs::write(&log, format!("{before}\n{reserve}\n{after}\n")).unwrap();

        let parsed = super::super::parser::parse_codex_file_with_scope(
            &log,
            Timezone::Named(chrono_tz::UTC),
            false,
            super::super::config::CodexScope::All,
        );
        assert_eq!(parsed.errors, 0);
        assert_eq!(
            parsed
                .entries
                .iter()
                .map(|e| e.to_stats().total_tokens())
                .sum::<i64>(),
            1300
        );
        assert!(parsed.entries.iter().any(|e| e.model == "gpt-reserve"));

        let pricing = PricingDb::default();
        let estimate =
            estimate_codex_weekly_value_with_pricing(&quota(), Some(home.path()), &pricing)
                .unwrap();
        assert_eq!(estimate.observed_tokens, 300);
        assert!((estimate.estimated_weekly_tokens - 1200.0).abs() < 1e-6);
        assert!((estimate.used_pct - 25.0).abs() < 1e-6);
        assert_eq!(estimate.valid_entries, 2);
        assert_eq!(estimate.model_estimates.len(), 1);
        assert_eq!(estimate.model_estimates[0].model, "gpt-5");
        assert_eq!(estimate.model_estimates[0].estimated_weekly_tokens, None);
        let cost = crate::pricing::calculate_cost(
            &Stats {
                input_tokens: 300,
                ..Stats::default()
            },
            "gpt-5",
            &pricing,
        );
        assert!(cost.is_finite() && cost > 0.0);
        assert!((estimate.observed_cost_usd - cost).abs() < 1e-12);
        assert!((estimate.estimated_weekly_value_usd - cost * 4.0).abs() < 1e-12);
    }

    #[test]
    fn reserve_only_week_has_no_subscription_usage_to_estimate() {
        let home = tempdir().unwrap();
        fs::create_dir_all(home.path().join("sessions")).unwrap();
        fs::write(
            home.path().join("sessions/reserve.jsonl"),
            usage_event("2026-08-21T09:00:00Z", 1000, 1000).replace("gpt-5", "gpt-reserve"),
        )
        .unwrap();
        let error = estimate_codex_weekly_value_with_pricing(
            &quota(),
            Some(home.path()),
            &PricingDb::default(),
        )
        .unwrap_err();
        assert!(matches!(error, CodexWeeklyValueError::NoUsageInWindow));
    }

    #[test]
    fn estimate_models_follow_all_contributors_not_the_last_selected_model() {
        let home = tempdir().unwrap();
        fs::create_dir_all(home.path().join("sessions")).unwrap();
        let first = usage_event("2026-08-21T08:00:00Z", 100, 100).replace("gpt-5", "gpt-5.4");
        let second = usage_event("2026-08-21T09:00:00Z", 300, 200);
        fs::write(
            home.path().join("sessions/mixed.jsonl"),
            format!("{first}\n{second}\n"),
        )
        .unwrap();
        let estimate = estimate_codex_weekly_value_with_pricing(
            &quota(),
            Some(home.path()),
            &PricingDb::default(),
        )
        .unwrap();
        assert_eq!(
            estimate
                .model_estimates
                .iter()
                .map(|entry| entry.model.as_str())
                .collect::<Vec<_>>(),
            vec!["gpt-5", "gpt-5.4"]
        );
        assert!(
            estimate
                .model_estimates
                .iter()
                .all(|entry| entry.estimated_weekly_tokens.is_none())
        );
        assert_eq!(estimate.observed_tokens, 300);
    }

    #[test]
    fn regular_luna_and_unknown_models_are_not_excluded() {
        let luna = usage_event("2026-08-21T08:00:00Z", 100, 100).replace("gpt-5", "gpt-5.6-luna");
        let unknown =
            usage_event("2026-08-21T09:00:00Z", 300, 200).replace("gpt-5", "unpriced-test-model");
        let file = write_log(&[&luna, &unknown]);
        let quota = quota();
        let usage = load_weekly_window_usage_from_files(
            quota.observed_at,
            quota.resets_at - Duration::minutes(quota.window_minutes),
            quota.resets_at,
            &[file.path().to_path_buf()],
        )
        .unwrap();
        assert_eq!(usage.stats.total_tokens(), 300);
        assert_eq!(usage.models["gpt-5.6-luna"].total_tokens(), 100);
        assert_eq!(usage.models["unpriced-test-model"].total_tokens(), 200);
    }

    #[test]
    fn malformed_usage_fails_value_estimate_closed() {
        let valid = usage_event("2026-08-21T00:00:00Z", 100, 100);
        let file = write_log(&[&valid, "{malformed"]);

        let quota = quota();
        let error = load_weekly_window_usage_from_files(
            quota.observed_at,
            quota.resets_at - Duration::minutes(quota.window_minutes),
            quota.resets_at,
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
            quota.resets_at,
            &[file.path().to_path_buf()],
        )
        .unwrap();

        assert!(usage.models.contains_key("unknown-model"));
        assert!(!usage.models.contains_key("gpt-5"));
    }

    fn sample_event(hour: u32, model: &str, total: i64, delta: i64, pct: Option<f64>) -> String {
        let timestamp = format!("2026-08-21T{hour:02}:00:00Z");
        let mut row: serde_json::Value =
            serde_json::from_str(&usage_event(&timestamp, total, delta)).unwrap();
        row["payload"]["model"] = json!(model);
        if let Some(pct) = pct {
            row["payload"]["rate_limits"] = json!({
                "limit_id": "codex",
                "secondary": {"used_percent": pct, "window_minutes": 10080, "resets_at": quota().resets_at.timestamp()}
            });
        }
        row.to_string()
    }

    fn samples_for(lines: &[String]) -> CodexWindowUsage {
        let file = write_log(&lines.iter().map(String::as_str).collect::<Vec<_>>());
        let quota = quota();
        load_weekly_window_usage_from_files(
            quota.observed_at,
            quota.resets_at - Duration::minutes(quota.window_minutes),
            quota.resets_at,
            &[file.path().to_path_buf()],
        )
        .unwrap()
    }

    #[test]
    fn separate_single_model_spans_yield_independent_full_week_capacities() {
        let usage = samples_for(&[
            sample_event(0, "gpt-6-astra", 0, 0, Some(0.0)),
            sample_event(1, "gpt-6-astra", 90_000_000, 90_000_000, Some(10.0)),
            sample_event(2, "gpt-5.6-sol", 290_000_000, 200_000_000, Some(20.0)),
        ]);
        let astra = &usage.model_samples["gpt-6-astra"];
        let sol = &usage.model_samples["gpt-5.6-sol"];
        assert!((astra.tokens as f64 * 100.0 / astra.used_pct - 900_000_000.0).abs() < 1e-6);
        assert!((sol.tokens as f64 * 100.0 / sol.used_pct - 2_000_000_000.0).abs() < 1e-6);
        assert_eq!(usage.stats.total_tokens(), 290_000_000);
    }

    #[test]
    fn mixed_intervals_and_quota_consumed_without_local_tokens_are_not_attributed() {
        let usage = samples_for(&[
            sample_event(0, "gpt-6-astra", 0, 0, Some(0.0)),
            sample_event(1, "gpt-6-astra", 10, 10, None),
            sample_event(2, "gpt-5.6-sol", 20, 10, Some(10.0)),
            sample_event(3, "gpt-5.6-sol", 20, 0, Some(20.0)),
        ]);
        assert!(usage.model_samples.is_empty());
        assert_eq!(usage.models.len(), 2);
    }

    #[test]
    fn rounded_plateaus_accumulate_but_small_samples_resets_and_saturation_do_not() {
        let usage = samples_for(&[
            sample_event(0, "gpt-6-astra", 0, 0, Some(0.0)),
            sample_event(1, "gpt-6-astra", 100, 100, Some(1.0)),
            sample_event(2, "gpt-6-astra", 200, 100, Some(1.0)),
            sample_event(3, "gpt-6-astra", 500, 300, Some(5.0)),
        ]);
        assert_eq!(usage.model_samples["gpt-6-astra"].tokens, 500);
        assert!((usage.model_samples["gpt-6-astra"].used_pct - 5.0).abs() < 1e-6);
        for percentages in [
            [0.0, 1.0, 4.0],
            [10.0, 14.0, 1.0],
            [0.0, 0.0, 100.0],
            [0.0, 10.0, 9.0],
        ] {
            let usage = samples_for(
                &percentages
                    .into_iter()
                    .enumerate()
                    .map(|(hour, pct)| {
                        sample_event(
                            hour as u32,
                            "gpt-6-astra",
                            hour as i64 * 100,
                            100,
                            Some(pct),
                        )
                    })
                    .collect::<Vec<_>>(),
            );
            assert!(usage.model_samples.is_empty(), "{percentages:?}");
        }
    }

    #[test]
    fn reserve_tokens_and_independent_pools_do_not_inflate_samples() {
        let foreign = sample_event(2, "gpt-reserve", 1100, 1000, Some(90.0))
            .replace("\"codex\"", "\"codex_bengalfox\"");
        let usage = samples_for(&[
            sample_event(0, "gpt-6-astra", 0, 0, Some(0.0)),
            sample_event(1, "gpt-6-astra", 100, 100, Some(5.0)),
            foreign,
            sample_event(3, "gpt-6-astra", 1200, 100, Some(10.0)),
        ]);
        assert!(!usage.models.contains_key("gpt-reserve"));
        assert_eq!(usage.model_samples.len(), 1);
        assert_eq!(usage.model_samples["gpt-6-astra"].tokens, 200);
        assert!((usage.model_samples["gpt-6-astra"].used_pct - 10.0).abs() < 1e-6);
    }

    #[test]
    fn quota_snapshots_from_another_week_cannot_calibrate_this_week() {
        let usage = samples_for(&[
            sample_event(0, "gpt-6-astra", 0, 0, Some(0.0)),
            sample_event(1, "gpt-6-astra", 100, 100, Some(10.0)).replace(
                &quota().resets_at.timestamp().to_string(),
                &(quota().resets_at + Duration::days(7))
                    .timestamp()
                    .to_string(),
            ),
        ]);
        assert!(usage.model_samples.is_empty());
    }

    #[test]
    fn concurrent_models_across_session_files_are_not_treated_as_separate_samples() {
        let astra = write_log(&[
            &sample_event(0, "gpt-6-astra", 0, 0, Some(0.0)),
            &sample_event(2, "gpt-6-astra", 100, 100, Some(10.0)),
        ]);
        let sol = write_log(&[&sample_event(1, "gpt-5.6-sol", 100, 100, None)]);
        let quota = quota();
        let usage = load_weekly_window_usage_from_files(
            quota.observed_at,
            quota.resets_at - Duration::days(7),
            quota.resets_at,
            &[
                astra.path().to_path_buf(),
                sol.path().to_path_buf(),
                astra.path().to_path_buf(),
            ],
        )
        .unwrap();
        assert!(usage.model_samples.is_empty());
        assert_eq!(usage.stats.total_tokens(), 200);
        assert!(usage.dedup_skipped_entries > 0);
    }

    #[test]
    fn conflicting_simultaneous_snapshots_cannot_bound_a_sample() {
        let usage = samples_for(&[
            sample_event(0, "gpt-6-astra", 0, 0, Some(0.0)),
            sample_event(1, "gpt-6-astra", 100, 100, Some(5.0)),
            sample_event(1, "gpt-6-astra", 100, 0, Some(50.0)),
            sample_event(2, "gpt-6-astra", 200, 100, Some(55.0)),
        ]);
        assert!(usage.model_samples.is_empty());
    }

    #[test]
    fn sdk_exposes_sample_based_capacities_independently_of_mixed_week_totals() {
        let home = tempdir().unwrap();
        fs::create_dir_all(home.path().join("sessions")).unwrap();
        fs::write(
            home.path().join("sessions/models.jsonl"),
            [
                sample_event(0, "gpt-5", 0, 0, Some(0.0)),
                sample_event(1, "gpt-5", 90_000_000, 90_000_000, Some(10.0)),
                sample_event(2, "gpt-5.4", 290_000_000, 200_000_000, Some(20.0)),
            ]
            .join("\n"),
        )
        .unwrap();
        let estimate = estimate_codex_weekly_value_with_pricing(
            &quota(),
            Some(home.path()),
            &PricingDb::default(),
        )
        .unwrap();
        assert_eq!(
            estimate.model_estimates[0].estimated_weekly_tokens,
            Some(900_000_000.0)
        );
        assert_eq!(
            estimate.model_estimates[1].estimated_weekly_tokens,
            Some(2_000_000_000.0)
        );
        assert_eq!(estimate.model_estimates[1].sample_tokens, 200_000_000);
        assert!((estimate.model_estimates[1].sample_used_pct - 10.0).abs() < 1e-6);
        assert!((estimate.estimated_weekly_tokens - 1_160_000_000.0).abs() < 1e-6);
    }
}
