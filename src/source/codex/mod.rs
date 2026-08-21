//! `OpenAI` Codex CLI data source
//!
//! Parses JSONL logs from ~/.codex/sessions/ directory.
//! Codex log format uses cumulative token counts that need delta computation.

mod config;
mod parser;
mod quota;

pub(crate) use config::{CodexScope, CodexSource};
pub(crate) use quota::{CodexQuotaReport, QuotaStatus, load_weekly_quota};
