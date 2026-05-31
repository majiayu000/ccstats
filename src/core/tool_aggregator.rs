//! Aggregation logic for tool call statistics

use std::collections::{HashMap, HashSet};

use super::tool_types::{ToolCall, ToolCallIdentity, ToolStats, ToolSummary};

/// Aggregate tool calls into a sorted summary
pub(crate) fn aggregate_tools(calls: &[ToolCall]) -> ToolSummary {
    let mut counts: HashMap<String, u64> = HashMap::with_capacity(calls.len());
    let mut seen_identities: HashSet<ToolCallIdentity> = HashSet::new();
    let mut total = 0;

    for call in calls {
        if let Some(identity) = &call.identity
            && !seen_identities.insert(identity.clone())
        {
            continue;
        }
        *counts.entry(call.name.clone()).or_default() += 1;
        total += 1;
    }

    let mut tools: Vec<ToolStats> = counts
        .into_iter()
        .map(|(name, calls)| ToolStats { name, calls })
        .collect();

    // Sort by call count descending
    tools.sort_by_key(|tool| std::cmp::Reverse(tool.calls));

    ToolSummary { tools, total }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_call(name: &str) -> ToolCall {
        ToolCall {
            name: name.to_string(),
            date_str: "2025-01-01".to_string(),
            identity: None,
        }
    }

    fn make_identified_call(name: &str, message_id: &str, tool_use_id: &str) -> ToolCall {
        ToolCall {
            name: name.to_string(),
            date_str: "2025-01-01".to_string(),
            identity: Some(ToolCallIdentity::new("session-a", message_id, tool_use_id)),
        }
    }

    #[test]
    fn aggregate_empty() {
        let summary = aggregate_tools(&[]);
        assert_eq!(summary.total, 0);
        assert!(summary.tools.is_empty());
    }

    #[test]
    fn aggregate_single_tool() {
        let calls = vec![make_call("Read"), make_call("Read"), make_call("Read")];
        let summary = aggregate_tools(&calls);
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
        let summary = aggregate_tools(&calls);
        assert_eq!(summary.total, 6);
        assert_eq!(summary.tools.len(), 3);
        assert_eq!(summary.tools[0].name, "Read");
        assert_eq!(summary.tools[0].calls, 3);
        assert_eq!(summary.tools[1].name, "Bash");
        assert_eq!(summary.tools[1].calls, 2);
        assert_eq!(summary.tools[2].name, "Edit");
        assert_eq!(summary.tools[2].calls, 1);
    }

    #[test]
    fn aggregate_deduplicates_repeated_tool_identity() {
        let calls = vec![
            make_identified_call("Read", "msg_1", "toolu_1"),
            make_identified_call("Read", "msg_1", "toolu_1"),
            make_identified_call("Bash", "msg_1", "toolu_2"),
        ];
        let summary = aggregate_tools(&calls);
        assert_eq!(summary.total, 2);
        assert_eq!(summary.tools.len(), 2);
        assert!(
            summary
                .tools
                .iter()
                .any(|tool| tool.name == "Read" && tool.calls == 1)
        );
        assert!(
            summary
                .tools
                .iter()
                .any(|tool| tool.name == "Bash" && tool.calls == 1)
        );
    }
}
