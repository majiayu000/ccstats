//! Core data types shared across all data sources
//!
//! These types represent the unified data model that all sources convert to.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Token subset from individual requests with more than 272K input tokens.
/// Kept separate from display totals so aggregation never changes the price tier.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct LongContextTokens {
    pub(crate) input_tokens: i64,
    pub(crate) output_tokens: i64,
    pub(crate) cache_creation: i64,
    pub(crate) cache_creation_1h: i64,
    pub(crate) cache_read: i64,
    pub(crate) reasoning_tokens: i64,
}

impl LongContextTokens {
    fn add(&mut self, other: &Self) {
        self.input_tokens = self.input_tokens.saturating_add(other.input_tokens);
        self.output_tokens = self.output_tokens.saturating_add(other.output_tokens);
        self.cache_creation = self.cache_creation.saturating_add(other.cache_creation);
        self.cache_creation_1h = self
            .cache_creation_1h
            .saturating_add(other.cache_creation_1h);
        self.cache_read = self.cache_read.saturating_add(other.cache_read);
        self.reasoning_tokens = self.reasoning_tokens.saturating_add(other.reasoning_tokens);
    }

    fn saturating_sub(self, other: &Self) -> Self {
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
        }
    }
}

/// Token usage statistics
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub(crate) struct Stats {
    #[serde(default)]
    pub(crate) above_272k: LongContextTokens,
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
        self.above_272k.add(&other.above_272k);
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
            above_272k: self.above_272k,
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

    /// Copy `CostKind::Real` buckets into the default display totals.
    ///
    /// Leaves `estimated_proxy` populated so estimated proxy cost can still be
    /// reported. After this transform, `CostDisplayMode::Total` prices the
    /// remaining buckets (already real); `RealOnly` would subtract
    /// `estimated_proxy` a second time.
    pub(crate) fn retain_real_token_totals(&mut self) {
        let real = self.real_cost_tokens();
        self.input_tokens = real.input_tokens;
        self.output_tokens = real.output_tokens;
        self.cache_creation = real.cache_creation;
        self.cache_creation_1h = real.cache_creation_1h;
        self.cache_read = real.cache_read;
        self.reasoning_tokens = real.reasoning_tokens;
        self.count = real.count;
        self.above_272k = real.above_272k;
        // Keep Total-mode recorded+priced cost real-only: priced_tokens still
        // includes estimated-proxy rows until they are dropped here.
        self.priced_tokens = self.priced_tokens.saturating_sub(&self.estimated_proxy);
    }

    /// True when default token buckets still contain estimated-proxy tokens.
    pub(crate) fn display_includes_estimated_proxy(&self) -> bool {
        let proxy = self.estimated_proxy;
        proxy.has_entries()
            && self.input_tokens >= proxy.input_tokens
            && self.output_tokens >= proxy.output_tokens
            && self.cache_creation >= proxy.cache_creation
            && self.cache_creation_1h >= proxy.cache_creation_1h
            && self.cache_read >= proxy.cache_read
            && self.reasoning_tokens >= proxy.reasoning_tokens
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
    #[serde(default)]
    pub(crate) above_272k: LongContextTokens,
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
        self.above_272k.add(&other.above_272k);
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
            above_272k: self.above_272k.saturating_sub(&other.above_272k),
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

/// Restrict `--source all` display totals to `CostKind::Real` tokens.
///
/// Does not clear `estimated_proxy`, so estimated proxy cost can still be
/// reported separately. Do not apply this to the Grok-only load path.
pub(crate) fn apply_real_token_totals_for_all_source(day_stats: &mut HashMap<String, DayStats>) {
    for day in day_stats.values_mut() {
        day.stats.retain_real_token_totals();
        for model_stats in day.models.values_mut() {
            model_stats.retain_real_token_totals();
        }
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

/// Estimated 5-hour session window statistics (inferred from local logs; not an official billing reset)
#[derive(Debug, Default, Clone)]
pub(crate) struct BlockStats {
    pub(crate) block_start: String,
    pub(crate) block_end: String,
    /// Floored UTC-hour start of this window, in Unix milliseconds.
    pub(crate) start_ms: i64,
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
            above_272k: LongContextTokens::default(),
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
        if self
            .input_tokens
            .saturating_add(self.cache_creation)
            .saturating_add(self.cache_read)
            > 272_000
        {
            stats.above_272k = LongContextTokens {
                input_tokens: self.input_tokens,
                output_tokens: self.output_tokens,
                cache_creation: self.cache_creation,
                cache_creation_1h: self.cache_creation_1h,
                cache_read: self.cache_read,
                reasoning_tokens: self.reasoning_tokens,
            };
        }
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
#[path = "types_tests.rs"]
mod tests;
