//! Core module - shared types and logic for all data sources

mod aggregator;
mod dedup;
mod types;

pub(crate) use aggregator::{aggregate_blocks, aggregate_daily, aggregate_projects, aggregate_sessions, format_project_name};
pub(crate) use dedup::deduplicate;
pub(crate) use types::{BlockStats, DateFilter, DayStats, LoadResult, ProjectStats, RawEntry, SessionStats, Stats};
