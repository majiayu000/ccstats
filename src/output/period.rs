use chrono::{Datelike, NaiveDate};
use std::collections::HashMap;

use crate::core::DayStats;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Period {
    Day,
    Week,
    Month,
}

fn week_start(date_str: &str) -> String {
    if let Ok(date) = NaiveDate::parse_from_str(date_str, "%Y-%m-%d") {
        let weekday = date.weekday().num_days_from_monday();
        let monday = date - chrono::Duration::days(weekday as i64);
        monday.format("%Y-%m-%d").to_string()
    } else {
        date_str.to_string()
    }
}

fn period_key(date: &str, period: Period) -> String {
    match period {
        Period::Day => date.to_string(),
        Period::Week => week_start(date),
        Period::Month => date.get(0..7).unwrap_or(date).to_string(),
    }
}

pub(crate) fn aggregate_day_stats_by_period(
    day_stats: &HashMap<String, DayStats>,
    period: Period,
) -> HashMap<String, DayStats> {
    debug_assert_ne!(period, Period::Day, "Day period should not be aggregated");

    let mut aggregated: HashMap<String, DayStats> = HashMap::new();

    for (date, stats) in day_stats {
        let key = period_key(date, period);
        let entry = aggregated.entry(key).or_default();

        for (model, model_stats) in &stats.models {
            entry.stats.add(model_stats);
            entry
                .models
                .entry(model.clone())
                .or_default()
                .add(model_stats);
        }
    }

    aggregated
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::Stats;

    fn make_day_stats(models: &[(&str, i64)]) -> DayStats {
        let mut ds = DayStats::default();
        for &(model, tokens) in models {
            let stats = Stats {
                input_tokens: tokens,
                output_tokens: tokens / 2,
                count: 1,
                ..Default::default()
            };
            ds.add_stats(model.to_string(), &stats);
        }
        ds
    }

    #[test]
    fn week_start_returns_monday() {
        // 2025-01-08 is a Wednesday
        assert_eq!(week_start("2025-01-08"), "2025-01-06");
        // 2025-01-06 is already Monday
        assert_eq!(week_start("2025-01-06"), "2025-01-06");
        // 2025-01-12 is Sunday
        assert_eq!(week_start("2025-01-12"), "2025-01-06");
    }

    #[test]
    fn week_start_cross_year_boundary() {
        // 2025-01-01 is Wednesday, Monday is 2024-12-30
        assert_eq!(week_start("2025-01-01"), "2024-12-30");
    }

    #[test]
    fn week_start_invalid_date_returns_input() {
        assert_eq!(week_start("not-a-date"), "not-a-date");
    }

    #[test]
    fn period_key_day_returns_as_is() {
        assert_eq!(period_key("2025-01-15", Period::Day), "2025-01-15");
    }

    #[test]
    fn period_key_week_returns_monday() {
        assert_eq!(period_key("2025-01-08", Period::Week), "2025-01-06");
    }

    #[test]
    fn period_key_month_returns_yyyy_mm() {
        assert_eq!(period_key("2025-01-15", Period::Month), "2025-01");
    }

    #[test]
    fn aggregate_by_week_merges_same_week() {
        let mut day_stats = HashMap::new();
        // Mon and Wed of same week
        day_stats.insert("2025-01-06".to_string(), make_day_stats(&[("sonnet", 100)]));
        day_stats.insert("2025-01-08".to_string(), make_day_stats(&[("sonnet", 200)]));

        let result = aggregate_day_stats_by_period(&day_stats, Period::Week);
        assert_eq!(result.len(), 1);
        let week = &result["2025-01-06"];
        assert_eq!(week.stats.input_tokens, 300);
    }

    #[test]
    fn aggregate_by_month_merges_same_month() {
        let mut day_stats = HashMap::new();
        day_stats.insert("2025-03-01".to_string(), make_day_stats(&[("opus", 500)]));
        day_stats.insert("2025-03-15".to_string(), make_day_stats(&[("opus", 300)]));
        day_stats.insert("2025-04-01".to_string(), make_day_stats(&[("opus", 100)]));

        let result = aggregate_day_stats_by_period(&day_stats, Period::Month);
        assert_eq!(result.len(), 2);
        assert_eq!(result["2025-03"].stats.input_tokens, 800);
        assert_eq!(result["2025-04"].stats.input_tokens, 100);
    }

    #[test]
    fn aggregate_merges_model_breakdown() {
        let mut day_stats = HashMap::new();
        day_stats.insert("2025-01-06".to_string(), make_day_stats(&[("sonnet", 100)]));
        day_stats.insert("2025-01-08".to_string(), make_day_stats(&[("sonnet", 50), ("opus", 200)]));

        let result = aggregate_day_stats_by_period(&day_stats, Period::Week);
        let week = &result["2025-01-06"];
        assert_eq!(week.models.len(), 2);
        assert_eq!(week.models["sonnet"].input_tokens, 150);
        assert_eq!(week.models["opus"].input_tokens, 200);
    }

    #[test]
    fn aggregate_empty_input() {
        let day_stats = HashMap::new();
        let result = aggregate_day_stats_by_period(&day_stats, Period::Week);
        assert!(result.is_empty());
    }
}
