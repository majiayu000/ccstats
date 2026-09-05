//! `OpenAI` Codex CLI data source
//!
//! Parses JSONL logs from ~/.codex/sessions/ directory.
//! Codex log format uses cumulative token counts that need delta computation.

mod cache;
mod config;
mod parser;
mod quota;
mod quota_value;

pub(crate) use config::{CodexScope, CodexSource};
pub use quota::{CodexQuotaError, CodexQuotaStatus, CodexWeeklyQuota};
pub(crate) use quota::{load_weekly_quota, load_weekly_quota_from_home};
pub(crate) use quota_value::load_weekly_window_usage_from_home;
