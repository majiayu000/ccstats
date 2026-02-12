//! Unified aggregation logic for all data sources
//!
//! Converts raw entries into various aggregated views (daily, session, etc.)

use chrono::{DateTime, Duration, FixedOffset, TimeZone, Timelike};
use std::collections::HashMap;

use crate::core::types::{BlockStats, DayStats, ProjectStats, RawEntry, SessionStats, Stats};

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

/// Session accumulator for building session stats
#[derive(Debug, Default)]
struct SessionAccumulator {
    project_path: String,
    first_timestamp: String,
    last_timestamp: String,
    first_timestamp_ms: i64,
    last_timestamp_ms: i64,
    stats: Stats,
    models: HashMap<String, Stats>,
}

impl SessionAccumulator {
    fn new(project_path: String, timestamp: &str, timestamp_ms: i64) -> Self {
        SessionAccumulator {
            project_path,
            first_timestamp: timestamp.to_string(),
            last_timestamp: timestamp.to_string(),
            first_timestamp_ms: timestamp_ms,
            last_timestamp_ms: timestamp_ms,
            stats: Stats::default(),
            models: HashMap::new(),
        }
    }

    fn add_entry(&mut self, entry: RawEntry) {
        let stats = entry.to_stats();
        self.stats.add(&stats);
        self.models.entry(entry.model).or_default().add(&stats);
        self.update_timestamps(&entry.timestamp, entry.timestamp_ms);
    }

    fn update_timestamps(&mut self, timestamp: &str, timestamp_ms: i64) {
        if self.first_timestamp.is_empty() || timestamp_ms < self.first_timestamp_ms {
            self.first_timestamp = timestamp.to_string();
            self.first_timestamp_ms = timestamp_ms;
        }
        if self.last_timestamp.is_empty() || timestamp_ms > self.last_timestamp_ms {
            self.last_timestamp = timestamp.to_string();
            self.last_timestamp_ms = timestamp_ms;
        }
    }
}

impl SessionAccumulator {
    fn into_session_stats(self, session_id: String) -> SessionStats {
        SessionStats {
            session_id,
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
    let mut sessions: HashMap<String, SessionAccumulator> = HashMap::new();

    for entry in entries {
        let session_id = entry.session_id.clone(); // one clone for HashMap key
        let session = sessions.entry(session_id).or_insert_with(|| {
            SessionAccumulator::new(
                entry.project_path.clone(),
                &entry.timestamp,
                entry.timestamp_ms,
            )
        });
        session.add_entry(entry);
    }

    sessions
        .into_iter()
        .map(|(id, acc)| acc.into_session_stats(id))
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
    projects.sort_by(|a, b| b.stats.total_tokens().cmp(&a.stats.total_tokens()));
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

/// Aggregate entries by 5-hour billing blocks (consumes entries to avoid cloning)
pub(crate) fn aggregate_blocks(
    entries: Vec<RawEntry>,
    local_times: &HashMap<i64, DateTime<FixedOffset>>,
) -> Vec<BlockStats> {
    let mut block_map: HashMap<DateTime<FixedOffset>, BlockStats> = HashMap::new();

    for entry in entries {
        let local_dt = match local_times.get(&entry.timestamp_ms) {
            Some(dt) => *dt,
            None => continue,
        };

        let stats = entry.to_stats();
        let block_start = get_block_start(local_dt);
        let block_end = block_start + Duration::hours(5);

        let block = block_map.entry(block_start).or_insert_with(|| BlockStats {
            block_start: block_start.format("%Y-%m-%d %H:%M").to_string(),
            block_end: block_end.format("%H:%M").to_string(),
            stats: Stats::default(),
            models: HashMap::new(),
        });

        block.stats.add(&stats);
        block.models.entry(entry.model).or_default().add(&stats);
    }

    let mut blocks: Vec<BlockStats> = block_map.into_values().collect();
    blocks.sort_by(|a, b| a.block_start.cmp(&b.block_start));
    blocks
}

/// Calculate the 5-hour block start time for a given timestamp
fn get_block_start(dt: DateTime<FixedOffset>) -> DateTime<FixedOffset> {
    let hour = i64::from(dt.hour());
    let block_hour = (hour / 5) * 5;
    let offset = *dt.offset();
    let naive = dt
        .date_naive()
        .and_hms_opt(block_hour as u32, 0, 0)
        .unwrap_or_else(|| dt.naive_utc());
    offset
        .from_local_datetime(&naive)
        .single()
        .unwrap_or_else(|| offset.from_utc_datetime(&naive))
}

#[cfg(test)]
mod tests {
    use super::*;

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
            session_id: session.to_string(),
            project_path: project.to_string(),
            model: model.to_string(),
            input_tokens: input,
            output_tokens: output,
            cache_creation: 0,
            cache_read: 0,
            reasoning_tokens: 0,
            stop_reason: Some("end_turn".to_string()),
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
                session_id: "s1".to_string(),
                project_path: "p1".to_string(),
                model: "claude".to_string(),
                input_tokens: 100,
                output_tokens: 50,
                cache_creation: 0,
                cache_read: 0,
                reasoning_tokens: 0,
                stop_reason: None,
            },
            RawEntry {
                timestamp: "2025-01-01T08:00:00Z".to_string(),
                timestamp_ms: 1000,
                date_str: "2025-01-01".to_string(),
                message_id: None,
                session_id: "s1".to_string(),
                project_path: "p1".to_string(),
                model: "claude".to_string(),
                input_tokens: 100,
                output_tokens: 50,
                cache_creation: 0,
                cache_read: 0,
                reasoning_tokens: 0,
                stop_reason: None,
            },
            RawEntry {
                timestamp: "2025-01-01T20:00:00Z".to_string(),
                timestamp_ms: 9000,
                date_str: "2025-01-01".to_string(),
                message_id: None,
                session_id: "s1".to_string(),
                project_path: "p1".to_string(),
                model: "claude".to_string(),
                input_tokens: 100,
                output_tokens: 50,
                cache_creation: 0,
                cache_read: 0,
                reasoning_tokens: 0,
                stop_reason: None,
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

    // --- aggregate_blocks ---

    #[test]
    fn aggregate_blocks_empty() {
        let result = aggregate_blocks(vec![], &HashMap::new());
        assert!(result.is_empty());
    }

    #[test]
    fn aggregate_blocks_skips_missing_timestamps() {
        let entries = vec![make_entry("2025-01-01", "s1", "p1", "claude", 100, 50, 999)];
        // local_times map doesn't contain ts_ms=999
        let result = aggregate_blocks(entries, &HashMap::new());
        assert!(result.is_empty());
    }

    #[test]
    fn aggregate_blocks_groups_by_5h_window() {
        let offset = FixedOffset::east_opt(0).unwrap();
        let dt1 = offset.with_ymd_and_hms(2025, 1, 1, 2, 30, 0).unwrap(); // block 0:00-5:00
        let dt2 = offset.with_ymd_and_hms(2025, 1, 1, 3, 0, 0).unwrap(); // same block
        let dt3 = offset.with_ymd_and_hms(2025, 1, 1, 7, 0, 0).unwrap(); // block 5:00-10:00

        let local_times: HashMap<i64, DateTime<FixedOffset>> =
            HashMap::from([(1000, dt1), (2000, dt2), (3000, dt3)]);

        let entries = vec![
            make_entry("2025-01-01", "s1", "p1", "claude", 100, 50, 1000),
            make_entry("2025-01-01", "s1", "p1", "claude", 200, 100, 2000),
            make_entry("2025-01-01", "s1", "p1", "claude", 300, 150, 3000),
        ];

        let result = aggregate_blocks(entries, &local_times);
        assert_eq!(result.len(), 2);
        // sorted by block_start
        assert!(result[0].block_start.contains("00:00"));
        assert_eq!(result[0].stats.input_tokens, 300); // 100+200
        assert!(result[1].block_start.contains("05:00"));
        assert_eq!(result[1].stats.input_tokens, 300);
    }

    #[test]
    fn aggregate_blocks_sorted_chronologically() {
        let offset = FixedOffset::east_opt(0).unwrap();
        let dt_late = offset.with_ymd_and_hms(2025, 1, 1, 22, 0, 0).unwrap();
        let dt_early = offset.with_ymd_and_hms(2025, 1, 1, 1, 0, 0).unwrap();

        let local_times: HashMap<i64, DateTime<FixedOffset>> =
            HashMap::from([(2000, dt_late), (1000, dt_early)]);

        let entries = vec![
            make_entry("2025-01-01", "s1", "p1", "claude", 100, 50, 2000),
            make_entry("2025-01-01", "s1", "p1", "claude", 100, 50, 1000),
        ];

        let result = aggregate_blocks(entries, &local_times);
        assert!(result[0].block_start < result[1].block_start);
    }

    // --- get_block_start ---

    #[test]
    fn get_block_start_boundaries() {
        let offset = FixedOffset::east_opt(0).unwrap();
        // Hour 0 → block 0
        let dt = offset.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
        assert_eq!(get_block_start(dt).hour(), 0);
        // Hour 4 → block 0
        let dt = offset.with_ymd_and_hms(2025, 1, 1, 4, 59, 59).unwrap();
        assert_eq!(get_block_start(dt).hour(), 0);
        // Hour 5 → block 5
        let dt = offset.with_ymd_and_hms(2025, 1, 1, 5, 0, 0).unwrap();
        assert_eq!(get_block_start(dt).hour(), 5);
        // Hour 23 → block 20
        let dt = offset.with_ymd_and_hms(2025, 1, 1, 23, 30, 0).unwrap();
        assert_eq!(get_block_start(dt).hour(), 20);
    }

    #[test]
    fn get_block_start_preserves_date_and_offset() {
        let offset = FixedOffset::east_opt(9 * 3600).unwrap(); // +09:00
        let dt = offset.with_ymd_and_hms(2025, 6, 15, 14, 30, 0).unwrap();
        let block = get_block_start(dt);
        assert_eq!(block.hour(), 10);
        assert_eq!(block.minute(), 0);
        assert_eq!(*block.offset(), offset);
    }
}
