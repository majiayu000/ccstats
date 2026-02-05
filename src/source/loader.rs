//! Unified data loader for all sources
//!
//! Uses Source trait implementations with caching and deduplication.

use rayon::prelude::*;
use std::collections::HashMap;
use std::time::Instant;

use crate::core::{
    aggregate_blocks, aggregate_daily, aggregate_projects, aggregate_sessions,
    deduplicate, get_source_cache_path, CacheManager, DateFilter,
    FileLoadResult, LoadResult, ProjectStats, RawEntry, SessionStats, BlockStats,
};
use crate::source::Source;
use crate::utils::{Timezone, TimingStats};
use chrono::{DateTime, FixedOffset, Utc};

/// Load data from a source with caching
pub struct DataLoader<'a> {
    source: &'a dyn Source,
    cache: CacheManager,
    quiet: bool,
    debug: bool,
    timing: TimingStats,
}

impl<'a> DataLoader<'a> {
    pub fn new(source: &'a dyn Source, quiet: bool, debug: bool) -> Self {
        let cache = get_source_cache_path(source.cache_name())
            .map(CacheManager::new)
            .unwrap_or_else(CacheManager::ephemeral);
        Self {
            source,
            cache,
            quiet,
            debug,
            timing: TimingStats::new(),
        }
    }

    fn filter_entries(entries: Vec<RawEntry>, filter: &DateFilter) -> Vec<RawEntry> {
        entries
            .into_iter()
            .filter(|entry| {
                if let Ok(date) = chrono::NaiveDate::parse_from_str(&entry.date_str, "%Y-%m-%d")
                {
                    filter.contains(date)
                } else {
                    false
                }
            })
            .collect()
    }

    /// Load raw entries from files with caching
    fn load_raw_entries(
        &mut self,
        filter: &DateFilter,
        timezone: &Timezone,
    ) -> (Vec<RawEntry>, Vec<FileLoadResult>) {
        // File discovery timing
        let discovery_start = Instant::now();
        let files = self.source.find_files();
        self.timing.file_discovery = discovery_start.elapsed();

        if files.is_empty() {
            return (Vec::new(), Vec::new());
        }

        if !self.quiet {
            eprintln!(
                "Scanning {} {} files... ({:.2}ms)",
                files.len(),
                self.source.display_name(),
                self.timing.file_discovery.as_secs_f64() * 1000.0
            );
        }

        // Parsing timing
        let parse_start = Instant::now();

        // Load files in parallel
        let results: Vec<FileLoadResult> = files
            .par_iter()
            .map(|path| {
                let key = path.to_string_lossy().to_string();
                let meta = crate::core::file_meta(path);

                // Check cache first
                if let Some(cached) = self.cache.get_cached(path) {
                    return FileLoadResult {
                        key,
                        entries: cached.entries.clone(),
                        mtime: Some(cached.mtime),
                        size: Some(cached.size),
                        from_cache: true,
                    };
                }

                // Parse file
                let entries = self.source.parse_file(path, filter, timezone);
                FileLoadResult {
                    key,
                    entries,
                    mtime: meta.map(|m| m.0),
                    size: meta.map(|m| m.1),
                    from_cache: false,
                }
            })
            .collect();

        self.timing.parsing = parse_start.elapsed();

        let cache_hits = results.iter().filter(|r| r.from_cache).count();
        let cache_misses = results.len() - cache_hits;

        if !self.quiet {
            if cache_hits > 0 && cache_misses == 0 {
                // All from cache
                eprintln!(
                    "Loaded from cache ({} files, {:.2}ms)",
                    cache_hits,
                    self.timing.parsing.as_secs_f64() * 1000.0
                );
            } else if cache_hits > 0 {
                // Partial cache
                eprintln!(
                    "Loaded {} cached + {} parsed ({:.2}ms)",
                    cache_hits,
                    cache_misses,
                    self.timing.parsing.as_secs_f64() * 1000.0
                );
            } else {
                // No cache
                eprintln!(
                    "Parsed {} files ({:.2}ms)",
                    cache_misses,
                    self.timing.parsing.as_secs_f64() * 1000.0
                );
            }
        }

        let all_entries: Vec<RawEntry> = results.iter().flat_map(|r| r.entries.clone()).collect();

        (all_entries, results)
    }

    /// Load and aggregate daily stats
    pub fn load_daily(
        &mut self,
        filter: &DateFilter,
        timezone: &Timezone,
    ) -> LoadResult {
        let load_start = Instant::now();
        let (entries, file_results) = self.load_raw_entries(filter, timezone);
        let entries = Self::filter_entries(entries, filter);

        if entries.is_empty() {
            return LoadResult::default();
        }

        // Deduplicate if needed
        let dedup_start = Instant::now();
        let (final_entries, skipped) = if self.source.capabilities().needs_dedup {
            deduplicate(entries)
        } else {
            (entries, 0)
        };
        self.timing.dedup = dedup_start.elapsed();

        // Aggregation timing
        let agg_start = Instant::now();
        let valid = final_entries.len() as i64;
        let day_stats = aggregate_daily(&final_entries);
        self.timing.aggregation = agg_start.elapsed();

        if !self.quiet {
            let dedup_ms = self.timing.dedup.as_secs_f64() * 1000.0;
            let agg_ms = self.timing.aggregation.as_secs_f64() * 1000.0;
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

        // Calculate elapsed time before cache save (this is the "real" processing time)
        let elapsed_ms = load_start.elapsed().as_secs_f64() * 1000.0;

        // Save cache (in background conceptually - not counted in elapsed time)
        let cache_start = Instant::now();
        self.cache.save(file_results);
        self.timing.cache_save = cache_start.elapsed();

        if !self.quiet {
            eprintln!("Cache saved ({:.2}ms)", self.timing.cache_save.as_secs_f64() * 1000.0);
        }

        LoadResult {
            day_stats,
            skipped,
            valid,
            elapsed_ms,
        }
    }

    /// Load session stats
    pub fn load_sessions(
        &mut self,
        filter: &DateFilter,
        timezone: &Timezone,
    ) -> Vec<SessionStats> {
        let (entries, file_results) = self.load_raw_entries(filter, timezone);
        let entries = Self::filter_entries(entries, filter);

        if entries.is_empty() {
            return Vec::new();
        }

        let dedup_start = Instant::now();
        let (final_entries, _) = if self.source.capabilities().needs_dedup {
            deduplicate(entries)
        } else {
            (entries, 0)
        };
        self.timing.dedup = dedup_start.elapsed();

        let agg_start = Instant::now();
        let sessions = aggregate_sessions(&final_entries);
        self.timing.aggregation = agg_start.elapsed();

        if !self.quiet {
            eprintln!("Found {} sessions", sessions.len());
        }

        let cache_start = Instant::now();
        self.cache.save(file_results);
        self.timing.cache_save = cache_start.elapsed();

        sessions
    }

    /// Load project stats (only for sources that support it)
    pub fn load_projects(
        &mut self,
        filter: &DateFilter,
        timezone: &Timezone,
    ) -> Vec<ProjectStats> {
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
    pub fn load_blocks(
        &mut self,
        filter: &DateFilter,
        timezone: &Timezone,
    ) -> Vec<BlockStats> {
        if !self.source.capabilities().has_billing_blocks {
            return Vec::new();
        }

        let (entries, file_results) = self.load_raw_entries(filter, timezone);
        let entries = Self::filter_entries(entries, filter);

        if entries.is_empty() {
            return Vec::new();
        }

        let dedup_start = Instant::now();
        let (final_entries, _) = if self.source.capabilities().needs_dedup {
            deduplicate(entries)
        } else {
            (entries, 0)
        };
        self.timing.dedup = dedup_start.elapsed();

        let agg_start = Instant::now();
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
        self.timing.aggregation = agg_start.elapsed();

        if !self.quiet {
            eprintln!("Found {} billing blocks", blocks.len());
        }

        let cache_start = Instant::now();
        self.cache.save(file_results);
        self.timing.cache_save = cache_start.elapsed();

        blocks
    }

}

/// Convenience function to load daily stats for a source
pub fn load_daily(
    source: &dyn Source,
    filter: &DateFilter,
    timezone: &Timezone,
    quiet: bool,
    debug: bool,
) -> LoadResult {
    let mut loader = DataLoader::new(source, quiet, debug);
    loader.load_daily(filter, timezone)
}

/// Convenience function to load sessions for a source
pub fn load_sessions(
    source: &dyn Source,
    filter: &DateFilter,
    timezone: &Timezone,
    quiet: bool,
) -> Vec<SessionStats> {
    let mut loader = DataLoader::new(source, quiet, false);
    loader.load_sessions(filter, timezone)
}

/// Convenience function to load projects for a source
pub fn load_projects(
    source: &dyn Source,
    filter: &DateFilter,
    timezone: &Timezone,
    quiet: bool,
) -> Vec<ProjectStats> {
    let mut loader = DataLoader::new(source, quiet, false);
    loader.load_projects(filter, timezone)
}

/// Convenience function to load blocks for a source
pub fn load_blocks(
    source: &dyn Source,
    filter: &DateFilter,
    timezone: &Timezone,
    quiet: bool,
) -> Vec<BlockStats> {
    let mut loader = DataLoader::new(source, quiet, false);
    loader.load_blocks(filter, timezone)
}
