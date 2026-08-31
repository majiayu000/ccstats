//! Grok data source configuration
//!
//! Defines the `GrokSource` implementation of the Source trait.

use std::path::{Path, PathBuf};

use crate::source::{Capabilities, ParseOutput, Source, SourceDiagnostic};
use crate::utils::Timezone;

use super::unified::{find_grok_files, grok_home, parse_grok_file_with_debug};

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
            has_reasoning_tokens: true,
            has_cache_creation: true,
            has_cache_read: true,
            needs_dedup: true,
            has_tool_calls: false,
            has_endpoints: false,
        }
    }

    fn setup_hint(&self) -> &'static str {
        "Run Grok once or set GROK_HOME to its data root"
    }

    fn diagnose(&self) -> SourceDiagnostic {
        let Some(home) = grok_home() else {
            return SourceDiagnostic::missing("Could not resolve the Grok data root");
        };
        let unified_log = home.join("logs/unified.jsonl");
        if unified_log.is_file() {
            return SourceDiagnostic::detected(1, "Found the Grok unified inference log");
        }

        let sessions_dir = home.join("sessions");
        if !sessions_dir.is_dir() {
            return SourceDiagnostic::missing("No Grok unified log or sessions directory found");
        }
        let files = super::parser::find_grok_files().len();
        if files == 0 {
            SourceDiagnostic::missing("The Grok sessions directory contains no usage records")
        } else {
            SourceDiagnostic::detected(files, format!("Found {files} Grok session file(s)"))
        }
    }

    fn find_files(&self) -> Vec<PathBuf> {
        find_grok_files()
    }

    fn parse_file(&self, path: &Path, timezone: Timezone, debug: bool) -> ParseOutput {
        parse_grok_file_with_debug(path, timezone, debug)
    }
}
