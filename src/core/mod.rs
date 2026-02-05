//! Core module - shared types and logic for all data sources

pub(crate) mod aggregator;
pub(crate) mod dedup;
pub(crate) mod types;

pub(crate) use aggregator::{aggregate_blocks, aggregate_daily, aggregate_projects, aggregate_sessions, format_project_name};
pub(crate) use dedup::deduplicate;
pub(crate) use types::{BlockStats, DateFilter, DayStats, LoadResult, ProjectStats, RawEntry, SessionStats, Stats};
