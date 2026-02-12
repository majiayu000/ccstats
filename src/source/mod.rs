//! Data source abstraction layer
//!
//! Each CLI tool (Claude, Codex, etc.) implements the Source trait
//! to provide a unified interface for loading and processing usage data.

mod claude;
mod codex;
mod loader;
mod registry;

use std::path::{Path, PathBuf};

use crate::core::RawEntry;
use crate::utils::Timezone;

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
    /// Requires deduplication (streaming creates duplicate entries)
    pub(crate) needs_dedup: bool,
}

/// Data source trait - implemented by each CLI tool
pub(crate) trait Source: Send + Sync {
    /// Unique name for this source (used in CLI subcommands)
    fn name(&self) -> &'static str;

    /// Display name for output
    fn display_name(&self) -> &'static str {
        self.name()
    }

    /// Short aliases for CLI (e.g., "cc" for "claude")
    fn aliases(&self) -> &'static [&'static str] {
        &[]
    }

    /// Capabilities of this source
    fn capabilities(&self) -> Capabilities;

    /// Find all data files for this source
    fn find_files(&self) -> Vec<PathBuf>;

    /// Parse a single file into raw entries
    fn parse_file(&self, path: &Path, timezone: Timezone) -> Vec<RawEntry>;
}

/// Box type for dynamic dispatch
pub(crate) type BoxedSource = Box<dyn Source>;

// Re-export registry functions
pub(crate) use registry::get_source;

// Re-export loader functions
pub(crate) use loader::{load_blocks, load_daily, load_projects, load_sessions};
