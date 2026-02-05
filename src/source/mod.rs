//! Data source abstraction layer
//!
//! Each CLI tool (Claude, Codex, etc.) implements the Source trait
//! to provide a unified interface for loading and processing usage data.

pub mod claude;
pub mod codex;
pub mod loader;
pub mod registry;

use std::path::PathBuf;

use crate::core::{DateFilter, RawEntry};
use crate::utils::Timezone;

/// Capabilities that a data source may support
#[derive(Debug, Clone, Default)]
pub struct Capabilities {
    /// Supports project-level aggregation
    pub has_projects: bool,
    /// Supports 5-hour billing block aggregation
    pub has_billing_blocks: bool,
    /// Has reasoning tokens (e.g., o1 models)
    pub has_reasoning_tokens: bool,
    /// Requires deduplication (streaming creates duplicate entries)
    pub needs_dedup: bool,
}

/// Data source trait - implemented by each CLI tool
pub trait Source: Send + Sync {
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
    fn parse_file(
        &self,
        path: &PathBuf,
        filter: &DateFilter,
        timezone: &Timezone,
    ) -> Vec<RawEntry>;
}

/// Box type for dynamic dispatch
pub type BoxedSource = Box<dyn Source>;

// Re-export registry functions
pub use registry::get_source;

// Re-export loader functions
pub use loader::{load_blocks, load_daily, load_projects, load_sessions};
