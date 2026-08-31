//! Grok data source configuration
//!
//! Defines the `GrokSource` implementation of the Source trait.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::core::{CostKind, RawEntry};
use crate::source::{Capabilities, ParseOutput, Source};
use crate::utils::Timezone;

use super::unified::{find_grok_files, parse_grok_file_with_debug};

/// Grok data source.
pub(crate) struct GrokSource;

impl GrokSource {
    pub(crate) fn new() -> Self {
        Self
    }
}

impl Default for GrokSource {
    fn default() -> Self {
        Self::new()
    }
}

impl Source for GrokSource {
    fn name(&self) -> &'static str {
        "grok"
    }

    fn display_name(&self) -> &'static str {
        "Grok"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["gx"]
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            has_projects: true,
            has_billing_blocks: false,
            has_reasoning_tokens: true,
            has_cache_creation: true,
            has_cache_read: true,
            needs_dedup: true,
            has_tool_calls: false,
            has_endpoints: false,
        }
    }

    fn find_files(&self) -> Vec<PathBuf> {
        find_grok_files()
    }

    fn parse_file(&self, path: &Path, timezone: Timezone, debug: bool) -> ParseOutput {
        parse_grok_file_with_debug(path, timezone, debug)
    }

    fn finalize_entries(&self, entries: Vec<RawEntry>) -> Vec<RawEntry> {
        let priced_sessions: HashSet<_> = entries
            .iter()
            .filter(|entry| entry.api_equivalent_priced_tokens > 0)
            .map(|entry| entry.session_id.clone())
            .collect();
        entries
            .into_iter()
            .map(|mut entry| {
                if entry.cost_kind == CostKind::EstimatedProxy
                    && priced_sessions.contains(&entry.session_id)
                {
                    entry.cost_kind = CostKind::Real;
                    entry.recorded_cost_usd = Some(0.0);
                }
                entry
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{CostKind, Endpoint, RawEntry};

    fn entry(session_id: &str, cost_kind: CostKind, priced_tokens: i64) -> RawEntry {
        RawEntry {
            timestamp: "2026-08-21T05:42:00Z".to_string(),
            timestamp_ms: 0,
            date_str: "2026-08-21".to_string(),
            message_id: Some(format!("{session_id}:{priced_tokens}")),
            session_key: session_id.to_string(),
            session_id: session_id.to_string(),
            project_path: String::new(),
            model: "grok-4.6".to_string(),
            input_tokens: i64::from(cost_kind == CostKind::EstimatedProxy) * 100,
            output_tokens: 0,
            cache_creation: 0,
            cache_creation_1h: 0,
            cache_read: 0,
            reasoning_tokens: 0,
            stop_reason: Some("complete".to_string()),
            cost_kind,
            endpoint: Endpoint::Unknown,
            call_count: 1,
            recorded_cost_usd: (priced_tokens > 0).then_some(0.1),
            api_equivalent_priced_tokens: priced_tokens,
            api_equivalent_coverage_tokens: i64::from(cost_kind == CostKind::EstimatedProxy) * 100,
        }
    }

    #[test]
    fn overlap_suppression_is_session_local() {
        let source = GrokSource::new();
        let entries = vec![
            entry("priced", CostKind::EstimatedProxy, 0),
            entry("priced", CostKind::Real, 110),
            entry("snapshot-only", CostKind::EstimatedProxy, 0),
        ];

        let result = source.finalize_entries(entries);

        assert_eq!(result.len(), 3);
        assert!(
            result
                .iter()
                .any(|entry| entry.session_id == "snapshot-only")
        );
        assert!(!result.iter().any(|entry| {
            entry.session_id == "priced" && entry.cost_kind == CostKind::EstimatedProxy
        }));
    }
}
