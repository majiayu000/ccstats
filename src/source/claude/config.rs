//! Claude Code data source configuration
//!
//! Defines the `ClaudeSource` implementation of the Source trait.

use std::path::{Path, PathBuf};

use crate::source::{Capabilities, ParseOutput, Source};
use crate::utils::Timezone;

use super::parser::{find_claude_files, parse_claude_file_with_debug};
use super::tool_parser::parse_tool_calls;

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
            has_cache_creation: true,
            needs_dedup: true,
            has_tool_calls: true,
            has_endpoints: true,
        }
    }

    fn find_files(&self) -> Vec<PathBuf> {
        find_claude_files()
    }

    fn parse_file(&self, path: &Path, timezone: Timezone, debug: bool) -> ParseOutput {
        parse_claude_file_with_debug(path, timezone, debug)
    }

    fn find_tool_call_files(&self) -> Vec<PathBuf> {
        find_claude_files()
    }

    fn parse_tool_call_file(&self, path: &Path, timezone: Timezone) -> Vec<crate::core::ToolCall> {
        parse_tool_calls(path, timezone)
    }
}
