//! Burn-rate and exhaustion forecast for one provider limit window.
//!
//! Pure math over official used-percentage observations. Callers supply the
//! current observation, its reset time, an optional window start, and any
//! recorded history; nothing here reads files or clocks.
//!
//! Basis order:
//! 1. `snapshot_history`: least-squares slope over at least
//!    [`MIN_HISTORY_SAMPLES`] observations of the current reset window that
//!    span at least [`MIN_HISTORY_SPAN_MINUTES`].
//! 2. `window_average`: `used_pct / elapsed` since the window start, once at
//!    least [`MIN_AVERAGE_ELAPSED_MINUTES`] (or 5% of the window) has elapsed.
//!
//! Otherwise the forecast stays empty with a reason. A forecast is always an
//! estimate and serializes with `"source": "estimated"`.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

/// Minimum observations in the current window before history is trusted.
pub const MIN_HISTORY_SAMPLES: usize = 3;
/// Minimum time covered by those observations.
pub const MIN_HISTORY_SPAN_MINUTES: i64 = 15;
/// Minimum elapsed window time before a window average is trusted.
pub const MIN_AVERAGE_ELAPSED_MINUTES: i64 = 15;
/// Two reset timestamps closer than this belong to the same window.
const RESET_TOLERANCE_SECONDS: i64 = 5 * 60;
/// Burn below this is treated as flat.
const FLAT_PCT_PER_HOUR: f64 = 1e-6;
/// A used percentage that drops by more than this marks a window reset.
const RESET_DROP_PCT: f64 = 0.5;

/// One official used-percentage observation of a limit window.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct QuotaSample {
    pub observed_at: DateTime<Utc>,
    pub used_pct: f64,
    /// Reset time reported with this observation; `None` when unknown.
    pub resets_at: Option<DateTime<Utc>>,
}

impl QuotaSample {
    #[must_use]
    pub fn new(
        observed_at: DateTime<Utc>,
        used_pct: f64,
        resets_at: Option<DateTime<Utc>>,
    ) -> Self {
        Self {
            observed_at,
            used_pct,
            resets_at,
        }
    }
}

/// How a burn rate was derived.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum ForecastBasis {
    /// Slope over recorded official snapshots in the current reset window.
    SnapshotHistory,
    /// Current used percentage divided by elapsed window time.
    WindowAverage,
}

impl ForecastBasis {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SnapshotHistory => "snapshot_history",
            Self::WindowAverage => "window_average",
        }
    }
}

/// Coarse trust level of a forecast.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum ForecastConfidence {
    Low,
    Medium,
}

/// Why a forecast has no projected exhaustion time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum ForecastReason {
    /// The window has no official used percentage.
    MissingUsedPct,
    /// The window has no reset time.
    MissingResetTime,
    /// The reset time has passed; earlier history no longer applies.
    WindowReset,
    /// Too few observations and too little elapsed window time.
    InsufficientHistory,
    /// Usage is flat or falling, so the window is not on pace to run out.
    NotIncreasing,
    /// The window is already at or above 100%.
    AlreadyExhausted,
}

impl ForecastReason {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MissingUsedPct => "missing_used_pct",
            Self::MissingResetTime => "missing_reset_time",
            Self::WindowReset => "window_reset",
            Self::InsufficientHistory => "insufficient_history",
            Self::NotIncreasing => "not_increasing",
            Self::AlreadyExhausted => "already_exhausted",
        }
    }
}

/// Estimated burn rate and exhaustion time for one limit window.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct LimitForecast {
    /// Percentage points per hour; `None` when it cannot be estimated.
    pub burn_pct_per_hour: Option<f64>,
    /// When the window reaches 100% at the current pace.
    pub projected_exhaustion_at: Option<DateTime<Utc>>,
    /// `Some(true)` when 100% is reached before the reset.
    pub exhausts_before_reset: Option<bool>,
    pub basis: Option<ForecastBasis>,
    pub confidence: Option<ForecastConfidence>,
    /// Observations of the current window used for `snapshot_history`.
    pub samples: usize,
    /// Always `"estimated"`.
    pub source: String,
    /// Set whenever `projected_exhaustion_at` is `None`.
    pub reason: Option<ForecastReason>,
}

impl LimitForecast {
    fn unavailable(reason: ForecastReason, samples: usize) -> Self {
        Self {
            burn_pct_per_hour: None,
            projected_exhaustion_at: None,
            exhausts_before_reset: None,
            basis: None,
            confidence: None,
            samples,
            source: "estimated".to_string(),
            reason: Some(reason),
        }
    }

    /// A forecast for a window without an official used percentage.
    #[must_use]
    pub fn missing_used_pct() -> Self {
        Self::unavailable(ForecastReason::MissingUsedPct, 0)
    }
}

/// Inputs for [`forecast_limit`].
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct ForecastInput<'a> {
    /// Latest official used percentage.
    pub used_pct: Option<f64>,
    /// When `used_pct` was observed.
    pub observed_at: DateTime<Utc>,
    pub resets_at: Option<DateTime<Utc>>,
    /// Start of the window, when the provider window length is known.
    pub window_start: Option<DateTime<Utc>>,
    /// Recorded observations in any order; other windows are filtered out.
    pub history: &'a [QuotaSample],
    pub now: DateTime<Utc>,
}

impl<'a> ForecastInput<'a> {
    #[must_use]
    pub fn new(
        used_pct: Option<f64>,
        observed_at: DateTime<Utc>,
        resets_at: Option<DateTime<Utc>>,
        window_start: Option<DateTime<Utc>>,
        history: &'a [QuotaSample],
        now: DateTime<Utc>,
    ) -> Self {
        Self {
            used_pct,
            observed_at,
            resets_at,
            window_start,
            history,
            now,
        }
    }
}

/// Projects whether and when a limit window reaches 100% before it resets.
#[must_use]
pub fn forecast_limit(input: &ForecastInput<'_>) -> LimitForecast {
    let Some(used) = input.used_pct.filter(|pct| pct.is_finite()) else {
        return LimitForecast::unavailable(ForecastReason::MissingUsedPct, 0);
    };
    let Some(resets_at) = input.resets_at else {
        return LimitForecast::unavailable(ForecastReason::MissingResetTime, 0);
    };
    if input.now >= resets_at {
        return LimitForecast::unavailable(ForecastReason::WindowReset, 0);
    }

    let points = current_window_points(input, used, resets_at);
    let samples = points.len();
    let estimate = history_rate(&points)
        .map(|rate| {
            (
                rate,
                ForecastBasis::SnapshotHistory,
                ForecastConfidence::Medium,
            )
        })
        .or_else(|| {
            average_rate(used, input.observed_at, input.window_start, resets_at)
                .map(|rate| (rate, ForecastBasis::WindowAverage, ForecastConfidence::Low))
        });
    let Some((rate, basis, confidence)) = estimate else {
        let mut forecast = LimitForecast::unavailable(ForecastReason::InsufficientHistory, samples);
        if used >= 100.0 {
            forecast.exhausts_before_reset = Some(true);
            forecast.reason = Some(ForecastReason::AlreadyExhausted);
        }
        return forecast;
    };

    let mut forecast = LimitForecast {
        burn_pct_per_hour: Some(round_2(rate)),
        projected_exhaustion_at: None,
        exhausts_before_reset: None,
        basis: Some(basis),
        confidence: Some(confidence),
        samples,
        source: "estimated".to_string(),
        reason: None,
    };
    if used >= 100.0 {
        forecast.exhausts_before_reset = Some(true);
        forecast.reason = Some(ForecastReason::AlreadyExhausted);
        return forecast;
    }
    if rate <= FLAT_PCT_PER_HOUR {
        forecast.exhausts_before_reset = Some(false);
        forecast.reason = Some(ForecastReason::NotIncreasing);
        return forecast;
    }
    let seconds = ((100.0 - used) / rate * 3600.0).round();
    let projected = (seconds < i64::MAX as f64)
        .then(|| Duration::try_seconds(seconds as i64))
        .flatten()
        .and_then(|delta| input.observed_at.checked_add_signed(delta));
    if let Some(at) = projected {
        forecast.projected_exhaustion_at = Some(at);
        forecast.exhausts_before_reset = Some(at < resets_at);
    } else {
        forecast.exhausts_before_reset = Some(false);
        forecast.reason = Some(ForecastReason::NotIncreasing);
    }
    forecast
}

fn same_window(sample_reset: Option<DateTime<Utc>>, resets_at: DateTime<Utc>) -> bool {
    sample_reset
        .is_some_and(|reset| (reset - resets_at).num_seconds().abs() <= RESET_TOLERANCE_SECONDS)
}

/// Observations of the current window, oldest first, starting after the
/// last drop in used percentage (a reset the timestamps did not reveal).
fn current_window_points(
    input: &ForecastInput<'_>,
    used: f64,
    resets_at: DateTime<Utc>,
) -> Vec<(DateTime<Utc>, f64)> {
    let mut points: Vec<(DateTime<Utc>, f64)> = input
        .history
        .iter()
        .filter(|sample| sample.used_pct.is_finite())
        .filter(|sample| same_window(sample.resets_at, resets_at))
        .filter(|sample| sample.observed_at <= input.observed_at)
        .filter(|sample| {
            input
                .window_start
                .is_none_or(|start| sample.observed_at >= start)
        })
        .map(|sample| (sample.observed_at, sample.used_pct))
        .collect();
    points.push((input.observed_at, used));
    points.sort_by_key(|(at, _)| *at);
    points.dedup_by(|later, earlier| {
        if later.0 == earlier.0 {
            *earlier = *later;
            true
        } else {
            false
        }
    });
    if let Some(last_drop) = points
        .windows(2)
        .rposition(|pair| pair[1].1 + RESET_DROP_PCT < pair[0].1)
    {
        points.drain(..=last_drop);
    }
    points
}

fn hours_between(from: DateTime<Utc>, to: DateTime<Utc>) -> f64 {
    (to - from).num_milliseconds() as f64 / 3_600_000.0
}

/// Least-squares slope in percentage points per hour.
fn history_rate(points: &[(DateTime<Utc>, f64)]) -> Option<f64> {
    if points.len() < MIN_HISTORY_SAMPLES {
        return None;
    }
    let first = points.first()?.0;
    let last = points.last()?.0;
    if last - first < Duration::minutes(MIN_HISTORY_SPAN_MINUTES) {
        return None;
    }
    let n = points.len() as f64;
    let xs: Vec<f64> = points
        .iter()
        .map(|(at, _)| hours_between(first, *at))
        .collect();
    let mean_x = xs.iter().sum::<f64>() / n;
    let mean_y = points.iter().map(|(_, pct)| pct).sum::<f64>() / n;
    let (mut num, mut den) = (0.0, 0.0);
    for (x, (_, y)) in xs.iter().zip(points) {
        num += (x - mean_x) * (y - mean_y);
        den += (x - mean_x) * (x - mean_x);
    }
    let slope = num / den;
    (den > 0.0 && slope.is_finite()).then_some(slope)
}

fn average_rate(
    used: f64,
    observed_at: DateTime<Utc>,
    window_start: Option<DateTime<Utc>>,
    resets_at: DateTime<Utc>,
) -> Option<f64> {
    let start = window_start?;
    let window = resets_at - start;
    let min_elapsed = Duration::minutes(MIN_AVERAGE_ELAPSED_MINUTES).max(window / 20);
    if observed_at - start < min_elapsed {
        return None;
    }
    let rate = used / hours_between(start, observed_at);
    rate.is_finite().then_some(rate)
}

fn round_2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(hours: f64) -> DateTime<Utc> {
        DateTime::from_timestamp(1_800_000_000, 0).unwrap()
            + Duration::seconds((hours * 3600.0) as i64)
    }

    fn samples(points: &[(f64, f64)], reset: DateTime<Utc>) -> Vec<QuotaSample> {
        points
            .iter()
            .map(|(hours, pct)| QuotaSample::new(t(*hours), *pct, Some(reset)))
            .collect()
    }

    fn run(
        used: f64,
        observed_h: f64,
        reset: DateTime<Utc>,
        start: Option<DateTime<Utc>>,
        history: &[QuotaSample],
    ) -> LimitForecast {
        forecast_limit(&ForecastInput::new(
            Some(used),
            t(observed_h),
            Some(reset),
            start,
            history,
            t(observed_h),
        ))
    }

    #[test]
    fn too_few_samples_without_window_start_is_null() {
        let reset = t(5.0);
        let history = samples(&[(0.0, 10.0)], reset);
        let forecast = run(12.0, 0.5, reset, None, &history);
        assert_eq!(forecast.burn_pct_per_hour, None);
        assert_eq!(forecast.projected_exhaustion_at, None);
        assert_eq!(forecast.exhausts_before_reset, None);
        assert_eq!(forecast.basis, None);
        assert_eq!(forecast.samples, 2);
        assert_eq!(forecast.reason, Some(ForecastReason::InsufficientHistory));
        assert_eq!(forecast.source, "estimated");
    }

    #[test]
    fn samples_spanning_too_little_time_are_not_history() {
        let reset = t(5.0);
        let history = samples(&[(0.0, 10.0), (0.05, 11.0), (0.1, 12.0)], reset);
        let forecast = run(13.0, 0.15, reset, None, &history);
        assert_eq!(forecast.reason, Some(ForecastReason::InsufficientHistory));
    }

    #[test]
    fn early_window_average_is_null() {
        let reset = t(5.0);
        let forecast = run(3.0, 0.1, reset, Some(t(0.0)), &[]);
        assert_eq!(forecast.reason, Some(ForecastReason::InsufficientHistory));
    }

    #[test]
    fn flat_history_never_exhausts() {
        let reset = t(5.0);
        let history = samples(&[(0.0, 40.0), (0.5, 40.0), (1.0, 40.0)], reset);
        let forecast = run(40.0, 1.5, reset, None, &history);
        assert_eq!(forecast.burn_pct_per_hour, Some(0.0));
        assert_eq!(forecast.projected_exhaustion_at, None);
        assert_eq!(forecast.exhausts_before_reset, Some(false));
        assert_eq!(forecast.reason, Some(ForecastReason::NotIncreasing));
    }

    #[test]
    fn negative_slope_without_reset_drop_never_exhausts() {
        let reset = t(5.0);
        // Small decreases (rounding) stay below the reset-drop threshold.
        let history = samples(&[(0.0, 40.4), (0.5, 40.2), (1.0, 40.1)], reset);
        let forecast = run(40.0, 1.5, reset, None, &history);
        assert!(forecast.burn_pct_per_hour.unwrap() < 0.0);
        assert_eq!(forecast.exhausts_before_reset, Some(false));
        assert_eq!(forecast.reason, Some(ForecastReason::NotIncreasing));
    }

    #[test]
    fn history_projects_exhaustion_before_reset() {
        let reset = t(5.0);
        let history = samples(&[(0.0, 10.0), (1.0, 30.0), (2.0, 50.0)], reset);
        let forecast = run(50.0, 2.0, reset, Some(t(0.0)), &history);
        assert_eq!(forecast.burn_pct_per_hour, Some(20.0));
        assert_eq!(forecast.basis, Some(ForecastBasis::SnapshotHistory));
        assert_eq!(forecast.confidence, Some(ForecastConfidence::Medium));
        assert_eq!(forecast.samples, 3);
        assert_eq!(forecast.projected_exhaustion_at, Some(t(4.5)));
        assert_eq!(forecast.exhausts_before_reset, Some(true));
        assert_eq!(forecast.reason, None);
    }

    #[test]
    fn history_projects_exhaustion_after_reset() {
        let reset = t(5.0);
        let history = samples(&[(0.0, 10.0), (1.0, 15.0), (2.0, 20.0)], reset);
        let forecast = run(20.0, 2.0, reset, None, &history);
        assert_eq!(forecast.burn_pct_per_hour, Some(5.0));
        assert_eq!(forecast.projected_exhaustion_at, Some(t(18.0)));
        assert_eq!(forecast.exhausts_before_reset, Some(false));
        assert_eq!(forecast.reason, None);
    }

    #[test]
    fn window_average_fallback_is_low_confidence() {
        let reset = t(168.0);
        let start = t(0.0);
        let forecast = run(48.0, 24.0, reset, Some(start), &[]);
        assert_eq!(forecast.basis, Some(ForecastBasis::WindowAverage));
        assert_eq!(forecast.confidence, Some(ForecastConfidence::Low));
        assert_eq!(forecast.burn_pct_per_hour, Some(2.0));
        assert_eq!(forecast.projected_exhaustion_at, Some(t(50.0)));
        assert_eq!(forecast.exhausts_before_reset, Some(true));
    }

    #[test]
    fn reset_boundary_clears_history() {
        let old_reset = t(1.0);
        let reset = t(6.0);
        let mut history = samples(&[(-2.0, 10.0), (-1.0, 60.0), (0.5, 95.0)], old_reset);
        history.extend(samples(&[(1.5, 2.0)], reset));
        let forecast = run(4.0, 2.0, reset, None, &history);
        assert_eq!(forecast.samples, 2);
        assert_eq!(forecast.reason, Some(ForecastReason::InsufficientHistory));
    }

    #[test]
    fn drop_in_used_pct_clears_history_without_reset_change() {
        let reset = t(5.0);
        let history = samples(&[(0.0, 80.0), (0.5, 90.0), (1.0, 5.0), (1.5, 10.0)], reset);
        let forecast = run(15.0, 2.0, reset, None, &history);
        assert_eq!(forecast.samples, 3);
        assert_eq!(forecast.burn_pct_per_hour, Some(10.0));
    }

    #[test]
    fn passed_reset_is_window_reset() {
        let reset = t(1.0);
        let forecast = forecast_limit(&ForecastInput::new(
            Some(50.0),
            t(0.5),
            Some(reset),
            None,
            &[],
            t(1.5),
        ));
        assert_eq!(forecast.reason, Some(ForecastReason::WindowReset));
        assert_eq!(forecast.burn_pct_per_hour, None);
    }

    #[test]
    fn missing_inputs_carry_reasons() {
        let missing_pct = forecast_limit(&ForecastInput::new(
            None,
            t(0.0),
            Some(t(1.0)),
            None,
            &[],
            t(0.0),
        ));
        assert_eq!(missing_pct.reason, Some(ForecastReason::MissingUsedPct));
        let missing_reset = forecast_limit(&ForecastInput::new(
            Some(5.0),
            t(0.0),
            None,
            None,
            &[],
            t(0.0),
        ));
        assert_eq!(missing_reset.reason, Some(ForecastReason::MissingResetTime));
    }

    #[test]
    fn exhausted_window_reports_exhausted() {
        let reset = t(5.0);
        let history = samples(&[(0.0, 60.0), (1.0, 80.0)], reset);
        let forecast = run(100.0, 2.0, reset, None, &history);
        assert_eq!(forecast.burn_pct_per_hour, Some(20.0));
        assert_eq!(forecast.basis, Some(ForecastBasis::SnapshotHistory));
        assert_eq!(forecast.confidence, Some(ForecastConfidence::Medium));
        assert_eq!(forecast.samples, 3);
        assert_eq!(forecast.exhausts_before_reset, Some(true));
        assert_eq!(forecast.projected_exhaustion_at, None);
        assert_eq!(forecast.reason, Some(ForecastReason::AlreadyExhausted));
    }

    #[test]
    fn exhausted_window_without_rate_reports_exhausted() {
        let reset = t(5.0);
        let short_history = samples(&[(0.0, 60.0), (0.05, 80.0)], reset);
        for used in [100.0, 101.0] {
            for (start, history, expected_samples) in [
                (None, &[][..], 1),
                (Some(t(0.0)), &[][..], 1),
                (None, short_history.as_slice(), 3),
            ] {
                let forecast = run(used, 0.1, reset, start, history);
                assert_eq!(forecast.reason, Some(ForecastReason::AlreadyExhausted));
                assert_eq!(forecast.exhausts_before_reset, Some(true));
                assert_eq!(forecast.projected_exhaustion_at, None);
                assert_eq!(forecast.burn_pct_per_hour, None);
                assert_eq!(forecast.basis, None);
                assert_eq!(forecast.confidence, None);
                assert_eq!(forecast.samples, expected_samples);
                assert_eq!(forecast.source, "estimated");
            }
        }
    }

    #[test]
    fn exhausted_observation_still_requires_active_reset() {
        for (reset, reason) in [
            (None, ForecastReason::MissingResetTime),
            (Some(t(0.0)), ForecastReason::WindowReset),
        ] {
            let forecast = forecast_limit(&ForecastInput::new(
                Some(100.0),
                t(0.1),
                reset,
                None,
                &[],
                t(0.1),
            ));
            assert_eq!(forecast.reason, Some(reason));
            assert_eq!(forecast.exhausts_before_reset, None);
            assert_eq!(forecast.burn_pct_per_hour, None);
            assert_eq!(forecast.samples, 0);
        }
    }

    #[test]
    fn json_shape_uses_snake_case_and_estimated_label() {
        let reset = t(5.0);
        let history = samples(&[(0.0, 10.0), (1.0, 30.0)], reset);
        let value = serde_json::to_value(run(50.0, 2.0, reset, None, &history)).unwrap();
        assert_eq!(value["basis"], "snapshot_history");
        assert_eq!(value["confidence"], "medium");
        assert_eq!(value["source"], "estimated");
        assert!(value["reason"].is_null());
        assert_eq!(value["exhausts_before_reset"], true);
    }
}
