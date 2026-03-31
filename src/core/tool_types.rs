//! Types for tool usage analysis

use serde::Serialize;

/// A single tool call extracted from JSONL
#[derive(Debug, Clone)]
pub(crate) struct ToolCall {
    pub(crate) name: String,
    pub(crate) date_str: String,
}

/// Aggregated statistics for a single tool
#[derive(Debug, Clone, Default, Serialize)]
pub(crate) struct ToolStats {
    pub(crate) name: String,
    pub(crate) calls: u64,
}

/// Result of tool aggregation
#[derive(Debug, Clone, Default, Serialize)]
pub(crate) struct ToolSummary {
    pub(crate) tools: Vec<ToolStats>,
    pub(crate) total: u64,
}
