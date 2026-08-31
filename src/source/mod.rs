//! Data source abstraction layer
//!
//! Each CLI tool (Claude, Codex, etc.) implements the Source trait
//! to provide a unified interface for loading and processing usage data.

mod amp;
mod claude;
mod cline;
mod cline_extension;
mod codex;
mod copilot;
mod cursor;
mod dsh;
mod dsh_format;
mod dsh_usage;
mod dsh_zstd;
mod gemini;
mod goose;
mod grok;
mod hermes;
mod kimi;
mod loader;
mod openclaw;
mod openclaw_store;
mod opencode;
mod opencode_fork;
mod pi;
mod pi_fork_paths;
mod pi_fork_schema;
mod pi_forks;
mod pi_paths;
mod qwen;
mod registry;
mod tool_loader;
mod unsloth;
mod xum;

mod reasonix {
    use std::env;
    use std::fs;
    use std::io::{BufRead, BufReader};
    use std::path::{Path, PathBuf};

    use chrono::{DateTime, NaiveDate, Utc};
    use serde::Deserialize;

    use crate::consts::{DATE_FORMAT, UNKNOWN};
    use crate::core::{CostKind, Endpoint, RawEntry, source_wide_message_id};
    use crate::utils::Timezone;

    use super::{Capabilities, ParseOutput, Source};

    pub(crate) struct ReasonixSource;

    impl ReasonixSource {
        pub(crate) const fn new() -> Self {
            Self
        }
    }

    impl Source for ReasonixSource {
        fn name(&self) -> &'static str {
            "reasonix"
        }

        fn display_name(&self) -> &'static str {
            "Reasonix"
        }

        fn capabilities(&self) -> Capabilities {
            Capabilities {
                has_projects: false,
                has_billing_blocks: false,
                has_reasoning_tokens: true,
                has_cache_creation: false,
                has_cache_read: true,
                needs_dedup: false,
                has_tool_calls: false,
                has_endpoints: false,
            }
        }

        fn find_files(&self) -> Vec<PathBuf> {
            state_root()
                .map(|root| find_files_in_root(&root))
                .unwrap_or_default()
        }

        fn parse_file(&self, path: &Path, timezone: Timezone, debug: bool) -> ParseOutput {
            parse_file(path, timezone, debug)
        }
    }

    fn expand_env(raw: &str) -> String {
        let mut result = String::with_capacity(raw.len());
        let mut rest = raw;
        while let Some(start) = rest.find("${") {
            result.push_str(&rest[..start]);
            let after = &rest[start + 2..];
            let Some(end) = after.find('}') else {
                result.push_str(&rest[start..]);
                return result;
            };
            let expression = &after[..end];
            let (name, fallback) = expression
                .split_once(":-")
                .map_or((expression, None), |(name, fallback)| {
                    (name, Some(fallback))
                });
            let value = env::var(name).ok().filter(|value| !value.is_empty());
            result.push_str(value.as_deref().or(fallback).unwrap_or_default());
            rest = &after[end + 1..];
        }
        result.push_str(rest);
        result
    }

    fn configured_dir(name: &str) -> Option<PathBuf> {
        let expanded = expand_env(env::var(name).ok()?.trim());
        if expanded.is_empty() {
            return None;
        }
        let path = if expanded == "~" {
            dirs::home_dir()?
        } else if let Some(relative) = expanded
            .strip_prefix("~/")
            .or_else(|| expanded.strip_prefix("~\\"))
        {
            dirs::home_dir()?.join(relative)
        } else {
            PathBuf::from(expanded)
        };
        if path.is_absolute() {
            Some(path)
        } else {
            env::current_dir().ok().map(|cwd| cwd.join(path))
        }
    }

    fn state_root() -> Option<PathBuf> {
        configured_dir("REASONIX_STATE_HOME")
            .or_else(|| configured_dir("REASONIX_HOME"))
            .or_else(|| {
                #[cfg(target_os = "windows")]
                {
                    dirs::config_dir()
                        .map(|root| root.join("reasonix"))
                        .or_else(|| {
                            dirs::home_dir().map(|home| home.join("AppData/Roaming/reasonix"))
                        })
                }
                #[cfg(not(target_os = "windows"))]
                {
                    dirs::home_dir().map(|home| home.join(".reasonix"))
                }
            })
    }

    fn find_files_in_root(root: &Path) -> Vec<PathBuf> {
        let Ok(entries) = fs::read_dir(root.join("stats")) else {
            return Vec::new();
        };
        let mut files = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.is_file())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .and_then(|name| name.strip_suffix(".jsonl"))
                    .is_some_and(|day| NaiveDate::parse_from_str(day, DATE_FORMAT).is_ok())
            })
            .collect::<Vec<_>>();
        files.sort();
        files
    }

    #[derive(Deserialize)]
    struct Record {
        ts: Option<String>,
        #[serde(default)]
        model: String,
        #[serde(default)]
        prompt: i64,
        #[serde(default)]
        completion: i64,
        #[serde(default)]
        reasoning: i64,
        #[serde(default)]
        cache_hit: i64,
        #[serde(default)]
        cache_miss: i64,
        #[serde(default)]
        total: i64,
        #[serde(default)]
        requests: i64,
        #[serde(default)]
        turn: bool,
        cost_amount: Option<String>,
        cost_currency: Option<String>,
        selected_amount: Option<String>,
        selected_currency: Option<String>,
        selected_cost: Option<f64>,
        cost_complete: Option<bool>,
        display_complete: Option<bool>,
        valuation_usd: Option<String>,
    }

    fn money(value: &str) -> Result<f64, &'static str> {
        let amount = value.parse::<f64>().map_err(|_| "invalid USD amount")?;
        if amount.is_finite() && amount >= 0.0 {
            Ok(amount)
        } else {
            Err("invalid USD amount")
        }
    }

    fn recorded_usd(record: &Record) -> Result<Option<f64>, &'static str> {
        if record.cost_complete != Some(true) {
            return Ok(None);
        }
        if let Some(value) = record.valuation_usd.as_deref() {
            return money(value).map(Some);
        }
        if record
            .cost_currency
            .as_deref()
            .is_some_and(|currency| currency.eq_ignore_ascii_case("USD"))
        {
            return record
                .cost_amount
                .as_deref()
                .ok_or("missing USD amount")
                .and_then(money)
                .map(Some);
        }
        if record.display_complete == Some(true)
            && record
                .selected_currency
                .as_deref()
                .is_some_and(|currency| currency.eq_ignore_ascii_case("USD"))
        {
            if let Some(value) = record.selected_amount.as_deref() {
                return money(value).map(Some);
            }
            let amount = record.selected_cost.ok_or("missing selected USD amount")?;
            return if amount.is_finite() && amount >= 0.0 {
                Ok(Some(amount))
            } else {
                Err("invalid selected USD amount")
            };
        }
        Ok(None)
    }

    fn entry(
        record: &Record,
        path: &Path,
        line_index: usize,
        timezone: Timezone,
    ) -> Result<Option<(RawEntry, bool)>, &'static str> {
        if record.turn {
            return Ok(None);
        }
        if [
            record.prompt,
            record.completion,
            record.reasoning,
            record.cache_hit,
            record.cache_miss,
            record.total,
            record.requests,
        ]
        .into_iter()
        .any(|value| value < 0)
        {
            return Err("negative usage value");
        }
        let model = record.model.trim();
        if model.is_empty() {
            return Err("empty model");
        }
        if record.reasoning > record.completion {
            return Err("reasoning tokens exceed completion tokens");
        }
        if record.cache_miss == 0 && record.cache_hit > record.prompt {
            return Err("cache hit tokens exceed prompt tokens");
        }
        if record.total == 0 && record.requests == 0 {
            return Ok(None);
        }
        if record.total > 0
            && record.prompt == 0
            && record.completion == 0
            && record.cache_hit == 0
            && record.cache_miss == 0
        {
            return Err("positive total has no token bucket detail");
        }
        let timestamp =
            DateTime::parse_from_rfc3339(record.ts.as_deref().ok_or("missing timestamp")?)
                .map_err(|_| "invalid timestamp")?
                .with_timezone(&Utc);
        let (recorded_cost_usd, cost_error) = match recorded_usd(record) {
            Ok(cost) => (cost, false),
            Err(_) => (None, true),
        };
        let identity = format!("{}:{line_index}", path.display());
        Ok(Some((
            RawEntry {
                timestamp: timestamp.to_rfc3339(),
                timestamp_ms: timestamp.timestamp_millis(),
                date_str: timezone
                    .to_fixed_offset(timestamp)
                    .date_naive()
                    .format(DATE_FORMAT)
                    .to_string(),
                message_id: Some(source_wide_message_id("reasonix", &identity)),
                session_key: "reasonix::ledger".to_string(),
                session_id: UNKNOWN.to_string(),
                project_path: UNKNOWN.to_string(),
                model: model.to_string(),
                input_tokens: if record.cache_miss > 0 {
                    record.cache_miss
                } else {
                    record.prompt - record.cache_hit
                },
                output_tokens: record.completion - record.reasoning,
                cache_creation: 0,
                cache_creation_1h: 0,
                cache_read: record.cache_hit,
                reasoning_tokens: record.reasoning,
                stop_reason: Some("completed".to_string()),
                cost_kind: CostKind::Real,
                endpoint: Endpoint::Unknown,
                call_count: if record.requests > 0 {
                    record.requests
                } else {
                    1
                },
                reported_total_tokens: None,
                recorded_cost_usd,
                api_equivalent_priced_tokens: 0,
                api_equivalent_coverage_tokens: 0,
            },
            cost_error,
        )))
    }

    fn parse_file(path: &Path, timezone: Timezone, debug: bool) -> ParseOutput {
        let file = match fs::File::open(path) {
            Ok(file) => file,
            Err(error) => {
                if debug {
                    eprintln!("Failed to open Reasonix ledger {}: {error}", path.display());
                }
                return ParseOutput {
                    entries: Vec::new(),
                    errors: 1,
                };
            }
        };
        let mut output = ParseOutput::default();
        for (line_index, line) in BufReader::new(file).lines().enumerate() {
            let result = line.map_err(|_| "failed to read line").and_then(|line| {
                if line.trim().is_empty() {
                    return Ok(None);
                }
                let record = serde_json::from_str::<Record>(&line).map_err(|_| "invalid JSON")?;
                entry(&record, path, line_index, timezone)
            });
            match result {
                Ok(Some((entry, cost_error))) => {
                    output.entries.push(entry);
                    output.errors += usize::from(cost_error);
                }
                Ok(None) => {}
                Err(error) => {
                    output.errors += 1;
                    if debug {
                        eprintln!(
                            "Invalid Reasonix usage in {} line {}: {error}",
                            path.display(),
                            line_index + 1
                        );
                    }
                }
            }
        }
        output
    }
}

mod fx;
mod fx_recovery;

use std::path::{Path, PathBuf};

use crate::core::{DateFilter, RawEntry, Stats, ToolCall};
use crate::utils::Timezone;

/// Coverage of a locally calculated API-equivalent cost.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct CostCoverage {
    pub(crate) total_tokens: i64,
    pub(crate) priced_tokens: i64,
    pub(crate) estimated_proxy: i64,
}

impl CostCoverage {
    pub(crate) fn from_stats<'a>(stats: impl IntoIterator<Item = &'a Stats>) -> Option<Self> {
        let mut coverage = Self::default();
        for stats in stats {
            coverage.total_tokens = coverage
                .total_tokens
                .saturating_add(stats.api_equivalent_coverage_tokens);
            coverage.priced_tokens = coverage
                .priced_tokens
                .saturating_add(stats.api_equivalent_priced_tokens);
            coverage.estimated_proxy = coverage
                .estimated_proxy
                .saturating_add(stats.estimated_proxy.total_tokens());
        }
        (coverage.total_tokens > 0 || coverage.priced_tokens > 0).then_some(coverage)
    }

    pub(crate) fn percent(self) -> f64 {
        if self.total_tokens <= 0 {
            0.0
        } else {
            self.priced_tokens.max(0).min(self.total_tokens) as f64 / self.total_tokens as f64
                * 100.0
        }
    }

    pub(crate) fn is_partial(self) -> bool {
        self.total_tokens <= 0 || self.priced_tokens != self.total_tokens
    }

    pub(crate) fn cost_is_lower_bound(self) -> bool {
        self.is_partial() && self.estimated_proxy == 0
    }
}

/// Parse result for a single source file.
#[derive(Clone, Debug, Default)]
pub(crate) struct ParseOutput {
    pub(crate) entries: Vec<RawEntry>,
    pub(crate) errors: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DiagnosticStatus {
    Detected,
    Configured,
    Missing,
}

impl DiagnosticStatus {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Detected => "detected",
            Self::Configured => "configured",
            Self::Missing => "missing",
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SourceDiagnostic {
    pub(crate) status: DiagnosticStatus,
    pub(crate) files: usize,
    pub(crate) detail: String,
}

impl SourceDiagnostic {
    pub(crate) fn detected(files: usize, detail: impl Into<String>) -> Self {
        Self {
            status: DiagnosticStatus::Detected,
            files,
            detail: detail.into(),
        }
    }

    pub(crate) fn configured(detail: impl Into<String>) -> Self {
        Self {
            status: DiagnosticStatus::Configured,
            files: 0,
            detail: detail.into(),
        }
    }

    pub(crate) fn missing(detail: impl Into<String>) -> Self {
        Self {
            status: DiagnosticStatus::Missing,
            files: 0,
            detail: detail.into(),
        }
    }
}

/// Capabilities that a data source may support
#[derive(Debug, Clone, Default)]
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct Capabilities {
    /// Supports project-level aggregation
    pub(crate) has_projects: bool,
    /// Supports 5-hour billing block aggregation
    pub(crate) has_billing_blocks: bool,
    /// Has reasoning tokens (e.g., o1 models)
    pub(crate) has_reasoning_tokens: bool,
    /// Has cache creation tokens
    pub(crate) has_cache_creation: bool,
    /// Has trustworthy prompt-cache read tokens
    pub(crate) has_cache_read: bool,
    /// Requires deduplication (streaming creates duplicate entries)
    pub(crate) needs_dedup: bool,
    /// Supports tool-call discovery and parsing
    pub(crate) has_tool_calls: bool,
    /// Populates the serving-endpoint field (native vs proxy classification)
    pub(crate) has_endpoints: bool,
}

impl Capabilities {
    pub(crate) fn combine<'a>(sources: impl IntoIterator<Item = &'a dyn Source>) -> Self {
        let mut combined = Self {
            has_cache_read: true,
            ..Self::default()
        };
        let mut has_sources = false;
        for source in sources {
            has_sources = true;
            let caps = source.capabilities();
            combined.has_projects |= caps.has_projects;
            combined.has_billing_blocks |= caps.has_billing_blocks;
            combined.has_reasoning_tokens |= caps.has_reasoning_tokens;
            combined.has_cache_creation |= caps.has_cache_creation;
            combined.has_cache_read &= caps.has_cache_read;
            combined.needs_dedup |= caps.needs_dedup;
            combined.has_tool_calls |= caps.has_tool_calls;
            combined.has_endpoints |= caps.has_endpoints;
        }
        combined.has_cache_read &= has_sources;
        combined
    }
}

/// Data source trait - implemented by each CLI tool
pub(crate) trait Source: Send + Sync {
    /// Unique name for this source (used in CLI subcommands)
    fn name(&self) -> &'static str;

    /// Display name for output
    fn display_name(&self) -> &'static str {
        self.name()
    }

    /// Short aliases for CLI.
    fn aliases(&self) -> &'static [&'static str] {
        &[]
    }

    /// Capabilities of this source
    fn capabilities(&self) -> Capabilities;

    /// Actionable setup guidance used by the read-only doctor command.
    fn setup_hint(&self) -> &'static str {
        "Run the source once so it creates local usage data"
    }

    /// Inspect whether this source can provide data without parsing logs or
    /// contacting remote services.
    fn diagnose(&self) -> SourceDiagnostic {
        let files = self.find_files().len();
        if files == 0 {
            SourceDiagnostic::missing("No local usage files found")
        } else {
            SourceDiagnostic::detected(files, format!("Found {files} local usage file(s)"))
        }
    }

    /// Find all data files for this source
    fn find_files(&self) -> Vec<PathBuf>;

    /// Find data for a requested date range. Remote sources may use the range
    /// to bound API requests; local sources keep the default file discovery.
    fn find_files_for_filter(&self, _filter: &DateFilter, _timezone: Timezone) -> Vec<PathBuf> {
        self.find_files()
    }

    /// Parse a single file into raw entries and diagnostics.
    fn parse_file(&self, path: &Path, timezone: Timezone, debug: bool) -> ParseOutput;

    fn finalize_entries(&self, entries: Vec<RawEntry>) -> Vec<RawEntry> {
        entries
    }

    /// Find files that may contain tool-call records for this source.
    fn find_tool_call_files(&self) -> Vec<PathBuf> {
        Vec::new()
    }

    /// Parse tool-call records from one source-owned file.
    fn parse_tool_call_file(&self, _path: &Path, _timezone: Timezone) -> Vec<ToolCall> {
        Vec::new()
    }
}

/// Box type for dynamic dispatch
pub(crate) type BoxedSource = Box<dyn Source>;

pub(crate) use codex::load_weekly_window_usage_from_home;
pub use codex::{CodexQuotaError, CodexQuotaStatus, CodexWeeklyQuota};
pub(crate) use codex::{CodexScope, CodexSource, load_weekly_quota, load_weekly_quota_from_home};

// Re-export registry functions
pub(crate) use registry::{ALL_SOURCES, all_sources, get_source, source_choices, suggest_source};

pub(crate) fn all_capabilities() -> Capabilities {
    Capabilities::combine(all_sources())
}

// Re-export loader functions
pub(crate) use loader::{load_blocks, load_daily, load_projects, load_sessions};
pub(crate) use tool_loader::load_tool_calls;

/// Load per-endpoint stats (native vs proxy) for a source. Claude-only; other
/// sources return empty. Lives here (not in `loader.rs`) to keep that file
/// under the module size limit.
pub(crate) fn load_endpoints(
    source: &dyn Source,
    filter: &crate::core::DateFilter,
    timezone: Timezone,
) -> Vec<crate::core::EndpointStats> {
    loader::DataLoader::new(source, false, false).load_endpoints(filter, timezone)
}

#[cfg(test)]
mod cost_coverage_tests {
    use super::CostCoverage;

    #[test]
    fn priced_tokens_without_a_total_are_incomplete() {
        let coverage = CostCoverage {
            total_tokens: 0,
            priced_tokens: 120,
            estimated_proxy: 0,
        };

        assert!(coverage.is_partial());
        assert!(coverage.percent().abs() < f64::EPSILON);
    }
}
