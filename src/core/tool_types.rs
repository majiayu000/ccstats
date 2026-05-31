//! Types for tool usage analysis

use serde::Serialize;

/// Stable identity for a tool call in Claude JSONL logs.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct ToolCallIdentity {
    pub(crate) session_key: String,
    pub(crate) message_id: String,
    pub(crate) tool_use_id: String,
}

impl ToolCallIdentity {
    pub(crate) fn new(session_key: &str, message_id: &str, tool_use_id: &str) -> Self {
        Self {
            session_key: session_key.to_string(),
            message_id: message_id.to_string(),
            tool_use_id: tool_use_id.to_string(),
        }
    }
}

/// A single tool call extracted from JSONL
#[derive(Debug, Clone)]
pub(crate) struct ToolCall {
    pub(crate) name: String,
    pub(crate) date_str: String,
    pub(crate) identity: Option<ToolCallIdentity>,
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
