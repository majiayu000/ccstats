//! Claude Code data source configuration
//!
//! Defines the ClaudeSource implementation of the Source trait.

use std::path::PathBuf;

use crate::core::RawEntry;
use crate::source::{Capabilities, Source};
use crate::utils::Timezone;

use super::parser::{find_claude_files, parse_claude_file};

/// Claude data source
pub(crate) struct ClaudeSource;

impl ClaudeSource {
    pub(crate) fn new() -> Self {
        Self
    }
}

impl Default for ClaudeSource {
    fn default() -> Self {
        Self::new()
    }
}

impl Source for ClaudeSource {
    fn name(&self) -> &'static str {
        "claude"
    }

    fn display_name(&self) -> &'static str {
        "Claude Code"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["cc"]
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            has_projects: true,
            has_billing_blocks: true,
            has_reasoning_tokens: false,
            needs_dedup: true,
        }
    }

    fn find_files(&self) -> Vec<PathBuf> {
        find_claude_files()
    }

    fn parse_file(&self, path: &PathBuf, timezone: &Timezone) -> Vec<RawEntry> {
        parse_claude_file(path, timezone)
    }
}
