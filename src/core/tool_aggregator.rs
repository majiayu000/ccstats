//! Aggregation logic for tool call statistics

use std::collections::HashMap;

use super::tool_types::{ToolCall, ToolStats, ToolSummary};

/// Aggregate tool calls into a sorted summary
pub(crate) fn aggregate_tools(calls: Vec<ToolCall>) -> ToolSummary {
    let mut counts: HashMap<String, u64> = HashMap::with_capacity(calls.len());
    for call in &calls {
        *counts.entry(call.name.clone()).or_default() += 1;
    }

    let total = calls.len() as u64;
    let mut tools: Vec<ToolStats> = counts
        .into_iter()
        .map(|(name, calls)| ToolStats { name, calls })
        .collect();

    // Sort by call count descending
    tools.sort_by(|a, b| b.calls.cmp(&a.calls));

    ToolSummary { tools, total }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_call(name: &str) -> ToolCall {
        ToolCall {
            name: name.to_string(),
            date_str: "2025-01-01".to_string(),
        }
    }

    #[test]
    fn aggregate_empty() {
        let summary = aggregate_tools(vec![]);
        assert_eq!(summary.total, 0);
        assert!(summary.tools.is_empty());
    }

    #[test]
    fn aggregate_single_tool() {
        let calls = vec![make_call("Read"), make_call("Read"), make_call("Read")];
        let summary = aggregate_tools(calls);
        assert_eq!(summary.total, 3);
        assert_eq!(summary.tools.len(), 1);
        assert_eq!(summary.tools[0].name, "Read");
        assert_eq!(summary.tools[0].calls, 3);
    }

    #[test]
    fn aggregate_multiple_tools_sorted_by_count() {
        let calls = vec![
            make_call("Read"),
            make_call("Bash"),
            make_call("Read"),
            make_call("Edit"),
            make_call("Bash"),
            make_call("Read"),
        ];
        let summary = aggregate_tools(calls);
        assert_eq!(summary.total, 6);
        assert_eq!(summary.tools.len(), 3);
        assert_eq!(summary.tools[0].name, "Read");
        assert_eq!(summary.tools[0].calls, 3);
        assert_eq!(summary.tools[1].name, "Bash");
        assert_eq!(summary.tools[1].calls, 2);
        assert_eq!(summary.tools[2].name, "Edit");
        assert_eq!(summary.tools[2].calls, 1);
    }
}
