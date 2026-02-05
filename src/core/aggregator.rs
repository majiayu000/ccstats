//! Unified aggregation logic for all data sources
//!
//! Converts raw entries into various aggregated views (daily, session, etc.)

use chrono::{DateTime, Duration, FixedOffset, TimeZone, Timelike};
use std::collections::HashMap;

use crate::core::types::{BlockStats, DayStats, ProjectStats, RawEntry, SessionStats, Stats};

/// Aggregate entries by day
pub fn aggregate_daily(entries: &[RawEntry]) -> HashMap<String, DayStats> {
    let mut day_stats: HashMap<String, DayStats> = HashMap::new();

    for entry in entries {
        let stats = entry.to_stats();
        let day = day_stats.entry(entry.date_str.clone()).or_default();
        day.add_stats(&entry.model, &stats);
    }

    day_stats
}

/// Session accumulator for building session stats
#[derive(Debug, Default)]
struct SessionAccumulator {
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
    fn new(session_id: String, project_path: String, timestamp: &str, timestamp_ms: i64) -> Self {
        SessionAccumulator {
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
        let stats = entry.to_stats();
        self.stats.add(&stats);
        self.models
            .entry(entry.model.clone())
            .or_default()
            .add(&stats);
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

impl From<SessionAccumulator> for SessionStats {
    fn from(acc: SessionAccumulator) -> Self {
        SessionStats {
            session_id: acc.session_id,
            project_path: acc.project_path,
            first_timestamp: acc.first_timestamp,
            last_timestamp: acc.last_timestamp,
            stats: acc.stats,
            models: acc.models,
        }
    }
}

/// Aggregate entries by session
pub fn aggregate_sessions(entries: &[RawEntry]) -> Vec<SessionStats> {
    let mut sessions: HashMap<String, SessionAccumulator> = HashMap::new();

    for entry in entries {
        let session = sessions
            .entry(entry.session_id.clone())
            .or_insert_with(|| {
                SessionAccumulator::new(
                    entry.session_id.clone(),
                    entry.project_path.clone(),
                    &entry.timestamp,
                    entry.timestamp_ms,
                )
            });
        session.add_entry(entry);
    }

    sessions.into_values().map(SessionStats::from).collect()
}

/// Aggregate sessions by project
pub fn aggregate_projects(sessions: &[SessionStats]) -> Vec<ProjectStats> {
    let mut project_map: HashMap<String, ProjectStats> = HashMap::new();

    for session in sessions {
        let project = project_map
            .entry(session.project_path.clone())
            .or_insert_with(|| ProjectStats {
                project_path: session.project_path.clone(),
                project_name: format_project_name(&session.project_path),
                session_count: 0,
                stats: Stats::default(),
                models: HashMap::new(),
            });

        project.session_count += 1;
        project.stats.add(&session.stats);

        for (model, model_stats) in &session.models {
            project.models.entry(model.clone()).or_default().add(model_stats);
        }
    }

    let mut projects: Vec<ProjectStats> = project_map.into_values().collect();
    projects.sort_by(|a, b| b.stats.total_tokens().cmp(&a.stats.total_tokens()));
    projects
}

/// Extract readable project name from encoded path
pub fn format_project_name(path: &str) -> String {
    if path.contains('/') || path.contains('\\') {
        return std::path::Path::new(path)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(path)
            .to_string();
    }

    path.trim_start_matches('-').to_string()
}

/// Aggregate entries by 5-hour billing blocks
pub fn aggregate_blocks(
    entries: &[RawEntry],
    local_times: &HashMap<String, DateTime<FixedOffset>>,
) -> Vec<BlockStats> {
    let mut block_map: HashMap<DateTime<FixedOffset>, BlockStats> = HashMap::new();

    for entry in entries {
        let local_dt = match local_times.get(&format!("{}:{}", entry.session_id, entry.timestamp)) {
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
        block.models.entry(entry.model.clone()).or_default().add(&stats);
    }

    let mut blocks: Vec<BlockStats> = block_map.into_values().collect();
    blocks.sort_by(|a, b| a.block_start.cmp(&b.block_start));
    blocks
}

/// Calculate the 5-hour block start time for a given timestamp
fn get_block_start(dt: DateTime<FixedOffset>) -> DateTime<FixedOffset> {
    let hour = dt.hour() as i64;
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

    #[test]
    fn test_format_project_name() {
        assert_eq!(
            format_project_name("-Users-john-projects-myapp"),
            "Users-john-projects-myapp"
        );
        assert_eq!(format_project_name("simple"), "simple");
        assert_eq!(
            format_project_name("/Users/john/projects/my-project"),
            "my-project"
        );
    }
}
