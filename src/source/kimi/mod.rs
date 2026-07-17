//! Kimi Code CLI data source
//!
//! Parses per-turn usage records from `~/.kimi-code/sessions/` wire logs.

mod config;
mod parser;

pub(crate) use config::KimiSource;
