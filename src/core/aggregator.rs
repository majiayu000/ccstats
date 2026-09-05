//! Unified aggregation logic for all data sources
//!
//! Converts raw entries into various aggregated views (daily, session, etc.)

use chrono::{DateTime, Utc};
use std::collections::HashMap;

use crate::core::types::{
    BlockStats, DayStats, Endpoint, EndpointStats, ProjectStats, RawEntry, SessionStats, Stats,
};
use crate::utils::Timezone;

/// Aggregate entries by day (consumes entries to avoid cloning)
pub(crate) fn aggregate_daily(entries: Vec<RawEntry>) -> HashMap<String, DayStats> {
    let mut day_stats: HashMap<String, DayStats> = HashMap::new();

    for entry in entries {
        let stats = entry.to_stats();
        let day = day_stats.entry(entry.date_str).or_default();
        day.add_stats(entry.model, &stats);
    }

    day_stats
}

/// Aggregate entries by serving endpoint (native vs proxy vs unknown).
/// Returns present endpoints in canonical order (native, proxy, unknown).
pub(crate) fn aggregate_by_endpoint(entries: Vec<RawEntry>) -> Vec<EndpointStats> {
    let mut map: HashMap<Endpoint, EndpointStats> = HashMap::new();
    for entry in entries {
        let stats = entry.to_stats();
        let acc = map.entry(entry.endpoint).or_default();
        acc.endpoint = entry.endpoint;
        acc.stats.add(&stats);
        acc.models.entry(entry.model).or_default().add(&stats);
    }
    Endpoint::ORDER
        .iter()
        .filter_map(|ep| map.remove(ep))
        .collect()
}

pub(crate) fn merge_day_stats(
    target: &mut HashMap<String, DayStats>,
    source: HashMap<String, DayStats>,
) {
    for (date, stats) in source {
        let day = target.entry(date).or_default();
        day.stats.add(&stats.stats);
        for (model, model_stats) in stats.models {
            day.models.entry(model).or_default().add(&model_stats);
        }
    }
}

/// Session accumulator for building session stats
#[derive(Debug, Default)]
struct SessionAccumulator {
    session_key: String,
    session_id: String,
    project_path: String,
    first_timestamp: String,
    last_timestamp: String,
    first_timestamp_ms: i64,
    last_timestamp_ms: i64,
    stats: Stats,
    models: HashMap<String, Stats>,
}

impl SessionAccumulator {
    fn new(
        session_key: String,
        session_id: String,
        project_path: String,
        timestamp: &str,
        timestamp_ms: i64,
    ) -> Self {
        SessionAccumulator {
            session_key,
            session_id,
            project_path,
            first_timestamp: timestamp.to_string(),
            last_timestamp: timestamp.to_string(),
            first_timestamp_ms: timestamp_ms,
            last_timestamp_ms: timestamp_ms,
            stats: Stats::default(),
            models: HashMap::new(),
        }
    }

    fn add_entry(&mut self, entry: &RawEntry) {
        let timestamp = entry.timestamp.clone();
        let timestamp_ms = entry.timestamp_ms;
        let model = entry.model.clone();
        let stats = entry.to_stats();
        self.stats.add(&stats);
        self.models.entry(model).or_default().add(&stats);
        self.update_timestamps(timestamp, timestamp_ms);
    }

    fn update_timestamps(&mut self, timestamp: String, timestamp_ms: i64) {
        let update_first =
            self.first_timestamp.is_empty() || timestamp_ms < self.first_timestamp_ms;
        let update_last = self.last_timestamp.is_empty() || timestamp_ms > self.last_timestamp_ms;

        if update_first {
            self.first_timestamp.clone_from(&timestamp);
            self.first_timestamp_ms = timestamp_ms;
        }
        if update_last {
            self.last_timestamp = timestamp;
            self.last_timestamp_ms = timestamp_ms;
        }
    }

    fn into_session_stats(self) -> SessionStats {
        SessionStats {
            session_key: self.session_key,
            session_id: self.session_id,
            project_path: self.project_path,
            first_timestamp: self.first_timestamp,
            last_timestamp: self.last_timestamp,
            stats: self.stats,
            models: self.models,
        }
    }
}

/// Aggregate entries by session (consumes entries to avoid cloning)
pub(crate) fn aggregate_sessions(entries: Vec<RawEntry>) -> Vec<SessionStats> {
    aggregate_sessions_map(entries).into_values().collect()
}

/// Aggregate entries by session into a map keyed by stable internal session key.
pub(crate) fn aggregate_sessions_map(entries: Vec<RawEntry>) -> HashMap<String, SessionStats> {
    let mut sessions: HashMap<String, SessionAccumulator> = HashMap::with_capacity(entries.len());

    for entry in entries {
        let session_key = if entry.session_key.is_empty() {
            entry.session_id.clone()
        } else {
            entry.session_key.clone()
        };
        let session = sessions.entry(session_key.clone()).or_insert_with(|| {
            SessionAccumulator::new(
                session_key,
                entry.session_id.clone(),
                entry.project_path.clone(),
                &entry.timestamp,
                entry.timestamp_ms,
            )
        });
        session.add_entry(&entry);
    }

    sessions
        .into_iter()
        .map(|(session_key, acc)| (session_key, acc.into_session_stats()))
        .collect()
}

/// Aggregate sessions by project (consumes sessions to avoid cloning)
pub(crate) fn aggregate_projects(sessions: Vec<SessionStats>) -> Vec<ProjectStats> {
    let mut project_map: HashMap<String, ProjectStats> = HashMap::new();

    for session in sessions {
        let project_path = session.project_path; // move, not clone
        let project = project_map
            .entry(project_path.clone()) // one clone for HashMap key
            .or_insert_with(|| ProjectStats {
                project_name: format_project_name(&project_path),
                project_path,
                session_count: 0,
                stats: Stats::default(),
                models: HashMap::new(),
            });

        project.session_count += 1;
        project.stats.add(&session.stats);

        for (model, model_stats) in session.models {
            project.models.entry(model).or_default().add(&model_stats);
        }
    }

    let mut projects: Vec<ProjectStats> = project_map.into_values().collect();
    projects.sort_by_key(|project| std::cmp::Reverse(project.stats.total_tokens()));
    projects
}

/// Extract readable project name from encoded path
pub(crate) fn format_project_name(path: &str) -> String {
    if path.contains('/') || path.contains('\\') {
        return std::path::Path::new(path)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(path)
            .to_string();
    }

    path.trim_start_matches('-').to_string()
}

/// Activity-driven 5-hour estimated session window (ccusage `identifySessionBlocks`).
///
/// Inferred from local logs. Not an official Anthropic billing reset.
const SESSION_WINDOW_MS: i64 = 5 * 60 * 60 * 1000;
const UTC_HOUR_MS: i64 = 60 * 60 * 1000;

/// Floor a Unix-ms timestamp to the start of its UTC hour (`setUTCMinutes(0,0,0)`).
fn floor_to_utc_hour_ms(timestamp_ms: i64) -> i64 {
    timestamp_ms.div_euclid(UTC_HOUR_MS) * UTC_HOUR_MS
}

fn local_from_utc_ms(utc_ms: i64, timezone: Timezone) -> DateTime<chrono::FixedOffset> {
    let utc = DateTime::<Utc>::from_timestamp_millis(utc_ms).unwrap_or(DateTime::<Utc>::UNIX_EPOCH);
    timezone.to_fixed_offset(utc)
}

fn into_block_stats(
    start_ms: i64,
    timezone: Timezone,
    stats: Stats,
    models: HashMap<String, Stats>,
) -> BlockStats {
    let local_start = local_from_utc_ms(start_ms, timezone);
    let local_end = local_from_utc_ms(start_ms + SESSION_WINDOW_MS, timezone);
    BlockStats {
        block_start: local_start.format("%Y-%m-%d %H:%M").to_string(),
        block_end: local_end.format("%H:%M").to_string(),
        stats,
        models,
    }
}

/// Aggregate entries into activity-driven 5-hour estimated session windows.
///
/// Matches ccusage `identifySessionBlocks` / `floorToHour`: sort by timestamp,
/// floor the first activity to the UTC hour, lasts 5 hours from that start, and
/// open a new window when an entry is more than 5 hours after the start **or**
/// more than 5 hours after the previous entry. Does not emit gap placeholders.
///
/// Labels convert the floored UTC start/end independently in `timezone`.
/// Not an official Anthropic billing reset.
pub(crate) fn aggregate_blocks(mut entries: Vec<RawEntry>, timezone: Timezone) -> Vec<BlockStats> {
    entries.sort_by_key(|entry| entry.timestamp_ms);

    let mut blocks = Vec::new();
    let mut current: Option<(i64, i64)> = None; // start_ms, last_ms
    let mut stats = Stats::default();
    let mut models: HashMap<String, Stats> = HashMap::new();

    for entry in entries {
        if DateTime::<Utc>::from_timestamp_millis(entry.timestamp_ms).is_none() {
            continue;
        }
        let entry_stats = entry.to_stats();

        let start_new = match current {
            None => true,
            Some((start_ms, last_ms)) => {
                entry.timestamp_ms - start_ms > SESSION_WINDOW_MS
                    || entry.timestamp_ms - last_ms > SESSION_WINDOW_MS
            }
        };

        if start_new {
            if let Some((start_ms, _)) = current {
                blocks.push(into_block_stats(
                    start_ms,
                    timezone,
                    std::mem::take(&mut stats),
                    std::mem::take(&mut models),
                ));
            }
            current = Some((floor_to_utc_hour_ms(entry.timestamp_ms), entry.timestamp_ms));
        } else if let Some((_, last_ms)) = current.as_mut() {
            *last_ms = entry.timestamp_ms;
        }

        stats.add(&entry_stats);
        models.entry(entry.model).or_default().add(&entry_stats);
    }

    if let Some((start_ms, _)) = current {
        blocks.push(into_block_stats(start_ms, timezone, stats, models));
    }
    blocks
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn make_entry(
        date: &str,
        session: &str,
        project: &str,
        model: &str,
        input: i64,
        output: i64,
        ts_ms: i64,
    ) -> RawEntry {
        RawEntry {
            timestamp: format!("2025-01-01T{:02}:00:00Z", ts_ms / 3_600_000 % 24),
            timestamp_ms: ts_ms,
            date_str: date.to_string(),
            message_id: None,
            session_key: session.to_string(),
            session_id: session.to_string(),
            project_path: project.to_string(),
            model: model.to_string(),
            input_tokens: input,
            output_tokens: output,
            cache_creation: 0,
            cache_creation_1h: 0,
            cache_read: 0,
            reasoning_tokens: 0,
            stop_reason: Some("end_turn".to_string()),
            cost_kind: crate::core::CostKind::Real,
            endpoint: Endpoint::Unknown,
            call_count: 1,
            reported_total_tokens: None,
            recorded_cost_usd: None,
            api_equivalent_priced_tokens: 0,
            api_equivalent_coverage_tokens: 0,
        }
    }

    // --- merge_day_stats ---

    mod merge_day_stats {
        use super::*;

        fn make_day_stats(model: &str, input: i64, count: i64) -> DayStats {
            let mut ds = DayStats::default();
            let stats = Stats {
                input_tokens: input,
                count,
                ..Default::default()
            };
            ds.stats.add(&stats);
            ds.models.entry(model.to_string()).or_default().add(&stats);
            ds
        }

        #[test]
        fn disjoint_dates() {
            let mut target = HashMap::new();
            target.insert("2025-01-01".to_string(), make_day_stats("gpt-4", 100, 1));

            let mut source = HashMap::new();
            source.insert("2025-01-02".to_string(), make_day_stats("gpt-4", 200, 2));

            super::merge_day_stats(&mut target, source);
            assert_eq!(target.len(), 2);
            assert_eq!(target["2025-01-01"].stats.input_tokens, 100);
            assert_eq!(target["2025-01-02"].stats.input_tokens, 200);
        }

        #[test]
        fn overlapping_dates_accumulates() {
            let mut target = HashMap::new();
            target.insert("2025-01-01".to_string(), make_day_stats("gpt-4", 100, 1));

            let mut source = HashMap::new();
            source.insert("2025-01-01".to_string(), make_day_stats("gpt-4", 200, 2));

            super::merge_day_stats(&mut target, source);
            assert_eq!(target.len(), 1);
            assert_eq!(target["2025-01-01"].stats.input_tokens, 300);
            assert_eq!(target["2025-01-01"].stats.count, 3);
            assert_eq!(target["2025-01-01"].models["gpt-4"].input_tokens, 300);
        }

        #[test]
        fn different_models_preserved() {
            let mut target = HashMap::new();
            target.insert("2025-01-01".to_string(), make_day_stats("gpt-4", 100, 1));

            let mut source = HashMap::new();
            source.insert("2025-01-01".to_string(), make_day_stats("claude", 200, 2));

            super::merge_day_stats(&mut target, source);
            assert_eq!(target["2025-01-01"].models.len(), 2);
            assert_eq!(target["2025-01-01"].models["gpt-4"].input_tokens, 100);
            assert_eq!(target["2025-01-01"].models["claude"].input_tokens, 200);
            assert_eq!(target["2025-01-01"].stats.input_tokens, 300);
        }

        #[test]
        fn empty_source() {
            let mut target = HashMap::new();
            target.insert("2025-01-01".to_string(), make_day_stats("m", 100, 1));

            super::merge_day_stats(&mut target, HashMap::new());
            assert_eq!(target.len(), 1);
        }
    }

    // --- format_project_name ---

    #[test]
    fn format_project_name_encoded_path() {
        assert_eq!(
            format_project_name("-Users-john-projects-myapp"),
            "Users-john-projects-myapp"
        );
    }

    #[test]
    fn format_project_name_simple() {
        assert_eq!(format_project_name("simple"), "simple");
    }

    #[test]
    fn format_project_name_unix_path() {
        assert_eq!(
            format_project_name("/Users/john/projects/my-project"),
            "my-project"
        );
    }

    #[test]
    fn format_project_name_with_backslash() {
        // On Unix, backslash is not a path separator, so Path treats the whole
        // string as a filename. The function still enters the Path branch
        // because it detects '\\', but file_name() returns the full string.
        let result = format_project_name("C:\\Users\\john\\projects\\app");
        // On Windows this would be "app", on Unix it's the full string
        assert!(!result.is_empty());
    }

    #[test]
    fn format_project_name_empty() {
        assert_eq!(format_project_name(""), "");
    }

    #[test]
    fn format_project_name_leading_dashes() {
        assert_eq!(format_project_name("---foo"), "foo");
    }

    // --- aggregate_daily ---

    #[test]
    fn aggregate_daily_empty() {
        let result = aggregate_daily(vec![]);
        assert!(result.is_empty());
    }

    #[test]
    fn aggregate_daily_single_entry() {
        let entries = vec![make_entry(
            "2025-01-01",
            "s1",
            "p1",
            "claude",
            100,
            50,
            1000,
        )];
        let result = aggregate_daily(entries);
        assert_eq!(result.len(), 1);
        let day = &result["2025-01-01"];
        assert_eq!(day.stats.input_tokens, 100);
        assert_eq!(day.stats.output_tokens, 50);
        assert_eq!(day.stats.count, 1);
    }

    #[test]
    fn aggregate_daily_multiple_days() {
        let entries = vec![
            make_entry("2025-01-01", "s1", "p1", "claude", 100, 50, 1000),
            make_entry("2025-01-02", "s1", "p1", "claude", 200, 100, 2000),
        ];
        let result = aggregate_daily(entries);
        assert_eq!(result.len(), 2);
        assert_eq!(result["2025-01-01"].stats.input_tokens, 100);
        assert_eq!(result["2025-01-02"].stats.input_tokens, 200);
    }

    #[test]
    fn aggregate_daily_same_day_different_models() {
        let entries = vec![
            make_entry("2025-01-01", "s1", "p1", "claude", 100, 50, 1000),
            make_entry("2025-01-01", "s1", "p1", "gpt-4", 200, 100, 2000),
        ];
        let result = aggregate_daily(entries);
        assert_eq!(result.len(), 1);
        let day = &result["2025-01-01"];
        assert_eq!(day.stats.input_tokens, 300);
        assert_eq!(day.stats.count, 2);
        assert_eq!(day.models.len(), 2);
        assert_eq!(day.models["claude"].input_tokens, 100);
        assert_eq!(day.models["gpt-4"].input_tokens, 200);
    }

    #[test]
    fn aggregate_daily_same_model_accumulates() {
        let entries = vec![
            make_entry("2025-01-01", "s1", "p1", "claude", 100, 50, 1000),
            make_entry("2025-01-01", "s2", "p1", "claude", 150, 75, 2000),
        ];
        let result = aggregate_daily(entries);
        let day = &result["2025-01-01"];
        assert_eq!(day.stats.input_tokens, 250);
        assert_eq!(day.models["claude"].input_tokens, 250);
        assert_eq!(day.models["claude"].count, 2);
    }

    // --- aggregate_sessions ---

    #[test]
    fn aggregate_sessions_empty() {
        let result = aggregate_sessions(vec![]);
        assert!(result.is_empty());
    }

    #[test]
    fn aggregate_sessions_single_session() {
        let entries = vec![
            make_entry("2025-01-01", "s1", "/path/proj", "claude", 100, 50, 1000),
            make_entry("2025-01-01", "s1", "/path/proj", "claude", 200, 100, 5000),
        ];
        let result = aggregate_sessions(entries);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].session_id, "s1");
        assert_eq!(result[0].project_path, "/path/proj");
        assert_eq!(result[0].stats.input_tokens, 300);
        assert_eq!(result[0].stats.count, 2);
    }

    #[test]
    fn aggregate_sessions_tracks_min_max_timestamps() {
        let entries = vec![
            RawEntry {
                timestamp: "2025-01-01T12:00:00Z".to_string(),
                timestamp_ms: 5000,
                date_str: "2025-01-01".to_string(),
                message_id: None,
                session_key: "s1".to_string(),
                session_id: "s1".to_string(),
                project_path: "p1".to_string(),
                model: "claude".to_string(),
                input_tokens: 100,
                output_tokens: 50,
                cache_creation: 0,
                cache_creation_1h: 0,
                cache_read: 0,
                reasoning_tokens: 0,
                stop_reason: None,
                cost_kind: crate::core::CostKind::Real,
                endpoint: Endpoint::Unknown,
                call_count: 1,
                reported_total_tokens: None,
                recorded_cost_usd: None,
                api_equivalent_priced_tokens: 0,
                api_equivalent_coverage_tokens: 0,
            },
            RawEntry {
                timestamp: "2025-01-01T08:00:00Z".to_string(),
                timestamp_ms: 1000,
                date_str: "2025-01-01".to_string(),
                message_id: None,
                session_key: "s1".to_string(),
                session_id: "s1".to_string(),
                project_path: "p1".to_string(),
                model: "claude".to_string(),
                input_tokens: 100,
                output_tokens: 50,
                cache_creation: 0,
                cache_creation_1h: 0,
                cache_read: 0,
                reasoning_tokens: 0,
                stop_reason: None,
                cost_kind: crate::core::CostKind::Real,
                endpoint: Endpoint::Unknown,
                call_count: 1,
                reported_total_tokens: None,
                recorded_cost_usd: None,
                api_equivalent_priced_tokens: 0,
                api_equivalent_coverage_tokens: 0,
            },
            RawEntry {
                timestamp: "2025-01-01T20:00:00Z".to_string(),
                timestamp_ms: 9000,
                date_str: "2025-01-01".to_string(),
                message_id: None,
                session_key: "s1".to_string(),
                session_id: "s1".to_string(),
                project_path: "p1".to_string(),
                model: "claude".to_string(),
                input_tokens: 100,
                output_tokens: 50,
                cache_creation: 0,
                cache_creation_1h: 0,
                cache_read: 0,
                reasoning_tokens: 0,
                stop_reason: None,
                cost_kind: crate::core::CostKind::Real,
                endpoint: Endpoint::Unknown,
                call_count: 1,
                reported_total_tokens: None,
                recorded_cost_usd: None,
                api_equivalent_priced_tokens: 0,
                api_equivalent_coverage_tokens: 0,
            },
        ];
        let result = aggregate_sessions(entries);
        assert_eq!(result[0].first_timestamp, "2025-01-01T08:00:00Z");
        assert_eq!(result[0].last_timestamp, "2025-01-01T20:00:00Z");
    }

    #[test]
    fn aggregate_sessions_multiple_sessions() {
        let entries = vec![
            make_entry("2025-01-01", "s1", "p1", "claude", 100, 50, 1000),
            make_entry("2025-01-01", "s2", "p2", "gpt-4", 200, 100, 2000),
        ];
        let result = aggregate_sessions(entries);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn aggregate_sessions_model_breakdown() {
        let entries = vec![
            make_entry("2025-01-01", "s1", "p1", "claude", 100, 50, 1000),
            make_entry("2025-01-01", "s1", "p1", "gpt-4", 200, 100, 2000),
        ];
        let result = aggregate_sessions(entries);
        assert_eq!(result[0].models.len(), 2);
        assert_eq!(result[0].models["claude"].input_tokens, 100);
        assert_eq!(result[0].models["gpt-4"].input_tokens, 200);
    }

    // --- aggregate_projects ---

    #[test]
    fn aggregate_projects_empty() {
        let result = aggregate_projects(vec![]);
        assert!(result.is_empty());
    }

    #[test]
    fn aggregate_projects_single_project() {
        let sessions = vec![SessionStats {
            session_key: "s1".to_string(),
            session_id: "s1".to_string(),
            project_path: "/Users/john/myapp".to_string(),
            first_timestamp: "t1".to_string(),
            last_timestamp: "t2".to_string(),
            stats: Stats {
                input_tokens: 100,
                output_tokens: 50,
                count: 1,
                ..Default::default()
            },
            models: HashMap::from([(
                "claude".to_string(),
                Stats {
                    input_tokens: 100,
                    output_tokens: 50,
                    count: 1,
                    ..Default::default()
                },
            )]),
        }];
        let result = aggregate_projects(sessions);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].project_name, "myapp");
        assert_eq!(result[0].session_count, 1);
        assert_eq!(result[0].stats.input_tokens, 100);
    }

    #[test]
    fn aggregate_projects_merges_sessions() {
        let sessions = vec![
            SessionStats {
                session_id: "s1".to_string(),
                project_path: "/path/app".to_string(),
                stats: Stats {
                    input_tokens: 100,
                    count: 1,
                    ..Default::default()
                },
                models: HashMap::from([(
                    "claude".to_string(),
                    Stats {
                        input_tokens: 100,
                        count: 1,
                        ..Default::default()
                    },
                )]),
                ..Default::default()
            },
            SessionStats {
                session_id: "s2".to_string(),
                project_path: "/path/app".to_string(),
                stats: Stats {
                    input_tokens: 200,
                    count: 2,
                    ..Default::default()
                },
                models: HashMap::from([(
                    "claude".to_string(),
                    Stats {
                        input_tokens: 200,
                        count: 2,
                        ..Default::default()
                    },
                )]),
                ..Default::default()
            },
        ];
        let result = aggregate_projects(sessions);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].session_count, 2);
        assert_eq!(result[0].stats.input_tokens, 300);
        assert_eq!(result[0].models["claude"].input_tokens, 300);
    }

    #[test]
    fn aggregate_projects_sorted_by_total_tokens_desc() {
        let sessions = vec![
            SessionStats {
                session_id: "s1".to_string(),
                project_path: "/path/small".to_string(),
                stats: Stats {
                    input_tokens: 10,
                    ..Default::default()
                },
                models: HashMap::new(),
                ..Default::default()
            },
            SessionStats {
                session_id: "s2".to_string(),
                project_path: "/path/big".to_string(),
                stats: Stats {
                    input_tokens: 1000,
                    ..Default::default()
                },
                models: HashMap::new(),
                ..Default::default()
            },
        ];
        let result = aggregate_projects(sessions);
        assert_eq!(result[0].project_name, "big");
        assert_eq!(result[1].project_name, "small");
    }

    // --- aggregate_blocks (activity-driven session windows) ---

    fn utc(hour: u32, min: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2025, 1, 1, hour, min, 0).unwrap()
    }

    fn utc_tz() -> Timezone {
        Timezone::parse(Some("UTC")).unwrap()
    }

    fn entry_at(dt: DateTime<Utc>, input: i64) -> RawEntry {
        make_entry(
            "2025-01-01",
            "s1",
            "p1",
            "claude",
            input,
            50,
            dt.timestamp_millis(),
        )
    }

    #[test]
    fn aggregate_blocks_empty() {
        let result = aggregate_blocks(vec![], utc_tz());
        assert!(result.is_empty());
    }

    #[test]
    fn aggregate_blocks_skips_missing_timestamps() {
        let entries = vec![make_entry(
            "2025-01-01",
            "s1",
            "p1",
            "claude",
            100,
            50,
            i64::MAX,
        )];
        let result = aggregate_blocks(entries, utc_tz());
        assert!(result.is_empty());
    }

    #[test]
    fn aggregate_blocks_groups_by_5h_window() {
        // 02:30 floors to 02:00 (not clock-aligned 00:00). 06:00 stays in that
        // window even though it would have been a 05:00 clock bucket. 08:00
        // exceeds the 5h window and starts a new one at 08:00.
        let t1 = utc(2, 30);
        let t2 = utc(6, 0);
        let t3 = utc(8, 0);
        let entries = vec![entry_at(t1, 100), entry_at(t2, 200), entry_at(t3, 300)];

        let result = aggregate_blocks(entries, utc_tz());
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].block_start, "2025-01-01 02:00");
        assert_eq!(result[0].block_end, "07:00");
        assert_eq!(result[0].stats.input_tokens, 300);
        assert_eq!(result[1].block_start, "2025-01-01 08:00");
        assert_eq!(result[1].block_end, "13:00");
        assert_eq!(result[1].stats.input_tokens, 300);
    }

    #[test]
    fn aggregate_blocks_sorted_chronologically() {
        let early = utc(1, 0);
        let late = utc(22, 0);
        let entries = vec![entry_at(late, 100), entry_at(early, 100)];

        let result = aggregate_blocks(entries, utc_tz());
        assert_eq!(result[0].block_start, "2025-01-01 01:00");
        assert_eq!(result[1].block_start, "2025-01-01 22:00");
    }

    #[test]
    fn session_windows_same_utc_hour_floor() {
        // 10:55 UTC and 11:10 UTC same day → one window starting 10:00 UTC.
        let t1 = utc(10, 55);
        let t2 = utc(11, 10);
        let result = aggregate_blocks(vec![entry_at(t1, 100), entry_at(t2, 200)], utc_tz());
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].block_start, "2025-01-01 10:00");
        assert_eq!(result[0].block_end, "15:00");
        assert_eq!(result[0].stats.input_tokens, 300);
    }

    #[test]
    fn session_windows_split_after_five_hours_from_start() {
        // 10:55 UTC then 16:10 UTC (>5h after the 10:00 start) → two windows.
        let t1 = utc(10, 55);
        let t2 = utc(16, 10);
        let result = aggregate_blocks(vec![entry_at(t1, 100), entry_at(t2, 200)], utc_tz());
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].block_start, "2025-01-01 10:00");
        assert_eq!(result[0].stats.input_tokens, 100);
        assert_eq!(result[1].block_start, "2025-01-01 16:00");
        assert_eq!(result[1].stats.input_tokens, 200);
    }

    #[test]
    fn session_windows_split_on_gap_since_last_activity() {
        // 10:00 UTC then 16:30 UTC (6.5h gap since last activity) → two windows.
        let t1 = utc(10, 0);
        let t2 = utc(16, 30);
        let result = aggregate_blocks(vec![entry_at(t1, 100), entry_at(t2, 200)], utc_tz());
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].block_start, "2025-01-01 10:00");
        assert_eq!(result[1].block_start, "2025-01-01 16:00");
    }

    #[test]
    fn session_window_labels_convert_floored_utc_to_local() {
        let timezone = Timezone::parse(Some("Asia/Tokyo")).unwrap();
        let t1 = Utc.with_ymd_and_hms(2025, 6, 15, 10, 55, 0).unwrap();
        let result = aggregate_blocks(vec![entry_at(t1, 100)], timezone);
        // 10:00 UTC → 19:00 +09:00; window end 15:00 UTC → 00:00 local.
        assert_eq!(result[0].block_start, "2025-06-15 19:00");
        assert_eq!(result[0].block_end, "00:00");
    }

    #[test]
    fn session_window_labels_convert_start_and_end_independently_across_dst() {
        // DST ends 2025-11-02 02:00 America/New_York (07:00 UTC).
        // 04:30Z floors to 04:00Z = 00:00 EDT; window end 09:00Z = 04:00 EST.
        let timezone = Timezone::parse(Some("America/New_York")).unwrap();
        let t1 = Utc.with_ymd_and_hms(2025, 11, 2, 4, 30, 0).unwrap();
        let result = aggregate_blocks(vec![entry_at(t1, 100)], timezone);
        assert_eq!(result[0].block_start, "2025-11-02 00:00");
        assert_eq!(result[0].block_end, "04:00");
    }
}
