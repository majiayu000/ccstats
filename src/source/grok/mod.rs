//! Grok CLI data source
//!
//! Prefers `updates.jsonl` `turn_completed.usage` records. Sessions without
//! those events still fall back to local context-token snapshots.

mod config;
mod parser;
mod usage;

pub(crate) use config::GrokSource;
