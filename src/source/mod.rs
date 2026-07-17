//! Data source abstraction layer
//!
//! Each CLI tool (Claude, Codex, etc.) implements the Source trait
//! to provide a unified interface for loading and processing usage data.

mod claude;
mod codex;
mod cursor;
mod grok;
mod kimi;
mod loader;
mod registry;

use std::path::{Path, PathBuf};

use crate::core::{RawEntry, ToolCall};
use crate::utils::Timezone;

/// Parse result for a single source file.
#[derive(Debug, Default)]
pub(crate) struct ParseOutput {
    pub(crate) entries: Vec<RawEntry>,
    pub(crate) errors: usize,
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

    /// Find all data files for this source
    fn find_files(&self) -> Vec<PathBuf>;

    /// Parse a single file into raw entries and diagnostics.
    fn parse_file(&self, path: &Path, timezone: Timezone, debug: bool) -> ParseOutput;

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

// Re-export registry functions
pub(crate) use registry::{ALL_SOURCES, all_sources, get_source, source_choices, suggest_source};

pub(crate) fn all_capabilities() -> Capabilities {
    Capabilities::combine(all_sources())
}

// Re-export loader functions
pub(crate) use loader::{load_blocks, load_daily, load_projects, load_sessions, load_tool_calls};

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
