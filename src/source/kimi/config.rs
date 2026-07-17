//! Kimi Code data source configuration
//!
//! Defines the `KimiSource` implementation of the Source trait.

use std::path::{Path, PathBuf};

use crate::source::{Capabilities, ParseOutput, Source};
use crate::utils::Timezone;

use super::parser::{find_kimi_files, parse_kimi_wire_file_with_debug};

/// Kimi Code CLI data source.
pub(crate) struct KimiSource;

impl KimiSource {
    pub(crate) fn new() -> Self {
        Self
    }
}

impl Default for KimiSource {
    fn default() -> Self {
        Self::new()
    }
}

impl Source for KimiSource {
    fn name(&self) -> &'static str {
        "kimi"
    }

    fn display_name(&self) -> &'static str {
        "Kimi Code"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["km"]
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            has_projects: true,
            has_billing_blocks: false,
            has_reasoning_tokens: false,
            has_cache_creation: true,
            has_cache_read: true,
            needs_dedup: false,
            has_tool_calls: false,
            has_endpoints: false,
        }
    }

    fn find_files(&self) -> Vec<PathBuf> {
        find_kimi_files()
    }

    fn parse_file(&self, path: &Path, timezone: Timezone, debug: bool) -> ParseOutput {
        parse_kimi_wire_file_with_debug(path, timezone, debug)
    }
}
