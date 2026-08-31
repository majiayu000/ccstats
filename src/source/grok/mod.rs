//! Grok CLI data source
//!
//! Prefers durable `logs/unified.jsonl` inference records. Installations
//! without unified inference telemetry still use session usage records.

mod config;
mod ledger_lock;
mod parser;
mod unified;
mod usage;

pub(crate) use config::GrokSource;
