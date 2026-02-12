//! Unified data loader for all sources

use rayon::prelude::*;
use std::collections::HashMap;
use std::time::Instant;

use crate::core::{
    BlockStats, DateFilter, DayStats, DedupAccumulator, LoadResult, ProjectStats, RawEntry,
    SessionStats, aggregate_blocks, aggregate_daily, aggregate_projects, aggregate_sessions,
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

            if let Some(date) = date
                && filter.contains(date)
            {
                filtered.push(entry);
            }
        }
        filtered
    }

    /// Parallel file processing pipeline: discover → parse → filter → aggregate → reduce.
    /// Extracts the common pattern shared by all incremental loaders.
    fn par_process<T, F, I, R>(
        &self,
        filter: &DateFilter,
        timezone: &Timezone,
        per_file: F,
        init: I,
        reduce: R,
    ) -> Option<(T, usize)>
    where
        T: Send,
        F: Fn(Vec<RawEntry>) -> T + Send + Sync,
        I: Fn() -> T + Send + Sync,
        R: Fn(T, T) -> T + Send + Sync,
    {
        let discovery_start = Instant::now();
        let files = self.source.find_files();
        let discovery_ms = discovery_start.elapsed().as_secs_f64() * 1000.0;

        if files.is_empty() {
            return None;
        }

        if !self.quiet {
            eprintln!(
                "Scanning {} {} files... ({:.2}ms)",
                files.len(),
                self.source.display_name(),
                discovery_ms
            );
        }

        let file_count = files.len();
        let parse_start = Instant::now();
        let result = files
            .par_iter()
            .map(|path| {
                let entries = self.source.parse_file(path, timezone);
                let filtered = Self::filter_entries(entries, filter, timezone);
                per_file(filtered)
            })
            .reduce(&init, &reduce);
        let parse_ms = parse_start.elapsed().as_secs_f64() * 1000.0;

        if !self.quiet {
            eprintln!(
                "Parsed {} files incrementally ({:.2}ms)",
                file_count, parse_ms
            );
        }

        Some((result, file_count))
    }

    /// Load and deduplicate entries incrementally to avoid buffering all raw records in memory.
    fn load_deduped_entries_incremental(
        &self,
        filter: &DateFilter,
        timezone: &Timezone,
    ) -> (Vec<RawEntry>, i64) {
        let result = self.par_process(
            filter,
            timezone,
            |filtered| {
                let mut partial = DedupAccumulator::new();
                partial.extend(filtered);
                partial
            },
            DedupAccumulator::new,
            |mut acc, partial| {
                acc.merge(partial);
                acc
            },
        );

        match result {
            Some((accumulator, _)) => accumulator.finalize(),
            None => (Vec::new(), 0),
        }
    }

    fn merge_day_stats(target: &mut HashMap<String, DayStats>, source: HashMap<String, DayStats>) {
        for (date, stats) in source {
            let day = target.entry(date).or_default();
            day.stats.add(&stats.stats);
            for (model, model_stats) in stats.models {
                day.models.entry(model).or_default().add(&model_stats);
            }
        }
    }

    fn load_daily_incremental(&self, filter: &DateFilter, timezone: &Timezone) -> LoadResult {
        let load_start = Instant::now();

        let result = self.par_process(
            filter,
            timezone,
            aggregate_daily,
            HashMap::new,
            |mut acc, partial| {
                Self::merge_day_stats(&mut acc, partial);
                acc
            },
        );

        let day_stats = match result {
            Some((stats, _)) => stats,
            None => return LoadResult::default(),
        };

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
        let result = self.par_process(
            filter,
            timezone,
            |filtered| {
                let sessions = aggregate_sessions(filtered);
                let mut map = HashMap::<String, SessionStats>::new();
                for session in sessions {
                    let key = session.session_id.clone();
                    map.insert(key, session);
                }
                map
            },
            HashMap::<String, SessionStats>::new,
            |mut acc, partial| {
                for session in partial.into_values() {
                    match acc.get_mut(&session.session_id) {
                        Some(existing) => Self::merge_session_stats(existing, session),
                        None => {
                            let key = session.session_id.clone();
                            acc.insert(key, session);
                        }
                    }
                }
                acc
            },
        );

        match result {
            Some((merged, _)) => merged.into_values().collect(),
            None => Vec::new(),
        }
    }

    /// Load and aggregate daily stats
    fn load_daily(&self, filter: &DateFilter, timezone: &Timezone) -> LoadResult {
        if !self.source.capabilities().needs_dedup {
            return self.load_daily_incremental(filter, timezone);
        }

        let load_start = Instant::now();
        let (final_entries, skipped) = self.load_deduped_entries_incremental(filter, timezone);
        if final_entries.is_empty() {
            return LoadResult::default();
        }

        let agg_start = Instant::now();
        let valid = final_entries.len() as i64;
        let day_stats = aggregate_daily(final_entries);
        let agg_ms = agg_start.elapsed().as_secs_f64() * 1000.0;

        if !self.quiet {
            if skipped > 0 {
                eprintln!(
                    "Deduplicated {} entries, aggregated ({:.2}ms)",
                    skipped, agg_ms
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

        let (final_entries, skipped) = self.load_deduped_entries_incremental(filter, timezone);
        if final_entries.is_empty() {
            return Vec::new();
        }

        let sessions = aggregate_sessions(final_entries);

        if !self.quiet {
            if skipped > 0 {
                eprintln!(
                    "Found {} sessions after deduplicating {} entries",
                    sessions.len(),
                    skipped
                );
            } else {
                eprintln!("Found {} sessions", sessions.len());
            }
        }

        sessions
    }

    /// Load project stats (only for sources that support it)
    fn load_projects(&self, filter: &DateFilter, timezone: &Timezone) -> Vec<ProjectStats> {
        if !self.source.capabilities().has_projects {
            return Vec::new();
        }

        let sessions = self.load_sessions(filter, timezone);
        let projects = aggregate_projects(sessions);

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

        let (final_entries, skipped) = if self.source.capabilities().needs_dedup {
            self.load_deduped_entries_incremental(filter, timezone)
        } else {
            let entries: Vec<RawEntry> = self
                .source
                .find_files()
                .par_iter()
                .flat_map(|path| self.source.parse_file(path, timezone))
                .collect();
            let entries = Self::filter_entries(entries, filter, timezone);
            (entries, 0)
        };
        if final_entries.is_empty() {
            return Vec::new();
        }

        // Build local time map for block calculation
        let mut local_times: HashMap<i64, DateTime<FixedOffset>> = HashMap::new();
        for entry in &final_entries {
            if let Some(utc_dt) = DateTime::<Utc>::from_timestamp_millis(entry.timestamp_ms) {
                let local_dt = timezone.to_fixed_offset(utc_dt);
                local_times.insert(entry.timestamp_ms, local_dt);
            }
        }

        let blocks = aggregate_blocks(final_entries, &local_times);

        if !self.quiet {
            if skipped > 0 {
                eprintln!(
                    "Found {} billing blocks after deduplicating {} entries",
                    blocks.len(),
                    skipped
                );
            } else {
                eprintln!("Found {} billing blocks", blocks.len());
            }
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
