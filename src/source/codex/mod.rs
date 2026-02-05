//! OpenAI Codex CLI data source
//!
//! Parses JSONL logs from ~/.codex/sessions/ directory.
//! Codex log format uses cumulative token counts that need delta computation.

mod config;
mod parser;

pub use config::CodexSource;
