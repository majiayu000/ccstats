//! OpenAI Codex CLI data source configuration
//!
//! Defines the CodexSource implementation of the Source trait.

use std::path::PathBuf;

use crate::core::{DateFilter, RawEntry};
use crate::source::{Capabilities, Source};
use crate::utils::Timezone;

use super::parser::{find_codex_files, parse_codex_file};

/// Codex data source
pub struct CodexSource;

impl CodexSource {
    pub fn new() -> Self {
        Self
    }
}

impl Default for CodexSource {
    fn default() -> Self {
        Self::new()
    }
}

impl Source for CodexSource {
    fn name(&self) -> &'static str {
        "codex"
    }

    fn display_name(&self) -> &'static str {
        "OpenAI Codex"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["cx"]
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            has_projects: false, // Codex doesn't track projects
            has_billing_blocks: false, // Different billing model
            has_reasoning_tokens: true,
            needs_dedup: false, // Codex already handles dedup internally
        }
    }

    fn find_files(&self) -> Vec<PathBuf> {
        find_codex_files()
    }

    fn parse_file(
        &self,
        path: &PathBuf,
        filter: &DateFilter,
        timezone: &Timezone,
    ) -> Vec<RawEntry> {
        parse_codex_file(path, filter, timezone)
    }
}
