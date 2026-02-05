//! Core data types shared across all data sources
//!
//! These types represent the unified data model that all sources convert to.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Token usage statistics
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Stats {
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_creation: i64,
    pub cache_read: i64,
    /// Reasoning tokens (e.g., Codex o1 models)
    pub reasoning_tokens: i64,
    pub count: i64,
    pub skipped_chunks: i64,
}

impl Stats {
    pub fn add(&mut self, other: &Stats) {
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
        self.cache_creation += other.cache_creation;
        self.cache_read += other.cache_read;
        self.reasoning_tokens += other.reasoning_tokens;
        self.count += other.count;
        self.skipped_chunks += other.skipped_chunks;
    }

    /// Total tokens for display purposes
    pub fn total_tokens(&self) -> i64 {
        self.input_tokens
            + self.output_tokens
            + self.reasoning_tokens
            + self.cache_creation
            + self.cache_read
    }
}

/// Day-level aggregated statistics
#[derive(Debug, Default, Clone)]
pub struct DayStats {
    pub stats: Stats,
    pub models: HashMap<String, Stats>,
}

impl DayStats {
    pub fn add_stats(&mut self, model: &str, stats: &Stats) {
        self.stats.add(stats);
        self.models.entry(model.to_string()).or_default().add(stats);
    }
}

/// Session statistics
#[derive(Debug, Default, Clone)]
pub struct SessionStats {
    pub session_id: String,
    pub project_path: String,
    pub first_timestamp: String,
    pub last_timestamp: String,
    pub stats: Stats,
    pub models: HashMap<String, Stats>,
}

/// Project statistics
#[derive(Debug, Default, Clone)]
pub struct ProjectStats {
    pub project_path: String,
    pub project_name: String,
    pub session_count: usize,
    pub stats: Stats,
    pub models: HashMap<String, Stats>,
}

/// 5-hour billing block statistics
#[derive(Debug, Default, Clone)]
pub struct BlockStats {
    pub block_start: String,
    pub block_end: String,
    pub stats: Stats,
    pub models: HashMap<String, Stats>,
}

/// Raw entry parsed from source files
/// All sources convert their native format to this unified structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawEntry {
    /// UTC timestamp string
    pub timestamp: String,
    /// Timestamp in milliseconds for ordering
    pub timestamp_ms: i64,
    /// Local date string (YYYY-MM-DD)
    pub date_str: String,
    /// Message ID for deduplication (optional)
    pub message_id: Option<String>,
    /// Session ID
    pub session_id: String,
    /// Project path (may be empty for some sources)
    pub project_path: String,
    /// Model name
    pub model: String,
    /// Token counts
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_creation: i64,
    pub cache_read: i64,
    pub reasoning_tokens: i64,
    /// Stop reason for completion detection
    pub stop_reason: Option<String>,
}

impl RawEntry {
    pub fn to_stats(&self) -> Stats {
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
pub struct DateFilter {
    pub since: Option<chrono::NaiveDate>,
    pub until: Option<chrono::NaiveDate>,
}

impl DateFilter {
    pub fn new(since: Option<chrono::NaiveDate>, until: Option<chrono::NaiveDate>) -> Self {
        Self { since, until }
    }

    pub fn contains(&self, date: chrono::NaiveDate) -> bool {
        if let Some(s) = self.since {
            if date < s {
                return false;
            }
        }
        if let Some(u) = self.until {
            if date > u {
                return false;
            }
        }
        true
    }
}

/// Loading result with statistics
#[derive(Debug, Default)]
pub struct LoadResult {
    pub day_stats: HashMap<String, DayStats>,
    pub skipped: i64,
    pub valid: i64,
    /// Processing time in milliseconds (excluding cache save)
    pub elapsed_ms: f64,
}
