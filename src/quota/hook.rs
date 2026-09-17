//! Claude Code statusline hook JSON (2.1.80+). Extra fields are ignored.

use std::io::{IsTerminal, Read};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum CostSource {
    #[default]
    Auto,
    Ccstats,
    Cc,
    Both,
}

impl CostSource {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Ccstats => "ccstats",
            Self::Cc => "cc",
            Self::Both => "both",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Confidence {
    Official,
    Estimated,
}

impl Confidence {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Official => "official",
            Self::Estimated => "estimated",
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub(crate) struct RateWindow {
    pub used_percentage: Option<f64>,
    pub resets_at: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, Default, PartialEq)]
pub(crate) struct RateLimits {
    pub five_hour: Option<RateWindow>,
    pub seven_day: Option<RateWindow>,
}

#[derive(Debug, Clone, Deserialize, Default, PartialEq)]
pub(crate) struct HookCost {
    pub total_cost_usd: Option<f64>,
}

#[derive(Debug, Clone, Deserialize, Default, PartialEq)]
#[allow(clippy::struct_field_names)]
pub(crate) struct ContextWindow {
    pub used_percentage: Option<f64>,
    pub context_window_size: Option<i64>,
    pub current_usage: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize, Default, PartialEq)]
pub(crate) struct ClaudeHook {
    pub version: Option<String>,
    pub rate_limits: Option<RateLimits>,
    pub context_window: Option<ContextWindow>,
    pub cost: Option<HookCost>,
}

impl ClaudeHook {
    pub(crate) fn has_official_windows(&self) -> bool {
        self.rate_limits
            .as_ref()
            .is_some_and(|limits| limits.five_hour.is_some() || limits.seven_day.is_some())
    }

    pub(crate) fn five_hour(&self) -> Option<&RateWindow> {
        self.rate_limits.as_ref()?.five_hour.as_ref()
    }

    pub(crate) fn seven_day(&self) -> Option<&RateWindow> {
        self.rate_limits.as_ref()?.seven_day.as_ref()
    }

    pub(crate) fn cc_cost_usd(&self) -> Option<f64> {
        self.cost.as_ref()?.total_cost_usd.filter(|v| v.is_finite())
    }

    pub(crate) fn context_used_pct(&self) -> Option<f64> {
        self.context_window.as_ref()?.used_percentage
    }
}

pub(crate) fn parse_claude_hook(raw: &str) -> Option<ClaudeHook> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    serde_json::from_str(trimmed).ok()
}

pub(crate) fn read_claude_hook_from_stdin() -> Option<ClaudeHook> {
    let stdin = std::io::stdin();
    if stdin.is_terminal() {
        return None;
    }
    let mut buf = String::new();
    stdin.lock().read_to_string(&mut buf).ok()?;
    parse_claude_hook(&buf)
}

pub(crate) fn format_reset_countdown(resets_at: i64, now: DateTime<Utc>) -> String {
    let Some(reset) = DateTime::from_timestamp(resets_at, 0) else {
        return "?".to_string();
    };
    let delta = reset.signed_duration_since(now);
    if delta.num_seconds() <= 0 {
        return "now".to_string();
    }
    let hours = delta.num_hours();
    let minutes = delta.num_minutes() % 60;
    if hours >= 24 {
        format!("{}d{}h", hours / 24, hours % 24)
    } else if hours > 0 {
        format!("{hours}h{minutes}m")
    } else {
        format!("{minutes}m")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rate_limits_and_ignores_unknown_fields() {
        let hook = parse_claude_hook(
            r#"{
                "model": {"display_name": "Opus"},
                "cost": {"total_cost_usd": 1.25},
                "context_window": {"used_percentage": 42.0, "context_window_size": 200000},
                "rate_limits": {
                    "five_hour": {"used_percentage": 23.5, "resets_at": 1738425600},
                    "seven_day": {"used_percentage": 41.2, "resets_at": 1738857600}
                }
            }"#,
        )
        .unwrap();
        assert_eq!(hook.cc_cost_usd(), Some(1.25));
        assert_eq!(hook.context_used_pct(), Some(42.0));
        assert_eq!(hook.five_hour().unwrap().used_percentage, Some(23.5));
        assert!(hook.has_official_windows());
    }

    #[test]
    fn missing_rate_limits_is_still_a_hook() {
        let hook = parse_claude_hook(r#"{"cost":{"total_cost_usd":0.5}}"#).unwrap();
        assert!(!hook.has_official_windows());
        assert_eq!(hook.cc_cost_usd(), Some(0.5));
    }

    #[test]
    fn empty_and_invalid_are_none() {
        assert!(parse_claude_hook("").is_none());
        assert!(parse_claude_hook("not-json").is_none());
    }

    #[test]
    fn countdown_formats_hours_and_days() {
        let now = DateTime::from_timestamp(1_000_000, 0).unwrap();
        assert_eq!(format_reset_countdown(1_000_000 + 130 * 60, now), "2h10m");
        assert_eq!(format_reset_countdown(1_000_000 + 26 * 3600, now), "1d2h");
    }
}
