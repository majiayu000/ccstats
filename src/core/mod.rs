//! Core module - shared types and logic for all data sources

pub mod aggregator;
pub mod dedup;
pub mod types;

pub use aggregator::{aggregate_blocks, aggregate_daily, aggregate_projects, aggregate_sessions, format_project_name};
pub use dedup::deduplicate;
pub use types::{BlockStats, DateFilter, DayStats, LoadResult, ProjectStats, RawEntry, SessionStats, Stats};
