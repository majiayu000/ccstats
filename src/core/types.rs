//! Core data types shared across all data sources
//!
//! These types represent the unified data model that all sources convert to.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Token usage statistics
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub(crate) struct Stats {
    pub(crate) input_tokens: i64,
    pub(crate) output_tokens: i64,
    pub(crate) cache_creation: i64,
    pub(crate) cache_read: i64,
    /// Reasoning tokens (e.g., Codex o1 models)
    pub(crate) reasoning_tokens: i64,
    pub(crate) count: i64,
    pub(crate) skipped_chunks: i64,
}

impl Stats {
    pub(crate) fn add(&mut self, other: &Stats) {
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
        self.cache_creation += other.cache_creation;
        self.cache_read += other.cache_read;
        self.reasoning_tokens += other.reasoning_tokens;
        self.count += other.count;
        self.skipped_chunks += other.skipped_chunks;
    }

    /// Total tokens for display purposes
    pub(crate) fn total_tokens(&self) -> i64 {
        self.input_tokens
            + self.output_tokens
            + self.reasoning_tokens
            + self.cache_creation
            + self.cache_read
    }
}

/// Day-level aggregated statistics
#[derive(Debug, Default, Clone)]
pub(crate) struct DayStats {
    pub(crate) stats: Stats,
    pub(crate) models: HashMap<String, Stats>,
}

impl DayStats {
    pub(crate) fn add_stats(&mut self, model: String, stats: &Stats) {
        self.stats.add(stats);
        self.models.entry(model).or_default().add(stats);
    }
}

/// Session statistics
#[derive(Debug, Default, Clone)]
pub(crate) struct SessionStats {
    pub(crate) session_id: String,
    pub(crate) project_path: String,
    pub(crate) first_timestamp: String,
    pub(crate) last_timestamp: String,
    pub(crate) stats: Stats,
    pub(crate) models: HashMap<String, Stats>,
}

/// Project statistics
#[derive(Debug, Default, Clone)]
pub(crate) struct ProjectStats {
    pub(crate) project_path: String,
    pub(crate) project_name: String,
    pub(crate) session_count: usize,
    pub(crate) stats: Stats,
    pub(crate) models: HashMap<String, Stats>,
}

/// 5-hour billing block statistics
#[derive(Debug, Default, Clone)]
pub(crate) struct BlockStats {
    pub(crate) block_start: String,
    pub(crate) block_end: String,
    pub(crate) stats: Stats,
    pub(crate) models: HashMap<String, Stats>,
}

/// Raw entry parsed from source files
/// All sources convert their native format to this unified structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RawEntry {
    /// UTC timestamp string
    pub(crate) timestamp: String,
    /// Timestamp in milliseconds for ordering
    pub(crate) timestamp_ms: i64,
    /// Local date string (YYYY-MM-DD)
    pub(crate) date_str: String,
    /// Message ID for deduplication (optional)
    pub(crate) message_id: Option<String>,
    /// Session ID
    pub(crate) session_id: String,
    /// Project path (may be empty for some sources)
    pub(crate) project_path: String,
    /// Model name
    pub(crate) model: String,
    /// Token counts
    pub(crate) input_tokens: i64,
    pub(crate) output_tokens: i64,
    pub(crate) cache_creation: i64,
    pub(crate) cache_read: i64,
    pub(crate) reasoning_tokens: i64,
    /// Stop reason for completion detection
    pub(crate) stop_reason: Option<String>,
}

impl RawEntry {
    pub(crate) fn to_stats(&self) -> Stats {
        Stats {
            input_tokens: self.input_tokens,
            output_tokens: self.output_tokens,
            cache_creation: self.cache_creation,
            cache_read: self.cache_read,
            reasoning_tokens: self.reasoning_tokens,
            count: 1,
            skipped_chunks: 0,
        }
    }
}

/// Date filter for queries
#[derive(Debug, Clone, Default)]
pub(crate) struct DateFilter {
    pub(crate) since: Option<chrono::NaiveDate>,
    pub(crate) until: Option<chrono::NaiveDate>,
}

impl DateFilter {
    pub(crate) fn new(since: Option<chrono::NaiveDate>, until: Option<chrono::NaiveDate>) -> Self {
        Self { since, until }
    }

    pub(crate) fn contains(&self, date: chrono::NaiveDate) -> bool {
        if let Some(s) = self.since
            && date < s
        {
            return false;
        }
        if let Some(u) = self.until
            && date > u
        {
            return false;
        }
        true
    }
}

/// Loading result with statistics
#[derive(Debug, Default)]
pub(crate) struct LoadResult {
    pub(crate) day_stats: HashMap<String, DayStats>,
    pub(crate) skipped: i64,
    pub(crate) valid: i64,
    /// Processing time in milliseconds (excluding cache save)
    pub(crate) elapsed_ms: f64,
}
