//! Cursor data source configuration
//!
//! Defines the `CursorSource` implementation of the Source trait.

use std::path::{Path, PathBuf};

use crate::source::{Capabilities, ParseOutput, Source};
use crate::utils::Timezone;

use super::parser::{find_cursor_files, parse_cursor_db_with_debug};

/// Experimental Cursor data source.
pub(crate) struct CursorSource;

impl CursorSource {
    pub(crate) fn new() -> Self {
        Self
    }
}

impl Default for CursorSource {
    fn default() -> Self {
        Self::new()
    }
}

impl Source for CursorSource {
    fn name(&self) -> &'static str {
        "cursor"
    }

    fn display_name(&self) -> &'static str {
        "Cursor"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["cur"]
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            has_projects: false,
            has_billing_blocks: false,
            has_reasoning_tokens: false,
            has_cache_creation: false,
            needs_dedup: false,
        }
    }

    fn find_files(&self) -> Vec<PathBuf> {
        find_cursor_files()
    }

    fn parse_file(&self, path: &Path, timezone: Timezone, debug: bool) -> ParseOutput {
        parse_cursor_db_with_debug(path, timezone, debug)
    }
}
