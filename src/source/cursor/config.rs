//! Cursor data source configuration
//!
//! Defines the `CursorSource` implementation of the Source trait.

use std::env;
use std::path::{Path, PathBuf};

use crate::core::DateFilter;
use crate::source::{Capabilities, ParseOutput, Source, SourceDiagnostic};
use crate::utils::Timezone;

use super::client::has_api_credentials;
use super::parser::{USAGE_FILE_ENV, find_cursor_files, parse_cursor_with_debug};

/// Cursor usage API data source.
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
            has_cache_creation: true,
            has_cache_read: true,
            needs_dedup: false,
            has_tool_calls: false,
            has_endpoints: false,
        }
    }

    fn setup_hint(&self) -> &'static str {
        "Run `ccstats login cursor` or set CURSOR_API_KEY, CURSOR_SESSION_TOKEN, or CURSOR_USAGE_FILE"
    }

    fn diagnose(&self) -> SourceDiagnostic {
        if let Some(path) = env::var_os(USAGE_FILE_ENV).filter(|path| !path.is_empty()) {
            let path = PathBuf::from(path);
            return if path.is_file() {
                SourceDiagnostic::detected(1, "Cursor replay file is available")
            } else {
                SourceDiagnostic::missing("CURSOR_USAGE_FILE is set but does not point to a file")
            };
        }

        match has_api_credentials() {
            Ok(true) => SourceDiagnostic::configured(
                "Cursor API credentials are set; the provider was not contacted",
            ),
            Ok(false) => {
                SourceDiagnostic::missing("No Cursor API credentials or replay file found")
            }
            Err(error) => SourceDiagnostic::error(error.to_string()),
        }
    }

    fn find_files(&self) -> Vec<PathBuf> {
        find_cursor_files(&DateFilter::default(), Timezone::Local)
    }

    fn find_files_for_filter(&self, filter: &DateFilter, timezone: Timezone) -> Vec<PathBuf> {
        find_cursor_files(filter, timezone)
    }

    fn parse_file(&self, path: &Path, timezone: Timezone, debug: bool) -> ParseOutput {
        parse_cursor_with_debug(path, timezone, debug)
    }
}
