//! `OpenAI` Codex CLI data source configuration
//!
//! Defines the `CodexSource` implementation of the Source trait.

use std::path::{Path, PathBuf};

use clap::ValueEnum;

use crate::source::{Capabilities, ParseOutput, Source};
use crate::utils::Timezone;

use super::parser::{
    find_codex_files, parse_codex_file_with_diagnostics, parse_codex_file_with_scope,
};

#[derive(Debug, Clone, Copy, Default, ValueEnum, PartialEq, Eq)]
pub(crate) enum CodexScope {
    /// Include every Codex session origin.
    #[default]
    All,
    /// Include interactive CLI and IDE sessions.
    Interactive,
    /// Include `codex exec` sessions only.
    Exec,
    /// Include spawned subagent sessions only.
    Subagent,
}

impl CodexScope {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            CodexScope::All => "all",
            CodexScope::Interactive => "interactive",
            CodexScope::Exec => "exec",
            CodexScope::Subagent => "subagent",
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            CodexScope::All => "all sessions",
            CodexScope::Interactive => "interactive CLI and IDE sessions",
            CodexScope::Exec => "exec sessions",
            CodexScope::Subagent => "subagent sessions",
        }
    }
}

/// Codex data source
pub(crate) struct CodexSource {
    scope: CodexScope,
    accounting_diagnostics: bool,
}

impl CodexSource {
    pub(crate) fn new() -> Self {
        Self::with_scope(CodexScope::All)
    }

    pub(crate) fn with_scope(scope: CodexScope) -> Self {
        Self {
            scope,
            accounting_diagnostics: false,
        }
    }
}

impl CodexSource {
    pub(crate) fn with_accounting_diagnostics(scope: CodexScope) -> Self {
        Self {
            scope,
            accounting_diagnostics: true,
        }
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
            has_projects: true,        // Recorded session_meta.cwd when available
            has_billing_blocks: false, // Different billing model
            has_reasoning_tokens: true,
            has_cache_creation: true,
            has_cache_read: true,
            needs_dedup: true,
            has_tool_calls: false,
            has_endpoints: false,
        }
    }

    fn setup_hint(&self) -> &'static str {
        "Run OpenAI Codex once or set CODEX_HOME to its data root"
    }

    fn find_files(&self) -> Vec<PathBuf> {
        find_codex_files()
    }

    fn parse_file(&self, path: &Path, timezone: Timezone, debug: bool) -> ParseOutput {
        if self.accounting_diagnostics {
            parse_codex_file_with_diagnostics(path, timezone, debug, self.scope)
        } else {
            parse_codex_file_with_scope(path, timezone, debug, self.scope)
        }
    }

    fn cache_partition(&self) -> &'static str {
        static DETAILS: std::sync::LazyLock<[String; 4]> = std::sync::LazyLock::new(|| {
            ["all", "interactive", "exec", "subagent"]
                .map(|scope| format!("{}:details-v1:{scope}", agent_sessions::VERSION))
        });
        static PARTITIONS: std::sync::LazyLock<[String; 4]> = std::sync::LazyLock::new(|| {
            ["all", "interactive", "exec", "subagent"]
                .map(|scope| format!("{}:{scope}", agent_sessions::VERSION))
        });
        let partitions = if self.accounting_diagnostics {
            &DETAILS
        } else {
            &PARTITIONS
        };
        match self.scope {
            CodexScope::All => &partitions[0],
            CodexScope::Interactive => &partitions[1],
            CodexScope::Exec => &partitions[2],
            CodexScope::Subagent => &partitions[3],
        }
    }
}
