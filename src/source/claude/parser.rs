//! Claude usage mapped from the shared session event reader.
use crate::source::session_reader;
use crate::{
    consts::{DATE_FORMAT, UNKNOWN},
    core::{RawEntry, source_wide_message_id},
    source::ParseOutput,
    utils::Timezone,
};
use agent_sessions::{Agent, CodexUsageMode, Event, EventKinds};
use std::path::{Path, PathBuf};

pub(super) fn find_claude_files() -> Vec<PathBuf> {
    session_reader::files(&session_reader::roots(Agent::ClaudeCode), Agent::ClaudeCode)
}

pub(super) fn parse_claude_file_with_debug(
    path: &Path,
    timezone: Timezone,
    debug: bool,
) -> ParseOutput {
    let mut out = ParseOutput {
        entries: Vec::new(),
        errors: 0,
    };
    let mut reader = match session_reader::reader(
        path,
        Agent::ClaudeCode,
        EventKinds::USAGE,
        CodexUsageMode::TokenCount,
    ) {
        Ok(r) => r,
        Err(e) => {
            if debug {
                eprintln!("Cannot read {}: {e}", path.display());
            }
            out.errors = 1;
            return out;
        }
    };
    let session_key = path.display().to_string();
    let session_id = path.file_stem().and_then(|s| s.to_str()).unwrap_or(UNKNOWN);
    let project_path = derive_project_path(path);
    for result in reader.by_ref() {
        let event = match result {
            Ok(e) => e,
            Err(e) => {
                out.errors += 1;
                if debug {
                    eprintln!("{}: {e}", path.display());
                }
                continue;
            }
        };
        let Event::Usage(usage) = event.value else {
            continue;
        };
        let Some(timestamp) = event.timestamp_text else {
            continue;
        };
        let Some(at) = event.at else {
            continue;
        };
        let model = usage
            .model
            .as_deref()
            .map_or_else(|| UNKNOWN.into(), normalize_model_name);
        if model.is_empty() || model == "<synthetic>" {
            continue;
        }
        let Some(
            [
                input,
                cache_read,
                cache_write,
                output,
                reasoning,
                _,
                cache_write_1h,
            ],
        ) = session_reader::buckets(usage.counts)
        else {
            out.errors += 1;
            continue;
        };
        let endpoint = endpoint(usage.endpoint);
        out.entries.push(RawEntry {
            timestamp,
            timestamp_ms: at.timestamp_millis(),
            date_str: timezone
                .to_fixed_offset(at)
                .date_naive()
                .format(DATE_FORMAT)
                .to_string(),
            message_id: event
                .message_id
                .map(|id| source_wide_message_id("claude", &id)),
            session_key: session_key.clone(),
            session_id: session_id.into(),
            project_path: project_path.clone(),
            model,
            input_tokens: input,
            output_tokens: output,
            cache_creation: cache_write,
            cache_creation_1h: cache_write_1h,
            cache_read,
            reasoning_tokens: reasoning,
            stop_reason: usage.stop_reason,
            cost_kind: crate::core::CostKind::Real,
            endpoint,
            call_count: 1,
            reported_total_tokens: None,
            recorded_cost_usd: None,
            api_equivalent_priced_tokens: 0,
            api_equivalent_coverage_tokens: 0,
        });
    }
    out
}

fn endpoint(value: agent_sessions::Endpoint) -> crate::core::Endpoint {
    match value {
        agent_sessions::Endpoint::Native => crate::core::Endpoint::Native,
        agent_sessions::Endpoint::Proxy => crate::core::Endpoint::Proxy,
        _ => crate::core::Endpoint::Unknown,
    }
}

fn normalize_model_name(model: &str) -> String {
    let mut name = model;
    if let Some(stripped) = name.strip_prefix("anthropic.") {
        name = stripped;
    }
    if let Some(stripped) = name.strip_prefix("claude-") {
        name = stripped;
    }

    // Remove date suffix like -20251101
    if let Some(pos) = name.rfind('-') {
        let suffix = &name[pos + 1..];
        if suffix.len() == 8 && suffix.chars().all(|c| c.is_ascii_digit()) {
            return name[..pos].to_string();
        }
    }

    name.to_string()
}

fn derive_project_path(path: &Path) -> String {
    let mut components = path.parent().into_iter().flat_map(Path::components);
    while let Some(component) = components.next() {
        if component.as_os_str() == "projects" {
            if let Some(project) = components.next() {
                return project.as_os_str().to_string_lossy().into_owned();
            }
            break;
        }
    }

    path.parent()
        .and_then(|p| p.file_name())
        .and_then(|s| s.to_str())
        .unwrap_or(UNKNOWN)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_normalize_strips_anthropic_and_claude_prefix_and_date() {
        assert_eq!(
            normalize_model_name("anthropic.claude-3-5-sonnet-20241022"),
            "3-5-sonnet"
        );
    }

    #[test]
    fn test_normalize_strips_claude_prefix_and_date() {
        assert_eq!(normalize_model_name("claude-3-opus-20240229"), "3-opus");
    }

    #[test]
    fn test_normalize_no_prefix_no_date() {
        assert_eq!(normalize_model_name("gpt-4"), "gpt-4");
    }

    #[test]
    fn test_normalize_only_anthropic_prefix() {
        assert_eq!(normalize_model_name("anthropic.some-model"), "some-model");
    }

    #[test]
    fn test_normalize_short_suffix_not_stripped() {
        // Suffix "123" is only 3 chars, not 8 — should NOT be stripped
        assert_eq!(normalize_model_name("claude-model-123"), "model-123");
    }

    #[test]
    fn test_normalize_non_digit_suffix_not_stripped() {
        assert_eq!(
            normalize_model_name("claude-model-2024abcd"),
            "model-2024abcd"
        );
    }

    #[test]
    fn test_normalize_no_dash() {
        assert_eq!(normalize_model_name("singleword"), "singleword");
    }

    #[test]
    fn test_normalize_anthropic_claude_no_date() {
        assert_eq!(
            normalize_model_name("anthropic.claude-4-sonnet"),
            "4-sonnet"
        );
    }

    #[test]
    fn test_derive_project_path_uses_project_segment_for_subagent_logs() {
        let path = Path::new("/tmp/.claude/projects/myproject/subagents/agent-a.jsonl");
        assert_eq!(derive_project_path(path), "myproject");
    }

    #[test]
    fn test_derive_project_path_falls_back_to_immediate_parent() {
        let path = Path::new("/tmp/custom/session-a.jsonl");
        assert_eq!(derive_project_path(path), "custom");
    }

    // ========================================================================
    // parse_entry
    // ========================================================================
}

#[cfg(test)]
#[path = "parser_tests.rs"]
mod native_tests;
