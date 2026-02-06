//! Unified data loader for all sources

use rayon::prelude::*;
use std::collections::HashMap;
use std::time::Instant;

use crate::core::{
    aggregate_blocks, aggregate_daily, aggregate_projects, aggregate_sessions, deduplicate,
    BlockStats, DateFilter, DayStats, LoadResult, ProjectStats, RawEntry, SessionStats,
};
use crate::source::Source;
use crate::utils::Timezone;
use chrono::{DateTime, FixedOffset, Utc};

/// Load data from a source
struct DataLoader<'a> {
    source: &'a dyn Source,
    quiet: bool,
    debug: bool,
}

impl<'a> DataLoader<'a> {
    fn new(source: &'a dyn Source, quiet: bool, debug: bool) -> Self {
        Self {
            source,
            quiet,
            debug,
        }
    }

    fn filter_entries(
        entries: Vec<RawEntry>,
        filter: &DateFilter,
        timezone: &Timezone,
    ) -> Vec<RawEntry> {
        let mut filtered = Vec::new();
        for mut entry in entries {
            let date = chrono::NaiveDate::parse_from_str(&entry.date_str, "%Y-%m-%d")
                .ok()
                .or_else(|| {
                    let utc_dt = entry.timestamp.parse::<DateTime<Utc>>().ok()?;
                    let local_dt = timezone.to_fixed_offset(utc_dt);
                    let date = local_dt.date_naive();
                    entry.date_str = date.format("%Y-%m-%d").to_string();
                    entry.timestamp_ms = utc_dt.timestamp_millis();
                    Some(date)
                });

            if let Some(date) = date {
                if filter.contains(date) {
                    filtered.push(entry);
                }
            }
        }
        filtered
    }

    /// Load raw entries from files
    fn load_raw_entries(&self, timezone: &Timezone) -> Vec<RawEntry> {
        let discovery_start = Instant::now();
        let files = self.source.find_files();
        let discovery_ms = discovery_start.elapsed().as_secs_f64() * 1000.0;

        if files.is_empty() {
            return Vec::new();
        }

        if !self.quiet {
            eprintln!(
                "Scanning {} {} files... ({:.2}ms)",
                files.len(),
                self.source.display_name(),
                discovery_ms
            );
        }

        let parse_start = Instant::now();
        let entries: Vec<RawEntry> = files
            .par_iter()
            .flat_map(|path| self.source.parse_file(path, timezone))
            .collect();
        let parse_ms = parse_start.elapsed().as_secs_f64() * 1000.0;

        if !self.quiet {
            eprintln!("Parsed {} files ({:.2}ms)", files.len(), parse_ms);
        }

        entries
    }

    fn merge_day_stats(
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

    fn load_daily_incremental(
        &self,
        filter: &DateFilter,
        timezone: &Timezone,
    ) -> LoadResult {
        let load_start = Instant::now();
        let discovery_start = Instant::now();
        let files = self.source.find_files();
        let discovery_ms = discovery_start.elapsed().as_secs_f64() * 1000.0;

        if files.is_empty() {
            return LoadResult::default();
        }

        if !self.quiet {
            eprintln!(
                "Scanning {} {} files... ({:.2}ms)",
                files.len(),
                self.source.display_name(),
                discovery_ms
            );
        }

        let parse_start = Instant::now();
        let day_stats = files
            .par_iter()
            .map(|path| {
                let entries = self.source.parse_file(path, timezone);
                let filtered = Self::filter_entries(entries, filter, timezone);
                aggregate_daily(&filtered)
            })
            .reduce(HashMap::new, |mut acc, partial| {
                Self::merge_day_stats(&mut acc, partial);
                acc
            });
        let parse_ms = parse_start.elapsed().as_secs_f64() * 1000.0;

        if !self.quiet {
            eprintln!(
                "Parsed {} files and aggregated incrementally ({:.2}ms)",
                files.len(),
                parse_ms
            );
        }

        let valid: i64 = day_stats.values().map(|day| day.stats.count).sum();
        if self.debug && !self.quiet {
            eprintln!("[DEBUG] Processed {} entries, {} skipped", valid, 0);
            eprintln!("[DEBUG] Days with data: {}", day_stats.len());
        }

        let elapsed_ms = load_start.elapsed().as_secs_f64() * 1000.0;
        LoadResult {
            day_stats,
            skipped: 0,
            valid,
            elapsed_ms,
        }
    }

    fn merge_session_stats(into: &mut SessionStats, incoming: SessionStats) {
        let SessionStats {
            project_path,
            first_timestamp,
            last_timestamp,
            stats,
            models,
            ..
        } = incoming;

        if first_timestamp < into.first_timestamp {
            into.first_timestamp = first_timestamp;
        }
        if last_timestamp > into.last_timestamp {
            into.last_timestamp = last_timestamp;
        }
        into.stats.add(&stats);
        for (model, stats) in models {
            into.models.entry(model).or_default().add(&stats);
        }
        if into.project_path.is_empty() && !project_path.is_empty() {
            into.project_path = project_path;
        }
    }

    fn load_sessions_incremental(
        &self,
        filter: &DateFilter,
        timezone: &Timezone,
    ) -> Vec<SessionStats> {
        let discovery_start = Instant::now();
        let files = self.source.find_files();
        let discovery_ms = discovery_start.elapsed().as_secs_f64() * 1000.0;

        if files.is_empty() {
            return Vec::new();
        }

        if !self.quiet {
            eprintln!(
                "Scanning {} {} files... ({:.2}ms)",
                files.len(),
                self.source.display_name(),
                discovery_ms
            );
        }

        let parse_start = Instant::now();
        let merged = files
            .par_iter()
            .map(|path| {
                let entries = self.source.parse_file(path, timezone);
                let filtered = Self::filter_entries(entries, filter, timezone);
                aggregate_sessions(&filtered)
            })
            .map(|sessions| {
                let mut map = HashMap::<String, SessionStats>::new();
                for session in sessions {
                    map.insert(session.session_id.clone(), session);
                }
                map
            })
            .reduce(HashMap::<String, SessionStats>::new, |mut acc, partial| {
                for session in partial.into_values() {
                    match acc.get_mut(&session.session_id) {
                        Some(existing) => Self::merge_session_stats(existing, session),
                        None => {
                            acc.insert(session.session_id.clone(), session);
                        }
                    }
                }
                acc
            });
        let parse_ms = parse_start.elapsed().as_secs_f64() * 1000.0;

        let sessions: Vec<SessionStats> = merged.into_values().collect();
        if !self.quiet {
            eprintln!(
                "Parsed {} files and aggregated {} sessions incrementally ({:.2}ms)",
                files.len(),
                sessions.len(),
                parse_ms
            );
        }
        sessions
    }

    /// Load and aggregate daily stats
    fn load_daily(&self, filter: &DateFilter, timezone: &Timezone) -> LoadResult {
        if !self.source.capabilities().needs_dedup {
            return self.load_daily_incremental(filter, timezone);
        }

        let load_start = Instant::now();
        let entries = self.load_raw_entries(timezone);
        let entries = Self::filter_entries(entries, filter, timezone);

        if entries.is_empty() {
            return LoadResult::default();
        }

        let dedup_start = Instant::now();
        let (final_entries, skipped) = if self.source.capabilities().needs_dedup {
            deduplicate(entries)
        } else {
            (entries, 0)
        };
        let dedup_ms = dedup_start.elapsed().as_secs_f64() * 1000.0;

        let agg_start = Instant::now();
        let valid = final_entries.len() as i64;
        let day_stats = aggregate_daily(&final_entries);
        let agg_ms = agg_start.elapsed().as_secs_f64() * 1000.0;

        if !self.quiet {
            if skipped > 0 {
                eprintln!(
                    "Deduplicated {} entries ({:.2}ms), aggregated ({:.2}ms)",
                    skipped, dedup_ms, agg_ms
                );
            } else {
                eprintln!("Aggregated ({:.2}ms)", agg_ms);
            }
        }

        if self.debug && !self.quiet {
            eprintln!("[DEBUG] Processed {} entries, {} skipped", valid, skipped);
            eprintln!("[DEBUG] Days with data: {}", day_stats.len());
        }

        let elapsed_ms = load_start.elapsed().as_secs_f64() * 1000.0;

        LoadResult {
            day_stats,
            skipped,
            valid,
            elapsed_ms,
        }
    }

    /// Load session stats
    fn load_sessions(&self, filter: &DateFilter, timezone: &Timezone) -> Vec<SessionStats> {
        if !self.source.capabilities().needs_dedup {
            return self.load_sessions_incremental(filter, timezone);
        }

        let entries = self.load_raw_entries(timezone);
        let entries = Self::filter_entries(entries, filter, timezone);

        if entries.is_empty() {
            return Vec::new();
        }

        let (final_entries, _) = if self.source.capabilities().needs_dedup {
            deduplicate(entries)
        } else {
            (entries, 0)
        };

        let sessions = aggregate_sessions(&final_entries);

        if !self.quiet {
            eprintln!("Found {} sessions", sessions.len());
        }

        sessions
    }

    /// Load project stats (only for sources that support it)
    fn load_projects(&self, filter: &DateFilter, timezone: &Timezone) -> Vec<ProjectStats> {
        if !self.source.capabilities().has_projects {
            return Vec::new();
        }

        let sessions = self.load_sessions(filter, timezone);
        let projects = aggregate_projects(&sessions);

        if !self.quiet {
            eprintln!("Aggregated into {} projects", projects.len());
        }

        projects
    }

    /// Load block stats (only for sources that support it)
    fn load_blocks(&self, filter: &DateFilter, timezone: &Timezone) -> Vec<BlockStats> {
        if !self.source.capabilities().has_billing_blocks {
            return Vec::new();
        }

        let entries = self.load_raw_entries(timezone);
        let entries = Self::filter_entries(entries, filter, timezone);

        if entries.is_empty() {
            return Vec::new();
        }

        let (final_entries, _) = if self.source.capabilities().needs_dedup {
            deduplicate(entries)
        } else {
            (entries, 0)
        };

        // Build local time map for block calculation
        let mut local_times: HashMap<i64, DateTime<FixedOffset>> = HashMap::new();
        for entry in &final_entries {
            if let Some(utc_dt) = DateTime::<Utc>::from_timestamp_millis(entry.timestamp_ms) {
                let local_dt = timezone.to_fixed_offset(utc_dt);
                local_times.insert(entry.timestamp_ms, local_dt);
            }
        }

        let blocks = aggregate_blocks(&final_entries, &local_times);

        if !self.quiet {
            eprintln!("Found {} billing blocks", blocks.len());
        }

        blocks
    }
}

/// Convenience function to load daily stats for a source
pub(crate) fn load_daily(
    source: &dyn Source,
    filter: &DateFilter,
    timezone: &Timezone,
    quiet: bool,
    debug: bool,
) -> LoadResult {
    let loader = DataLoader::new(source, quiet, debug);
    loader.load_daily(filter, timezone)
}

/// Convenience function to load sessions for a source
pub(crate) fn load_sessions(
    source: &dyn Source,
    filter: &DateFilter,
    timezone: &Timezone,
    quiet: bool,
) -> Vec<SessionStats> {
    let loader = DataLoader::new(source, quiet, false);
    loader.load_sessions(filter, timezone)
}

/// Convenience function to load projects for a source
pub(crate) fn load_projects(
    source: &dyn Source,
    filter: &DateFilter,
    timezone: &Timezone,
    quiet: bool,
) -> Vec<ProjectStats> {
    let loader = DataLoader::new(source, quiet, false);
    loader.load_projects(filter, timezone)
}

/// Convenience function to load blocks for a source
pub(crate) fn load_blocks(
    source: &dyn Source,
    filter: &DateFilter,
    timezone: &Timezone,
    quiet: bool,
) -> Vec<BlockStats> {
    let loader = DataLoader::new(source, quiet, false);
    loader.load_blocks(filter, timezone)
}
