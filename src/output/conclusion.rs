//! One-line human conclusion for table-mode period reports.

use std::cmp::Reverse;
use std::collections::HashMap;

use chrono::NaiveDate;

use crate::consts::DATE_FORMAT;
use crate::core::{DayStats, Stats};
use crate::output::format::{NumberFormat, format_compact, format_cost, format_number};
use crate::pricing::{
    CostDisplayMode, CurrencyConverter, PricingDb, calculate_display_cost, sum_display_model_costs,
};
use crate::source::CostCoverage;

const PACE_PRIOR_MIN: usize = 3;
const PACE_PRIOR_MAX: usize = 7;
const PACE_EVEN_RATIO: f64 = 0.05;

pub(super) struct PeriodConclusionInput<'a> {
    pub(super) day_stats: &'a HashMap<String, DayStats>,
    pub(super) displayed: &'a HashMap<String, DayStats>,
    pub(super) is_today: bool,
    pub(super) source_count: Option<usize>,
    pub(super) compact: bool,
    pub(super) show_cost: bool,
    pub(super) number_format: NumberFormat,
    pub(super) currency: Option<&'a CurrencyConverter>,
    pub(super) cost_mode: CostDisplayMode,
    pub(super) pricing_db: &'a PricingDb,
    pub(super) footer_cost: f64,
    pub(super) footer_cost_text: &'a str,
}

pub(super) fn period_conclusion_line(input: &PeriodConclusionInput<'_>) -> Option<String> {
    if input.displayed.is_empty() {
        return None;
    }

    let mut total_stats = Stats::default();
    for data in input.displayed.values() {
        total_stats.add(&data.stats);
    }

    let label = if input.is_today {
        "Today"
    } else {
        "This period"
    };
    let sources = match input.source_count {
        Some(count) if count >= 2 => format!(" ({count} sources)"),
        _ => String::new(),
    };
    let token_text = if input.compact {
        format_compact(total_stats.total_tokens(), input.number_format)
    } else {
        format_number(total_stats.total_tokens(), input.number_format)
    };

    let mut line = format!("{label}{sources}: {token_text} tokens");
    if let Some(cost_clause) = cost_clause(input) {
        line.push_str(&cost_clause);
    }
    if !input.compact
        && let Some(pace) = pace_clause(input)
    {
        line.push_str(&pace);
    }
    line.push('.');
    Some(line)
}

fn cost_clause(input: &PeriodConclusionInput<'_>) -> Option<String> {
    if !input.show_cost {
        return None;
    }

    let amount = input.footer_cost_text.trim();
    let unknown =
        input.footer_cost.is_nan() || amount.is_empty() || amount.eq_ignore_ascii_case("N/A");
    if unknown {
        return Some(", cost unknown".to_string());
    }

    if is_floor(input) {
        Some(format!(", {} (floor)", floor_amount(amount)))
    } else {
        Some(format!(", {amount}"))
    }
}

fn is_floor(input: &PeriodConclusionInput<'_>) -> bool {
    if input.footer_cost.is_nan() {
        return false;
    }
    let coverage_floor = CostCoverage::from_stats(input.displayed.values().map(|day| &day.stats))
        .is_some_and(CostCoverage::cost_is_lower_bound);
    coverage_floor || has_unpriced_models(input) || input.footer_cost_text.contains('≥')
}

fn has_unpriced_models(input: &PeriodConclusionInput<'_>) -> bool {
    input.displayed.values().any(|data| {
        data.models.iter().any(|(model, stats)| {
            calculate_display_cost(stats, model, input.pricing_db, input.cost_mode).is_nan()
        })
    })
}

fn floor_amount(amount: &str) -> String {
    let trimmed = amount.trim();
    if trimmed.starts_with('≥') {
        if trimmed.starts_with("≥ ") {
            trimmed.to_string()
        } else {
            trimmed.replacen('≥', "≥ ", 1)
        }
    } else {
        format!("≥ {trimmed}")
    }
}

fn pace_clause(input: &PeriodConclusionInput<'_>) -> Option<String> {
    if !input.is_today {
        return None;
    }
    let today_key = today_key(input.day_stats)?;
    let prior = prior_complete_days(input.day_stats, today_key);
    if prior.len() < PACE_PRIOR_MIN {
        return None;
    }

    let today = input.day_stats.get(today_key)?;
    let n = prior.len();
    let compare_cost = input.show_cost
        && today_metric_cost(today, input).is_finite()
        && prior
            .iter()
            .all(|day| today_metric_cost(day, input).is_finite());

    let (today_value, mean, mean_text) = if compare_cost {
        let today_cost = today_metric_cost(today, input);
        let mean = prior
            .iter()
            .map(|day| today_metric_cost(day, input))
            .sum::<f64>()
            / n as f64;
        (today_cost, mean, format_cost(mean, input.currency))
    } else {
        let today_tokens = today.stats.total_tokens() as f64;
        let mean = prior
            .iter()
            .map(|day| day.stats.total_tokens() as f64)
            .sum::<f64>()
            / n as f64;
        let mean_tokens = mean.round() as i64;
        let mean_text = if input.compact {
            format_compact(mean_tokens, input.number_format)
        } else {
            format_number(mean_tokens, input.number_format)
        };
        (today_tokens, mean, format!("{mean_text} tokens"))
    };

    let relation = pace_relation(today_value, mean);
    Some(format!(". {relation} the last {n}-day mean ({mean_text})"))
}

fn today_metric_cost(day: &DayStats, input: &PeriodConclusionInput<'_>) -> f64 {
    sum_display_model_costs(&day.models, input.pricing_db, input.cost_mode)
}

fn today_key(day_stats: &HashMap<String, DayStats>) -> Option<&str> {
    day_stats
        .keys()
        .filter(|key| NaiveDate::parse_from_str(key, DATE_FORMAT).is_ok())
        .max()
        .map(String::as_str)
}

fn prior_complete_days<'a>(
    day_stats: &'a HashMap<String, DayStats>,
    today: &str,
) -> Vec<&'a DayStats> {
    let Ok(today_date) = NaiveDate::parse_from_str(today, DATE_FORMAT) else {
        return Vec::new();
    };
    let mut prior: Vec<_> = day_stats
        .iter()
        .filter_map(|(key, day)| {
            let date = NaiveDate::parse_from_str(key, DATE_FORMAT).ok()?;
            (date < today_date).then_some((date, day))
        })
        .collect();
    prior.sort_by_key(|(date, _)| Reverse(*date));
    prior.truncate(PACE_PRIOR_MAX);
    prior.into_iter().map(|(_, day)| day).collect()
}

fn pace_relation(today: f64, mean: f64) -> &'static str {
    if !today.is_finite() || !mean.is_finite() {
        return "About even with";
    }
    if mean == 0.0 {
        return if today == 0.0 {
            "About even with"
        } else {
            "Faster than"
        };
    }
    let delta = (today - mean).abs() / mean.abs();
    if delta <= PACE_EVEN_RATIO {
        "About even with"
    } else if today > mean {
        "Faster than"
    } else {
        "Slower than"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pricing::CostDisplayMode;

    fn recorded(
        input_tokens: i64,
        output_tokens: i64,
        cost: f64,
        priced: i64,
        coverage: i64,
        model: &str,
    ) -> DayStats {
        let stats = Stats {
            input_tokens,
            output_tokens,
            count: 1,
            recorded_cost_usd: cost,
            recorded_cost_entries: 1,
            api_equivalent_priced_tokens: priced,
            api_equivalent_coverage_tokens: coverage,
            ..Default::default()
        };
        DayStats {
            stats: stats.clone(),
            models: HashMap::from([(model.to_string(), stats)]),
        }
    }

    fn unpriced(input_tokens: i64, output_tokens: i64) -> Stats {
        Stats {
            input_tokens,
            output_tokens,
            count: 1,
            api_equivalent_priced_tokens: 0,
            api_equivalent_coverage_tokens: input_tokens + output_tokens,
            ..Default::default()
        }
    }

    fn line_for(
        displayed: &HashMap<String, DayStats>,
        extra_days: HashMap<String, DayStats>,
        is_today: bool,
        show_cost: bool,
        compact: bool,
        source_count: Option<usize>,
    ) -> Option<String> {
        let pricing_db = PricingDb::default();
        let footer_cost = displayed.values().fold(0.0, |total, day| {
            total + sum_display_model_costs(&day.models, &pricing_db, CostDisplayMode::Total)
        });
        let footer_cost_text = format_cost(footer_cost, None);
        let mut day_stats = displayed.clone();
        day_stats.extend(extra_days);
        period_conclusion_line(&PeriodConclusionInput {
            day_stats: &day_stats,
            displayed,
            is_today,
            source_count,
            compact,
            show_cost,
            number_format: NumberFormat::default(),
            currency: None,
            cost_mode: CostDisplayMode::Total,
            pricing_db: &pricing_db,
            footer_cost,
            footer_cost_text: &footer_cost_text,
        })
    }

    fn today_only(day: DayStats) -> HashMap<String, DayStats> {
        HashMap::from([("2026-09-03".to_string(), day)])
    }

    #[test]
    fn exact_cost_uses_plain_formatted_amount() {
        let days = today_only(recorded(1_000, 500, 1.25, 1_500, 1_500, "claude-sonnet"));
        let line = line_for(&days, HashMap::new(), true, true, false, None).expect("conclusion");
        assert_eq!(line, "Today: 1,500 tokens, $1.25.");
        assert!(!line.contains('≥'));
        assert!(!line.contains("floor"));
        assert!(!line.contains("$0.00"));
    }

    #[test]
    fn floor_cost_prefixes_ge_and_mentions_floor() {
        let mut mixed = recorded(1_000, 500, 2.00, 1_500, 1_610, "claude-sonnet");
        let extra = unpriced(100, 10);
        mixed.stats.add(&extra);
        mixed.models.insert("mystery-model".to_string(), extra);
        let displayed = today_only(mixed);
        let line =
            line_for(&displayed, HashMap::new(), true, true, false, None).expect("conclusion");
        assert_eq!(line, "Today: 1,610 tokens, ≥ $2.00 (floor).");
        assert!(!line.contains("$0.00"));
    }

    #[test]
    fn no_cost_talks_tokens_only() {
        let days = today_only(recorded(1_000, 500, 1.25, 1_500, 1_500, "claude-sonnet"));
        let line = line_for(&days, HashMap::new(), true, false, false, None).expect("conclusion");
        assert_eq!(line, "Today: 1,500 tokens.");
        assert!(!line.contains('$'));
        assert!(!line.contains("cost"));
    }

    fn prior_days() -> HashMap<String, DayStats> {
        HashMap::from([
            (
                "2026-09-02".to_string(),
                recorded(800, 400, 1.00, 1_200, 1_200, "claude-sonnet"),
            ),
            (
                "2026-09-01".to_string(),
                recorded(800, 400, 1.00, 1_200, 1_200, "claude-sonnet"),
            ),
        ])
    }

    #[test]
    fn insufficient_pace_data_omits_comparison() {
        let displayed = today_only(recorded(1_000, 500, 3.00, 1_500, 1_500, "claude-sonnet"));
        let line = line_for(&displayed, prior_days(), true, true, false, None).expect("conclusion");
        assert_eq!(line, "Today: 1,500 tokens, $3.00.");
        assert!(!line.contains("mean"));
        assert!(!line.contains("Faster"));
        assert!(!line.contains("Slower"));
    }

    #[test]
    fn unknown_cost_does_not_print_zero_dollars() {
        let stats = unpriced(100, 50);
        let day = DayStats {
            stats: stats.clone(),
            models: HashMap::from([("mystery-model".to_string(), stats)]),
        };
        let displayed = today_only(day);
        let line =
            line_for(&displayed, HashMap::new(), false, true, false, None).expect("conclusion");
        assert_eq!(line, "This period: 150 tokens, cost unknown.");
        assert!(!line.contains("$0.00"));
        assert!(!line.contains("Today"));
    }

    #[test]
    fn empty_window_has_no_conclusion() {
        let empty = HashMap::new();
        assert!(line_for(&empty, HashMap::new(), true, true, false, None).is_none());
    }

    #[test]
    fn compact_omits_pace_even_with_prior_days() {
        let mut extra = prior_days();
        extra.insert(
            "2026-08-31".to_string(),
            recorded(800, 400, 1.00, 1_200, 1_200, "claude-sonnet"),
        );
        let displayed = today_only(recorded(1_000, 500, 3.00, 1_500, 1_500, "claude-sonnet"));
        let line = line_for(&displayed, extra, true, true, true, None).expect("conclusion");
        assert_eq!(line, "Today: 1.5K tokens, $3.00.");
        assert!(!line.contains("mean"));
    }

    #[test]
    fn pace_uses_prior_complete_days_when_available() {
        let mut extra = prior_days();
        extra.insert(
            "2026-08-31".to_string(),
            recorded(800, 400, 1.00, 1_200, 1_200, "claude-sonnet"),
        );
        let displayed = today_only(recorded(1_000, 500, 3.00, 1_500, 1_500, "claude-sonnet"));
        let line = line_for(&displayed, extra, true, true, false, None).expect("conclusion");
        assert_eq!(
            line,
            "Today: 1,500 tokens, $3.00. Faster than the last 3-day mean ($1.00)."
        );
    }

    #[test]
    fn source_all_mentions_count_not_names() {
        let days = today_only(recorded(1_000, 500, 1.25, 1_500, 1_500, "claude-sonnet"));
        let line =
            line_for(&days, HashMap::new(), false, true, false, Some(29)).expect("conclusion");
        assert_eq!(line, "This period (29 sources): 1,500 tokens, $1.25.");
        assert!(!line.contains("claude"));
    }
}
