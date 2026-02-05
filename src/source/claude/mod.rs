//! Claude Code data source
//!
//! Parses JSONL logs from ~/.claude/projects/ directory.

mod config;
mod parser;

pub(crate) use config::ClaudeSource;
