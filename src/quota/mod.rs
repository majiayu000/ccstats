//! Official quota snapshots and Claude Code statusline hook input.

mod hook;
mod snapshot;

pub(crate) use hook::{
    ClaudeHook, Confidence, CostSource, format_reset_countdown, read_claude_hook_from_stdin,
};
pub(crate) use snapshot::{
    append_claude_snapshot, load_claude_quota_state, scale_observed_to_full_window,
};
