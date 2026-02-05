//! Unified data loader for all sources

use rayon::prelude::*;
use std::collections::HashMap;
use std::time::Instant;

use crate::core::{
    aggregate_blocks, aggregate_daily, aggregate_projects, aggregate_sessions, deduplicate,
    BlockStats, DateFilter, LoadResult, ProjectStats, RawEntry, SessionStats,
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
            let date = if let Ok(utc_dt) = entry.timestamp.parse::<DateTime<Utc>>() {
                let local_dt = timezone.to_fixed_offset(utc_dt);
                let date = local_dt.date_naive();
                entry.date_str = date.format("%Y-%m-%d").to_string();
                entry.timestamp_ms = utc_dt.timestamp_millis();
                Some(date)
            } else if let Ok(date) =
                chrono::NaiveDate::parse_from_str(&entry.date_str, "%Y-%m-%d")
            {
                Some(date)
            } else {
                None
            };

            if let Some(date) = date {
                if filter.contains(date) {
                    filtered.push(entry);
                }
            }
        }
        filtered
    }

    /// Load raw entries from files
    fn load_raw_entries(
        &self,
        filter: &DateFilter,
        timezone: &Timezone,
    ) -> Vec<RawEntry> {
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
            .flat_map(|path| self.source.parse_file(path, filter, timezone))
            .collect();
        let parse_ms = parse_start.elapsed().as_secs_f64() * 1000.0;

        if !self.quiet {
            eprintln!("Parsed {} files ({:.2}ms)", files.len(), parse_ms);
        }

        entries
    }

    /// Load and aggregate daily stats
    fn load_daily(&self, filter: &DateFilter, timezone: &Timezone) -> LoadResult {
        let load_start = Instant::now();
        let entries = self.load_raw_entries(filter, timezone);
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
        let entries = self.load_raw_entries(filter, timezone);
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

        let entries = self.load_raw_entries(filter, timezone);
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
        let mut local_times: HashMap<String, DateTime<FixedOffset>> = HashMap::new();
        for entry in &final_entries {
            if let Ok(utc_dt) = entry.timestamp.parse::<DateTime<Utc>>() {
                let local_dt = timezone.to_fixed_offset(utc_dt);
                let key = format!("{}:{}", entry.session_id, entry.timestamp);
                local_times.insert(key, local_dt);
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
