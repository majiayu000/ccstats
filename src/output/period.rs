use chrono::{Datelike, NaiveDate};
use std::collections::HashMap;

use crate::core::DayStats;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Period {
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

pub(super) fn aggregate_day_stats_by_period(
    day_stats: &HashMap<String, DayStats>,
    period: Period,
) -> HashMap<String, DayStats> {
    if period == Period::Day {
        return day_stats.clone();
    }

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
