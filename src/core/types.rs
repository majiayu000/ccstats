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
    /// Portion of `cache_creation` written with a 1-hour TTL (billed at a higher rate).
    #[serde(default)]
    pub(crate) cache_creation_1h: i64,
    pub(crate) cache_read: i64,
    /// Reasoning tokens (e.g., Codex o1 models)
    pub(crate) reasoning_tokens: i64,
    /// Difference between source-authoritative total tokens and the component sum.
    /// This preserves an independent provider total without corrupting token buckets.
    #[serde(skip)]
    pub(crate) reported_total_adjustment: i64,
    pub(crate) count: i64,
    /// Number of parsed source records represented by this aggregation.
    #[serde(default)]
    pub(crate) records: i64,
    pub(crate) skipped_chunks: i64,
    pub(crate) estimated_proxy: CostTokens,
    /// Source-recorded USD cost that bypasses the current local price list.
    #[serde(default)]
    pub(crate) recorded_cost_usd: f64,
    /// Number of source records that contributed `recorded_cost_usd`.
    #[serde(default)]
    pub(crate) recorded_cost_entries: i64,
    /// Tokens covered by exact API-equivalent request telemetry.
    #[serde(default)]
    pub(crate) api_equivalent_priced_tokens: i64,
    /// Usage tokens eligible for API-equivalent coverage reporting.
    #[serde(default)]
    pub(crate) api_equivalent_coverage_tokens: i64,
    /// Tokens that still need local pricing (records without a provider cost).
    #[serde(default)]
    pub(crate) priced_tokens: CostTokens,
}

impl Stats {
    pub(crate) fn add(&mut self, other: &Stats) {
        self.input_tokens = self.input_tokens.saturating_add(other.input_tokens);
        self.output_tokens = self.output_tokens.saturating_add(other.output_tokens);
        self.cache_creation = self.cache_creation.saturating_add(other.cache_creation);
        self.cache_creation_1h = self
            .cache_creation_1h
            .saturating_add(other.cache_creation_1h);
        self.cache_read = self.cache_read.saturating_add(other.cache_read);
        self.reasoning_tokens = self.reasoning_tokens.saturating_add(other.reasoning_tokens);
        self.reported_total_adjustment = self
            .reported_total_adjustment
            .saturating_add(other.reported_total_adjustment);
        self.count = self.count.saturating_add(other.count);
        self.records = self.records.saturating_add(other.records);
        self.skipped_chunks = self.skipped_chunks.saturating_add(other.skipped_chunks);
        self.estimated_proxy.add(&other.estimated_proxy);
        self.recorded_cost_usd += other.recorded_cost_usd;
        self.recorded_cost_entries = self
            .recorded_cost_entries
            .saturating_add(other.recorded_cost_entries);
        self.api_equivalent_priced_tokens = self
            .api_equivalent_priced_tokens
            .saturating_add(other.api_equivalent_priced_tokens);
        self.api_equivalent_coverage_tokens = self
            .api_equivalent_coverage_tokens
            .saturating_add(other.api_equivalent_coverage_tokens);
        self.priced_tokens.add(&other.priced_tokens);
    }

    /// Total tokens for display purposes
    pub(crate) fn total_tokens(&self) -> i64 {
        self.input_tokens
            .saturating_add(self.output_tokens)
            .saturating_add(self.reasoning_tokens)
            .saturating_add(self.cache_creation)
            .saturating_add(self.cache_read)
            .saturating_add(self.reported_total_adjustment)
    }

    /// Percentage of reported input-side tokens served from the prompt cache.
    ///
    /// `supports_cache_read` comes from the selected source capability. Keeping
    /// it explicit prevents sources that do not report cache reads from being
    /// misrepresented as a real 0% hit rate.
    pub(crate) fn cache_hit_rate(&self, supports_cache_read: bool) -> Option<f64> {
        if !supports_cache_read {
            return None;
        }

        let input = i128::from(self.input_tokens.max(0));
        let cache_creation = i128::from(self.cache_creation.max(0));
        let cache_read = i128::from(self.cache_read.max(0));
        let total_input = input + cache_creation + cache_read;
        if total_input == 0 {
            None
        } else {
            Some(cache_read as f64 / total_input as f64 * 100.0)
        }
    }

    pub(crate) fn cost_tokens(&self) -> CostTokens {
        CostTokens {
            input_tokens: self.input_tokens,
            output_tokens: self.output_tokens,
            cache_creation: self.cache_creation,
            cache_creation_1h: self.cache_creation_1h,
            cache_read: self.cache_read,
            reasoning_tokens: self.reasoning_tokens,
            count: self.count,
        }
    }

    pub(crate) fn real_cost_tokens(&self) -> CostTokens {
        self.cost_tokens().saturating_sub(&self.estimated_proxy)
    }

    pub(crate) fn cost_kind(&self) -> CostKind {
        let estimated_count = self.estimated_proxy.count.max(0);
        if estimated_count == 0 {
            CostKind::Real
        } else if estimated_count >= self.count.max(0) {
            CostKind::EstimatedProxy
        } else {
            CostKind::Mixed
        }
    }
}

/// Cost provenance for token records.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CostKind {
    #[default]
    Real,
    EstimatedProxy,
    Mixed,
}

impl CostKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            CostKind::Real => "real",
            CostKind::EstimatedProxy => "estimated_proxy",
            CostKind::Mixed => "mixed",
        }
    }
}

/// Serving endpoint an entry was routed through.
///
/// Derived from the Claude Code `inference_geo` usage field (see
/// `source/claude/parser.rs`). This field is non-standard and undocumented;
/// the classification is empirically derived and may change across Claude Code
/// versions or proxies. Non-Claude sources are always `Unknown`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Endpoint {
    /// Native Anthropic endpoint (`inference_geo == "not_available"`).
    Native,
    /// Third-party proxy / gateway (`inference_geo == ""`): does not report
    /// cache creation and bills full context as raw input.
    Proxy,
    /// Field absent, other value, or non-Claude source.
    #[default]
    Unknown,
}

impl Endpoint {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Endpoint::Native => "native",
            Endpoint::Proxy => "proxy",
            Endpoint::Unknown => "unknown",
        }
    }

    /// Canonical display order for tables/JSON.
    pub(crate) const ORDER: [Endpoint; 3] = [Endpoint::Native, Endpoint::Proxy, Endpoint::Unknown];
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CostTokens {
    pub(crate) input_tokens: i64,
    pub(crate) output_tokens: i64,
    pub(crate) cache_creation: i64,
    /// Portion of `cache_creation` written with a 1-hour TTL (billed at a higher rate).
    #[serde(default)]
    pub(crate) cache_creation_1h: i64,
    pub(crate) cache_read: i64,
    pub(crate) reasoning_tokens: i64,
    pub(crate) count: i64,
}

impl CostTokens {
    pub(crate) fn total_tokens(self) -> i64 {
        self.input_tokens
            .saturating_add(self.output_tokens)
            .saturating_add(self.reasoning_tokens)
            .saturating_add(self.cache_creation)
            .saturating_add(self.cache_read)
    }

    pub(crate) fn add(&mut self, other: &Self) {
        self.input_tokens = self.input_tokens.saturating_add(other.input_tokens);
        self.output_tokens = self.output_tokens.saturating_add(other.output_tokens);
        self.cache_creation = self.cache_creation.saturating_add(other.cache_creation);
        self.cache_creation_1h = self
            .cache_creation_1h
            .saturating_add(other.cache_creation_1h);
        self.cache_read = self.cache_read.saturating_add(other.cache_read);
        self.reasoning_tokens = self.reasoning_tokens.saturating_add(other.reasoning_tokens);
        self.count = self.count.saturating_add(other.count);
    }

    pub(crate) fn saturating_sub(self, other: &Self) -> Self {
        Self {
            input_tokens: self.input_tokens.saturating_sub(other.input_tokens).max(0),
            output_tokens: self
                .output_tokens
                .saturating_sub(other.output_tokens)
                .max(0),
            cache_creation: self
                .cache_creation
                .saturating_sub(other.cache_creation)
                .max(0),
            cache_creation_1h: self
                .cache_creation_1h
                .saturating_sub(other.cache_creation_1h)
                .max(0),
            cache_read: self.cache_read.saturating_sub(other.cache_read).max(0),
            reasoning_tokens: self
                .reasoning_tokens
                .saturating_sub(other.reasoning_tokens)
                .max(0),
            count: self.count.saturating_sub(other.count).max(0),
        }
    }

    pub(crate) fn has_entries(self) -> bool {
        self.count > 0
            || self.input_tokens > 0
            || self.output_tokens > 0
            || self.cache_creation > 0
            || self.cache_read > 0
            || self.reasoning_tokens > 0
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
    pub(crate) session_key: String,
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

/// Per-endpoint (native vs proxy) statistics
#[derive(Debug, Default, Clone)]
pub(crate) struct EndpointStats {
    pub(crate) endpoint: Endpoint,
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
    /// Stable internal session identity used for aggregation/deduplication.
    #[serde(skip_serializing, skip_deserializing, default)]
    pub(crate) session_key: String,
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
    /// Portion of `cache_creation` written with a 1-hour TTL (billed at a higher rate).
    #[serde(default)]
    pub(crate) cache_creation_1h: i64,
    pub(crate) cache_read: i64,
    pub(crate) reasoning_tokens: i64,
    /// Source-authoritative total when it is independent from component counters.
    #[serde(default)]
    pub(crate) reported_total_tokens: Option<i64>,
    /// Stop reason for completion detection
    pub(crate) stop_reason: Option<String>,
    #[serde(default)]
    pub(crate) cost_kind: CostKind,
    /// Serving endpoint (native vs proxy); Claude-only, else `Unknown`.
    #[serde(default)]
    pub(crate) endpoint: Endpoint,
    /// Number of model calls represented by this record. Defaults to 1.
    #[serde(default = "default_call_count")]
    pub(crate) call_count: i64,
    /// Source-recorded USD cost for this record, when the source logs one.
    #[serde(default)]
    pub(crate) recorded_cost_usd: Option<f64>,
    /// Tokens represented by exact API-equivalent request telemetry.
    #[serde(default)]
    pub(crate) api_equivalent_priced_tokens: i64,
    /// Usage tokens eligible for API-equivalent coverage reporting.
    #[serde(default)]
    pub(crate) api_equivalent_coverage_tokens: i64,
}

fn default_call_count() -> i64 {
    1
}

impl RawEntry {
    pub(crate) fn to_stats(&self) -> Stats {
        // Parsers default real records to one call; a synthetic residual may
        // legitimately carry tokens or cost without representing another call.
        let call_count = self.call_count.max(0);
        let component_total = self
            .input_tokens
            .saturating_add(self.output_tokens)
            .saturating_add(self.cache_creation)
            .saturating_add(self.cache_read)
            .saturating_add(self.reasoning_tokens);
        let mut stats = Stats {
            input_tokens: self.input_tokens,
            output_tokens: self.output_tokens,
            cache_creation: self.cache_creation,
            cache_creation_1h: self.cache_creation_1h,
            cache_read: self.cache_read,
            reasoning_tokens: self.reasoning_tokens,
            reported_total_adjustment: self
                .reported_total_tokens
                .map_or(0, |total| total.saturating_sub(component_total)),
            count: call_count,
            records: 1,
            skipped_chunks: 0,
            estimated_proxy: CostTokens::default(),
            recorded_cost_usd: 0.0,
            recorded_cost_entries: 0,
            api_equivalent_priced_tokens: self.api_equivalent_priced_tokens.max(0),
            api_equivalent_coverage_tokens: self.api_equivalent_coverage_tokens.max(0),
            priced_tokens: CostTokens::default(),
        };
        if self.cost_kind == CostKind::EstimatedProxy {
            stats.estimated_proxy = stats.cost_tokens();
            stats.priced_tokens = stats.cost_tokens();
        } else if let Some(recorded) = self.recorded_cost_usd {
            stats.recorded_cost_usd = recorded.max(0.0);
            stats.recorded_cost_entries = 1;
        } else {
            stats.priced_tokens = stats.cost_tokens();
        }
        stats
    }
}

/// Date filter for queries
#[derive(Debug, Clone, Default)]
pub(crate) struct DateFilter {
    pub(crate) since: Option<chrono::NaiveDate>,
    pub(crate) until: Option<chrono::NaiveDate>,
    pub(crate) since_timestamp_ms: Option<i64>,
    pub(crate) until_timestamp_ms: Option<i64>,
    since_timestamp: Option<chrono::DateTime<chrono::Utc>>,
    until_timestamp: Option<chrono::DateTime<chrono::Utc>>,
}

impl DateFilter {
    pub(crate) fn new(since: Option<chrono::NaiveDate>, until: Option<chrono::NaiveDate>) -> Self {
        Self {
            since,
            until,
            since_timestamp_ms: None,
            until_timestamp_ms: None,
            since_timestamp: None,
            until_timestamp: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_timestamp_range(mut self, since: i64, until: i64) -> Self {
        self.since_timestamp_ms = Some(since);
        self.until_timestamp_ms = Some(until);
        self
    }

    pub(crate) fn with_exact_timestamp_range(
        mut self,
        since: chrono::DateTime<chrono::Utc>,
        until: chrono::DateTime<chrono::Utc>,
    ) -> Self {
        self.since_timestamp_ms = Some(since.timestamp_millis());
        self.until_timestamp_ms = Some(until.timestamp_millis());
        self.since_timestamp = Some(since);
        self.until_timestamp = Some(until);
        self
    }

    pub(crate) fn has_timestamp_range(&self) -> bool {
        self.since_timestamp_ms.is_some() || self.until_timestamp_ms.is_some()
    }

    pub(crate) fn contains_timestamp(&self, timestamp_ms: i64) -> bool {
        if let Some(since) = self.since_timestamp_ms
            && timestamp_ms < since
        {
            return false;
        }
        if let Some(until) = self.until_timestamp_ms
            && timestamp_ms > until
        {
            return false;
        }
        true
    }

    pub(crate) fn contains_datetime(&self, timestamp: chrono::DateTime<chrono::Utc>) -> bool {
        if let Some(since) = self.since_timestamp
            && timestamp < since
        {
            return false;
        }
        if let Some(until) = self.until_timestamp
            && timestamp > until
        {
            return false;
        }
        if self.since_timestamp.is_none() && self.until_timestamp.is_none() {
            return self.contains_timestamp(timestamp.timestamp_millis());
        }
        true
    }

    pub(crate) fn contains_entry_timestamp(&self, timestamp: &str, timestamp_ms: i64) -> bool {
        if self.since_timestamp.is_some() || self.until_timestamp.is_some() {
            return timestamp
                .parse::<chrono::DateTime<chrono::Utc>>()
                .is_ok_and(|timestamp| self.contains_datetime(timestamp));
        }
        self.contains_timestamp(timestamp_ms)
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
    pub(crate) parse_errors: usize,
    /// Processing time in milliseconds (excluding cache save)
    pub(crate) elapsed_ms: f64,
}

/// Machine-readable data quality metadata for structured consumers.
#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
pub(crate) struct DataQuality {
    pub(crate) valid_entries: i64,
    pub(crate) dedup_skipped_entries: i64,
    pub(crate) parse_errors: usize,
}

impl DataQuality {
    pub(crate) fn has_warnings(self) -> bool {
        self.dedup_skipped_entries > 0 || self.parse_errors > 0
    }
}

impl LoadResult {
    pub(crate) fn data_quality(&self) -> DataQuality {
        DataQuality {
            valid_entries: self.valid,
            dedup_skipped_entries: self.skipped,
            parse_errors: self.parse_errors,
        }
    }
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
            cache_creation_1h: 0,
            cache_read: cache_r,
            reasoning_tokens: reason,
            count: 1,
            skipped_chunks: 0,
            estimated_proxy: CostTokens::default(),
            ..Default::default()
        }
    }

    #[test]
    fn cache_hit_rate_uses_all_input_side_tokens() {
        let stats = make_stats(100, 50, 20, 80, 10);
        assert_eq!(stats.cache_hit_rate(true), Some(40.0));
    }

    #[test]
    fn cache_hit_rate_is_zero_when_supported_without_hits() {
        let stats = make_stats(100, 50, 0, 0, 0);
        assert_eq!(stats.cache_hit_rate(true), Some(0.0));
    }

    #[test]
    fn cache_hit_rate_is_unavailable_without_input_or_source_support() {
        assert_eq!(Stats::default().cache_hit_rate(true), None);
        assert_eq!(make_stats(100, 0, 0, 50, 0).cache_hit_rate(false), None);
    }

    #[test]
    fn cache_hit_rate_clamps_negative_components() {
        let stats = make_stats(-100, 0, -20, 80, 0);
        assert_eq!(stats.cache_hit_rate(true), Some(100.0));
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
            cache_creation_1h: 0,
            cache_read: 0,
            reasoning_tokens: 0,
            count: 999,
            skipped_chunks: 42,
            estimated_proxy: CostTokens::default(),
            ..Default::default()
        };
        assert_eq!(s.total_tokens(), 15);
    }

    #[test]
    fn stats_total_tokens_zero_when_default() {
        assert_eq!(Stats::default().total_tokens(), 0);
    }

    #[test]
    fn stats_aggregation_saturates_at_i64_max() {
        let mut total = Stats {
            input_tokens: i64::MAX,
            count: i64::MAX,
            records: i64::MAX,
            ..Default::default()
        };
        total.add(&Stats {
            input_tokens: 1,
            count: 1,
            records: 1,
            ..Default::default()
        });

        assert_eq!(total.input_tokens, i64::MAX);
        assert_eq!(total.count, i64::MAX);
        assert_eq!(total.records, i64::MAX);
        assert_eq!(total.total_tokens(), i64::MAX);
    }

    #[test]
    fn stats_add_accumulates_all_fields() {
        let mut a = make_stats(10, 20, 5, 3, 1);
        a.skipped_chunks = 2;
        let b = Stats {
            input_tokens: 100,
            output_tokens: 200,
            cache_creation: 50,
            cache_creation_1h: 0,
            cache_read: 30,
            reasoning_tokens: 10,
            count: 3,
            skipped_chunks: 5,
            estimated_proxy: CostTokens::default(),
            ..Default::default()
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
            session_key: String::new(),
            session_id: String::new(),
            project_path: String::new(),
            model: String::new(),
            input_tokens: 100,
            output_tokens: 200,
            cache_creation: 50,
            cache_creation_1h: 0,
            cache_read: 30,
            reasoning_tokens: 10,
            stop_reason: None,
            cost_kind: CostKind::Real,
            endpoint: Endpoint::Unknown,
            call_count: 1,
            reported_total_tokens: None,
            recorded_cost_usd: None,
            api_equivalent_priced_tokens: 0,
            api_equivalent_coverage_tokens: 0,
        };
        let s = entry.to_stats();
        assert_eq!(s.input_tokens, 100);
        assert_eq!(s.output_tokens, 200);
        assert_eq!(s.cache_creation, 50);
        assert_eq!(s.cache_read, 30);
        assert_eq!(s.reasoning_tokens, 10);
        assert_eq!(s.count, 1);
        assert_eq!(s.records, 1);
        assert_eq!(s.skipped_chunks, 0);
        assert_eq!(s.recorded_cost_entries, 0);
        assert_eq!(s.priced_tokens.input_tokens, 100);
    }

    #[test]
    fn raw_entry_to_stats_uses_call_count_and_recorded_cost() {
        let entry = RawEntry {
            timestamp: String::new(),
            timestamp_ms: 0,
            date_str: String::new(),
            message_id: None,
            session_key: String::new(),
            session_id: String::new(),
            project_path: String::new(),
            model: String::new(),
            input_tokens: 10,
            output_tokens: 2,
            cache_creation: 0,
            cache_creation_1h: 0,
            cache_read: 0,
            reasoning_tokens: 0,
            stop_reason: None,
            cost_kind: CostKind::Real,
            endpoint: Endpoint::Unknown,
            call_count: 5,
            reported_total_tokens: None,
            recorded_cost_usd: Some(1.25),
            api_equivalent_priced_tokens: 0,
            api_equivalent_coverage_tokens: 0,
        };
        let stats = entry.to_stats();
        assert_eq!(stats.count, 5);
        assert_eq!(stats.records, 1);
        assert!((stats.recorded_cost_usd - 1.25).abs() < 1e-12);
        assert_eq!(stats.recorded_cost_entries, 1);
        assert!(!stats.priced_tokens.has_entries());
    }

    #[test]
    fn raw_entry_preserves_independent_reported_total_without_repricing_the_delta() {
        let entry = RawEntry {
            timestamp: String::new(),
            timestamp_ms: 0,
            date_str: String::new(),
            message_id: None,
            session_key: String::new(),
            session_id: String::new(),
            project_path: String::new(),
            model: "provider/model".to_string(),
            input_tokens: 3,
            output_tokens: 4,
            cache_creation: 0,
            cache_creation_1h: 0,
            cache_read: 0,
            reasoning_tokens: 0,
            reported_total_tokens: Some(9),
            stop_reason: Some("complete".to_string()),
            cost_kind: CostKind::Real,
            endpoint: Endpoint::Unknown,
            call_count: 1,
            recorded_cost_usd: None,
            api_equivalent_priced_tokens: 0,
            api_equivalent_coverage_tokens: 0,
        };

        let stats = entry.to_stats();
        assert_eq!(stats.total_tokens(), 9);
        assert_eq!(stats.input_tokens, 3);
        assert_eq!(stats.output_tokens, 4);
        assert_eq!(stats.priced_tokens.input_tokens, 3);
        assert_eq!(stats.priced_tokens.output_tokens, 4);
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
