//! Grok data source configuration
//!
//! Defines the `GrokSource` implementation of the Source trait.

use std::path::{Path, PathBuf};

use crate::source::{Capabilities, ParseOutput, Source};
use crate::utils::Timezone;

use super::parser::{find_grok_files, parse_grok_signal_file_with_debug};

/// Grok data source.
pub(crate) struct GrokSource;

impl GrokSource {
    pub(crate) fn new() -> Self {
        Self
    }
}

impl Default for GrokSource {
    fn default() -> Self {
        Self::new()
    }
}

impl Source for GrokSource {
    fn name(&self) -> &'static str {
        "grok"
    }

    fn display_name(&self) -> &'static str {
        "Grok"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["gx"]
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            has_projects: true,
            has_billing_blocks: false,
            has_reasoning_tokens: false,
            has_cache_creation: false,
            needs_dedup: false,
        }
    }

    fn find_files(&self) -> Vec<PathBuf> {
        find_grok_files()
    }

    fn parse_file(&self, path: &Path, timezone: Timezone, debug: bool) -> ParseOutput {
        parse_grok_signal_file_with_debug(path, timezone, debug)
    }
}
