//! Core module - shared types and logic for all data sources

mod aggregator;
mod dedup;
mod tool_aggregator;
mod tool_types;
mod types;

pub(crate) use aggregator::{
    aggregate_blocks, aggregate_by_endpoint, aggregate_daily, aggregate_projects,
    aggregate_sessions, aggregate_sessions_map, format_project_name, merge_day_stats,
};
pub(crate) use dedup::{DedupAccumulator, source_wide_message_id};
pub(crate) use tool_aggregator::aggregate_tools;
#[cfg(test)]
pub(crate) use tool_types::ToolStats;
pub(crate) use tool_types::{ToolCall, ToolCallIdentity, ToolSummary};
pub(crate) use types::{
    BlockStats, CostKind, CostTokens, DataQuality, DateFilter, DayStats, Endpoint, EndpointStats,
    LoadResult, ProjectStats, RawEntry, SessionStats, Stats,
};
