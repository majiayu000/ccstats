use std::path::Path;

use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::pricing::{PricingDb, calculate_cost, sum_model_costs};
use crate::source::{
    CodexQuotaError, CodexWeeklyQuota, load_weekly_quota_from_home,
    load_weekly_window_usage_from_home,
};

/// API-equivalent value inferred for the active Codex weekly quota window.
///
/// This is an approximation based on local token pricing and the current
/// model/cache mix. It is not an official dollar allowance from the provider.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodexWeeklyValueEstimate {
    pub observed_at: chrono::DateTime<Utc>,
    pub window_started_at: chrono::DateTime<Utc>,
    pub resets_at: chrono::DateTime<Utc>,
    pub used_pct: f64,
    pub observed_cost_usd: f64,
    pub estimated_weekly_value_usd: f64,
    pub observed_tokens: i64,
    pub estimated_weekly_tokens: f64,
    pub valid_entries: i64,
    pub dedup_skipped_entries: i64,
}

/// Provider-authoritative window used to align a Codex weekly value estimate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodexWeeklyValueWindow {
    pub observed_at: chrono::DateTime<Utc>,
    pub resets_at: chrono::DateTime<Utc>,
    pub window_minutes: i64,
    pub used_pct: f64,
}

impl From<&CodexWeeklyQuota> for CodexWeeklyValueWindow {
    fn from(quota: &CodexWeeklyQuota) -> Self {
        Self {
            observed_at: quota.observed_at,
            resets_at: quota.resets_at,
            window_minutes: quota.window_minutes,
            used_pct: quota.used_pct,
        }
    }
}

/// Errors returned while estimating Codex weekly API-equivalent value.
#[derive(Debug, Error)]
pub enum CodexWeeklyValueError {
    #[error(transparent)]
    Quota(#[from] CodexQuotaError),

    #[error("cannot estimate weekly value while the provider-reported used percentage is zero")]
    ZeroUsagePercentage,

    #[error("no Codex token usage matched the active weekly quota window")]
    NoUsageInWindow,

    #[error("cannot price Codex models in the active weekly window: {models}")]
    UnpricedModels { models: String },

    #[error("no positive API-equivalent cost was available for the active weekly window")]
    NoPricedUsageInWindow,

    #[error("failed to load pricing data for the weekly value estimate: {message}")]
    Pricing { message: String },

    #[error("the weekly value calculation produced a non-finite result")]
    NonFiniteEstimate,
}

/// Errors returned by the explicit-window Codex value API.
#[derive(Debug, Error)]
pub enum CodexWeeklyValueWindowError {
    #[error("cannot estimate weekly value from an invalid provider window: {reason}")]
    InvalidWindow { reason: &'static str },

    #[error(transparent)]
    Estimate(#[from] CodexWeeklyValueError),
}

/// Load the newest provider-authoritative Codex weekly quota snapshot.
///
/// Pass an explicit Codex home to avoid process-global environment discovery.
/// The home must contain a `sessions` directory and is never replaced by a
/// fallback path. Passing `None` honors `CODEX_HOME` before `~/.codex`.
///
/// # Errors
///
/// Returns an error when the sessions directory or a usable weekly snapshot
/// cannot be found, a session file cannot be inspected or read, or the newest
/// snapshot is stale or malformed.
pub fn load_codex_weekly_quota(
    codex_home: Option<&Path>,
) -> Result<CodexWeeklyQuota, CodexQuotaError> {
    load_weekly_quota_from_home(codex_home)
}

/// Estimate the API-equivalent dollar value represented by the active Codex
/// weekly quota discovered from local session logs.
///
/// # Errors
///
/// Returns an error when the quota snapshot or matching usage cannot be read,
/// the used percentage is zero, any matched model cannot be priced, or pricing
/// data is unavailable.
pub fn estimate_codex_weekly_value(
    codex_home: Option<&Path>,
    offline: bool,
    strict_pricing: bool,
) -> Result<CodexWeeklyValueEstimate, CodexWeeklyValueError> {
    let quota = load_weekly_quota_from_home(codex_home)?;
    let pricing_db = PricingDb::try_load_quiet(offline, strict_pricing).map_err(|error| {
        CodexWeeklyValueError::Pricing {
            message: error.to_string(),
        }
    })?;
    estimate_codex_weekly_value_with_pricing(&quota, codex_home, &pricing_db)
}

/// Estimate Codex API-equivalent value against an explicit provider window.
///
/// Use this when the authoritative rate-limit response is newer than the
/// quota snapshot stored in local Codex session logs. Usage is read from the
/// exact timestamp interval and malformed matching logs fail closed.
///
/// # Errors
///
/// Returns an error when the window is invalid, matching usage cannot be read,
/// any matched model cannot be priced, or pricing data is unavailable.
pub fn estimate_codex_weekly_value_for_window(
    window: &CodexWeeklyValueWindow,
    codex_home: Option<&Path>,
    offline: bool,
    strict_pricing: bool,
) -> Result<CodexWeeklyValueEstimate, CodexWeeklyValueWindowError> {
    validate_window(window)?;
    let pricing_db = PricingDb::try_load_quiet(offline, strict_pricing).map_err(|error| {
        CodexWeeklyValueError::Pricing {
            message: error.to_string(),
        }
    })?;
    estimate_codex_weekly_value_for_window_with_pricing(window, codex_home, &pricing_db)
        .map_err(Into::into)
}

fn validate_window(window: &CodexWeeklyValueWindow) -> Result<(), CodexWeeklyValueWindowError> {
    if !window.used_pct.is_finite() || !(0.0..=100.0).contains(&window.used_pct) {
        return Err(CodexWeeklyValueWindowError::InvalidWindow {
            reason: "used percentage must be finite and between 0 and 100",
        });
    }
    if window.used_pct == 0.0 {
        return Err(CodexWeeklyValueWindowError::InvalidWindow {
            reason: "used percentage must be greater than zero",
        });
    }
    let duration = Duration::try_minutes(window.window_minutes).ok_or(
        CodexWeeklyValueWindowError::InvalidWindow {
            reason: "window length is outside the supported range",
        },
    )?;
    if duration <= Duration::zero() {
        return Err(CodexWeeklyValueWindowError::InvalidWindow {
            reason: "window length must be positive",
        });
    }
    let window_started_at = window.resets_at.checked_sub_signed(duration).ok_or(
        CodexWeeklyValueWindowError::InvalidWindow {
            reason: "window start is outside the supported timestamp range",
        },
    )?;
    if window.observed_at < window_started_at || window.observed_at >= window.resets_at {
        return Err(CodexWeeklyValueWindowError::InvalidWindow {
            reason: "observation must fall inside the active window",
        });
    }
    let now = Utc::now();
    if window.observed_at > now + Duration::minutes(5)
        || now - window.observed_at > Duration::hours(24)
    {
        return Err(CodexWeeklyValueWindowError::InvalidWindow {
            reason: "observation must describe the current provider window",
        });
    }
    Ok(())
}

pub(crate) fn estimate_codex_weekly_value_with_pricing(
    quota: &CodexWeeklyQuota,
    codex_home: Option<&Path>,
    pricing_db: &PricingDb,
) -> Result<CodexWeeklyValueEstimate, CodexWeeklyValueError> {
    estimate_codex_weekly_value_for_window_with_pricing(
        &CodexWeeklyValueWindow::from(quota),
        codex_home,
        pricing_db,
    )
}

fn estimate_codex_weekly_value_for_window_with_pricing(
    window: &CodexWeeklyValueWindow,
    codex_home: Option<&Path>,
    pricing_db: &PricingDb,
) -> Result<CodexWeeklyValueEstimate, CodexWeeklyValueError> {
    if window.used_pct <= 0.0 {
        return Err(CodexWeeklyValueError::ZeroUsagePercentage);
    }
    let duration = Duration::try_minutes(window.window_minutes)
        .ok_or(CodexWeeklyValueError::NonFiniteEstimate)?;
    let window_started_at = window
        .resets_at
        .checked_sub_signed(duration)
        .ok_or(CodexWeeklyValueError::NonFiniteEstimate)?;

    let usage =
        load_weekly_window_usage_from_home(window.observed_at, window_started_at, codex_home)?;
    let observed_tokens = usage.stats.total_tokens();
    if usage.valid_entries == 0 || observed_tokens <= 0 {
        return Err(CodexWeeklyValueError::NoUsageInWindow);
    }

    let mut unpriced_models: Vec<_> = usage
        .models
        .iter()
        .filter(|(model, stats)| !calculate_cost(stats, model, pricing_db).is_finite())
        .map(|(model, _)| model.clone())
        .collect();
    unpriced_models.sort();
    if !unpriced_models.is_empty() {
        return Err(CodexWeeklyValueError::UnpricedModels {
            models: unpriced_models.join(", "),
        });
    }

    let observed_cost_usd = sum_model_costs(&usage.models, pricing_db);
    if !observed_cost_usd.is_finite() || observed_cost_usd <= 0.0 {
        return Err(CodexWeeklyValueError::NoPricedUsageInWindow);
    }

    let scale = 100.0 / window.used_pct;
    let estimated_weekly_value_usd = observed_cost_usd * scale;
    let estimated_weekly_tokens = observed_tokens as f64 * scale;
    if !estimated_weekly_value_usd.is_finite() || !estimated_weekly_tokens.is_finite() {
        return Err(CodexWeeklyValueError::NonFiniteEstimate);
    }

    Ok(CodexWeeklyValueEstimate {
        observed_at: window.observed_at,
        window_started_at,
        resets_at: window.resets_at,
        used_pct: window.used_pct,
        observed_cost_usd,
        estimated_weekly_value_usd,
        observed_tokens,
        estimated_weekly_tokens,
        valid_entries: usage.valid_entries,
        dedup_skipped_entries: usage.dedup_skipped_entries,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window() -> CodexWeeklyValueWindow {
        let observed_at = Utc::now() - Duration::hours(1);
        CodexWeeklyValueWindow {
            observed_at,
            resets_at: observed_at + Duration::days(6),
            window_minutes: 10_080,
            used_pct: 25.0,
        }
    }

    #[test]
    fn explicit_window_rejects_extreme_duration_without_panicking() {
        let error = validate_window(&CodexWeeklyValueWindow {
            window_minutes: i64::MAX,
            ..window()
        })
        .unwrap_err();

        assert!(matches!(
            error,
            CodexWeeklyValueWindowError::InvalidWindow { .. }
        ));
    }

    #[test]
    fn explicit_window_rejects_invalid_percentages() {
        for used_pct in [-1.0, f64::INFINITY, f64::NEG_INFINITY, f64::NAN] {
            let error = validate_window(&CodexWeeklyValueWindow {
                used_pct,
                ..window()
            })
            .unwrap_err();
            assert!(matches!(
                error,
                CodexWeeklyValueWindowError::InvalidWindow { .. }
            ));
        }
    }
}
