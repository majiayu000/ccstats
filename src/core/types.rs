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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn make_stats(input: i64, output: i64, cache_c: i64, cache_r: i64, reason: i64) -> Stats {
        Stats {
            input_tokens: input,
            output_tokens: output,
            cache_creation: cache_c,
            cache_read: cache_r,
            reasoning_tokens: reason,
            count: 1,
            skipped_chunks: 0,
        }
    }

    // --- Stats ---

    #[test]
    fn stats_default_all_zero() {
        let s = Stats::default();
        assert_eq!(s.input_tokens, 0);
        assert_eq!(s.output_tokens, 0);
        assert_eq!(s.cache_creation, 0);
        assert_eq!(s.cache_read, 0);
        assert_eq!(s.reasoning_tokens, 0);
        assert_eq!(s.count, 0);
        assert_eq!(s.skipped_chunks, 0);
    }

    #[test]
    fn stats_total_tokens_sums_five_fields() {
        let s = make_stats(100, 200, 50, 30, 20);
        assert_eq!(s.total_tokens(), 400); // 100+200+50+30+20
    }

    #[test]
    fn stats_total_tokens_excludes_count_and_skipped() {
        let s = Stats {
            input_tokens: 10,
            output_tokens: 5,
            cache_creation: 0,
            cache_read: 0,
            reasoning_tokens: 0,
            count: 999,
            skipped_chunks: 42,
        };
        assert_eq!(s.total_tokens(), 15);
    }

    #[test]
    fn stats_total_tokens_zero_when_default() {
        assert_eq!(Stats::default().total_tokens(), 0);
    }

    #[test]
    fn stats_add_accumulates_all_fields() {
        let mut a = make_stats(10, 20, 5, 3, 1);
        a.skipped_chunks = 2;
        let b = Stats {
            input_tokens: 100,
            output_tokens: 200,
            cache_creation: 50,
            cache_read: 30,
            reasoning_tokens: 10,
            count: 3,
            skipped_chunks: 5,
        };
        a.add(&b);
        assert_eq!(a.input_tokens, 110);
        assert_eq!(a.output_tokens, 220);
        assert_eq!(a.cache_creation, 55);
        assert_eq!(a.cache_read, 33);
        assert_eq!(a.reasoning_tokens, 11);
        assert_eq!(a.count, 4);
        assert_eq!(a.skipped_chunks, 7);
    }

    #[test]
    fn stats_add_to_default() {
        let mut a = Stats::default();
        let b = make_stats(5, 10, 15, 20, 25);
        a.add(&b);
        assert_eq!(a.input_tokens, 5);
        assert_eq!(a.output_tokens, 10);
        assert_eq!(a.total_tokens(), 75);
    }

    // --- DayStats ---

    #[test]
    fn day_stats_add_single_model() {
        let mut ds = DayStats::default();
        let s = make_stats(100, 200, 0, 0, 0);
        ds.add_stats("claude-3".into(), &s);
        assert_eq!(ds.stats.input_tokens, 100);
        assert_eq!(ds.stats.output_tokens, 200);
        assert_eq!(ds.stats.count, 1);
        assert_eq!(ds.models.len(), 1);
        assert_eq!(ds.models["claude-3"].input_tokens, 100);
    }

    #[test]
    fn day_stats_add_same_model_twice() {
        let mut ds = DayStats::default();
        ds.add_stats("gpt-4".into(), &make_stats(10, 20, 0, 0, 0));
        ds.add_stats("gpt-4".into(), &make_stats(30, 40, 0, 0, 0));
        assert_eq!(ds.stats.input_tokens, 40);
        assert_eq!(ds.stats.output_tokens, 60);
        assert_eq!(ds.stats.count, 2);
        assert_eq!(ds.models.len(), 1);
        assert_eq!(ds.models["gpt-4"].input_tokens, 40);
    }

    #[test]
    fn day_stats_add_multiple_models() {
        let mut ds = DayStats::default();
        ds.add_stats("a".into(), &make_stats(10, 0, 0, 0, 0));
        ds.add_stats("b".into(), &make_stats(20, 0, 0, 0, 0));
        ds.add_stats("c".into(), &make_stats(30, 0, 0, 0, 0));
        assert_eq!(ds.stats.input_tokens, 60);
        assert_eq!(ds.models.len(), 3);
    }

    // --- RawEntry ---

    #[test]
    fn raw_entry_to_stats() {
        let entry = RawEntry {
            timestamp: String::new(),
            timestamp_ms: 0,
            date_str: String::new(),
            message_id: None,
            session_id: String::new(),
            project_path: String::new(),
            model: String::new(),
            input_tokens: 100,
            output_tokens: 200,
            cache_creation: 50,
            cache_read: 30,
            reasoning_tokens: 10,
            stop_reason: None,
        };
        let s = entry.to_stats();
        assert_eq!(s.input_tokens, 100);
        assert_eq!(s.output_tokens, 200);
        assert_eq!(s.cache_creation, 50);
        assert_eq!(s.cache_read, 30);
        assert_eq!(s.reasoning_tokens, 10);
        assert_eq!(s.count, 1);
        assert_eq!(s.skipped_chunks, 0);
    }

    // --- DateFilter ---

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn date_filter_no_bounds() {
        let f = DateFilter::new(None, None);
        assert!(f.contains(d(2020, 1, 1)));
        assert!(f.contains(d(2099, 12, 31)));
    }

    #[test]
    fn date_filter_since_only() {
        let f = DateFilter::new(Some(d(2025, 6, 1)), None);
        assert!(!f.contains(d(2025, 5, 31)));
        assert!(f.contains(d(2025, 6, 1))); // inclusive
        assert!(f.contains(d(2025, 6, 2)));
    }

    #[test]
    fn date_filter_until_only() {
        let f = DateFilter::new(None, Some(d(2025, 6, 30)));
        assert!(f.contains(d(2025, 6, 29)));
        assert!(f.contains(d(2025, 6, 30))); // inclusive
        assert!(!f.contains(d(2025, 7, 1)));
    }

    #[test]
    fn date_filter_both_bounds() {
        let f = DateFilter::new(Some(d(2025, 3, 1)), Some(d(2025, 3, 31)));
        assert!(!f.contains(d(2025, 2, 28)));
        assert!(f.contains(d(2025, 3, 1)));
        assert!(f.contains(d(2025, 3, 15)));
        assert!(f.contains(d(2025, 3, 31)));
        assert!(!f.contains(d(2025, 4, 1)));
    }

    #[test]
    fn date_filter_single_day_range() {
        let f = DateFilter::new(Some(d(2025, 1, 15)), Some(d(2025, 1, 15)));
        assert!(!f.contains(d(2025, 1, 14)));
        assert!(f.contains(d(2025, 1, 15)));
        assert!(!f.contains(d(2025, 1, 16)));
    }
}
