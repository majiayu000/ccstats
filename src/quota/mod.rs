//! Official quota snapshots and Claude Code statusline hook input.

mod forecast;
mod hook;
mod snapshot;

pub use forecast::{
    ForecastBasis, ForecastConfidence, ForecastInput, ForecastReason, LimitForecast, QuotaSample,
    forecast_limit,
};
pub(crate) use hook::{
    ClaudeHook, Confidence, CostSource, format_reset_countdown, read_claude_hook_from_stdin,
};
pub(crate) use snapshot::{append_claude_snapshot, load_claude_quota_state};
