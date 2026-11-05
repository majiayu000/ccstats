//! Claude usage mapped from the shared session event reader.
use crate::source::session_reader;
use crate::{
    consts::{DATE_FORMAT, UNKNOWN},
    core::{RawEntry, source_wide_message_id},
    source::ParseOutput,
    utils::Timezone,
};
use agent_sessions::{Agent, Event, EventKinds, ReadOptions};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

// Stream native fields the shared reader does not yet project. Each line is
// read once, and the shared reader remains authoritative for usage/errors.
#[derive(Default)]
struct NativeContext {
    turn: Option<u64>,
    turn_start_ms: Option<i64>,
    compactions: Vec<i64>,
    speed: ResponseClock,
    /// Trigger time of the response on the most recently observed line.
    response_start_ms: Option<i64>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativeHeader {
    #[serde(rename = "type")]
    kind: Option<String>,
    subtype: Option<String>,
    timestamp: Option<String>,
    uuid: Option<String>,
    parent_uuid: Option<String>,
    is_sidechain: Option<bool>,
    agent_id: Option<String>,
    message: Option<HeaderMessage>,
}

#[derive(serde::Deserialize)]
struct HeaderMessage {
    id: Option<String>,
    model: Option<String>,
}

/// Minimal header for rows whose `message` is not an object.
#[derive(serde::Deserialize)]
struct BareHeader {
    #[serde(rename = "type")]
    kind: Option<String>,
    subtype: Option<String>,
    timestamp: Option<String>,
}

#[derive(Clone, Copy, PartialEq)]
enum NodeKind {
    User,
    Assistant,
    System,
    Other,
}

struct Node {
    kind: NodeKind,
    parent: Option<String>,
    at_ms: Option<i64>,
}

/// Response start times from the `parentUuid` chain of one transcript.
///
/// A response starts at the nearest `user` record (prompt or tool result) in
/// its parent chain, never before the previous response in the same thread
/// ended, so queued prompts do not absorb earlier turns. A chain that reaches
/// an assistant or system record first (API retry, resume) has no start.
#[derive(Default)]
struct ResponseClock {
    nodes: HashMap<String, Node>,
    starts: HashMap<String, Option<i64>>,
    lane_end_ms: HashMap<(bool, Option<String>), i64>,
}

impl ResponseClock {
    fn observe(&mut self, header: NativeHeader, at_ms: Option<i64>) -> Option<i64> {
        let kind = match header.kind.as_deref() {
            Some("user") => NodeKind::User,
            Some("assistant") => NodeKind::Assistant,
            Some("system") => NodeKind::System,
            _ => NodeKind::Other,
        };
        if let Some(uuid) = header.uuid {
            self.nodes.entry(uuid).or_insert(Node {
                kind,
                parent: header.parent_uuid.clone(),
                at_ms,
            });
        }
        let message = header.message.filter(|_| kind == NodeKind::Assistant)?;
        let id = message.id?;
        if message.model.as_deref() == Some("<synthetic>") {
            return None;
        }
        let lane = (header.is_sidechain == Some(true), header.agent_id);
        let previous_end = self.lane_end_ms.get(&lane).copied();
        if let Some(at) = at_ms {
            let end = self.lane_end_ms.entry(lane).or_insert(at);
            *end = (*end).max(at);
        }
        if let Some(start) = self.starts.get(&id) {
            return *start;
        }
        let start = self
            .trigger_ms(header.parent_uuid.as_deref())
            .map(|trigger| trigger.max(previous_end.unwrap_or(i64::MIN)));
        self.starts.insert(id, start);
        start
    }

    fn trigger_ms<'a>(&'a self, mut parent: Option<&'a str>) -> Option<i64> {
        // Bounded by the node count so a malformed parent cycle cannot loop.
        for _ in 0..=self.nodes.len() {
            let node = self.nodes.get(parent?)?;
            match node.kind {
                NodeKind::User => return node.at_ms,
                NodeKind::Assistant | NodeKind::System => return None,
                NodeKind::Other => parent = node.parent.as_deref(),
            }
        }
        None
    }
}

fn millis(timestamp: Option<&str>) -> Option<i64> {
    timestamp
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|at| at.timestamp_millis())
}

impl session_reader::LineObserver for NativeContext {
    fn observe(&mut self, bytes: &[u8]) {
        self.response_start_ms = None;
        let (kind, subtype, timestamp) = match serde_json::from_slice::<NativeHeader>(bytes) {
            Ok(header) => {
                let at_ms = millis(header.timestamp.as_deref());
                let fields = (
                    header.kind.clone(),
                    header.subtype.clone(),
                    header.timestamp.clone(),
                );
                self.response_start_ms = self.speed.observe(header, at_ms);
                fields
            }
            Err(_) => match serde_json::from_slice::<BareHeader>(bytes) {
                Ok(header) => (header.kind, header.subtype, header.timestamp),
                Err(_) => return,
            },
        };
        if kind.as_deref() == Some("system") && subtype.as_deref() == Some("compact_boundary") {
            if let Some(at) = millis(timestamp.as_deref()) {
                self.compactions.push(at);
            }
            return;
        }
        if kind.as_deref() != Some("user") {
            return;
        }
        let Ok(v) = serde_json::from_slice::<serde_json::Value>(bytes) else {
            return;
        };
        if v["type"] == "user"
            && v["isMeta"] != true
            && v["is_meta"] != true
            && v["isCompactSummary"] != true
            && v.pointer("/message/isMeta") != Some(&serde_json::Value::Bool(true))
            && v.pointer("/message/is_meta") != Some(&serde_json::Value::Bool(true))
        {
            let content = &v["message"]["content"];
            let has_prompt = content.is_string()
                || content.as_array().is_some_and(|blocks| {
                    blocks.iter().any(|block| {
                        matches!(block["type"].as_str(), Some("text" | "image" | "document"))
                    })
                });
            if has_prompt {
                self.turn = Some(self.turn.unwrap_or(0) + 1);
                self.turn_start_ms = millis(timestamp.as_deref());
            }
        }
    }
}

pub(super) fn find_claude_files() -> Vec<PathBuf> {
    session_reader::files(&session_reader::roots(Agent::ClaudeCode), Agent::ClaudeCode).0
}

pub(super) fn parse_claude_file_with_debug(
    path: &Path,
    timezone: Timezone,
    debug: bool,
) -> ParseOutput {
    parse_file(path, timezone, debug, false)
}

pub(super) fn parse_claude_file_with_diagnostics(
    path: &Path,
    timezone: Timezone,
    debug: bool,
) -> ParseOutput {
    parse_file(path, timezone, debug, true)
}

#[allow(clippy::too_many_lines)]
fn parse_file(
    path: &Path,
    timezone: Timezone,
    debug: bool,
    accounting_diagnostics: bool,
) -> ParseOutput {
    let mut out = ParseOutput {
        entries: Vec::new(),
        errors: 0,
    };
    let context = Rc::new(RefCell::new(NativeContext::default()));
    let mut reader = match session_reader::observed_reader(
        path,
        Agent::ClaudeCode,
        EventKinds::USAGE.union(EventKinds::META),
        ReadOptions::default().codex_usage,
        Rc::clone(&context),
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
    let is_subagent = path.components().any(|c| c.as_os_str() == "subagents");
    let parent_session_id = is_subagent
        .then(|| {
            let parent = path.parent()?.parent()?;
            // Older flat project/subagents layouts do not encode a parent session.
            if parent.parent()?.file_name()? == "projects" {
                return None;
            }
            parent.file_name()?.to_str().map(str::to_owned)
        })
        .flatten();
    let mut agent_version = None;
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
        let usage = match event.value {
            Event::Meta(meta) => {
                // Version continuity comes from the shared metadata API.
                if meta.agent_version.is_some() {
                    agent_version = meta.agent_version;
                }
                continue;
            }
            Event::Usage(usage) => usage,
            _ => continue,
        };
        let (Some(timestamp), Some(at)) = (event.timestamp_text, event.at) else {
            if accounting_diagnostics {
                out.errors += 1;
            }
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
        let mut native = context.borrow_mut();
        if native.turn.is_some() {
            // Retain the whole-file prefix before the loader applies date cuts.
            native.turn_start_ms =
                Some(native.turn_start_ms.map_or(at.timestamp_millis(), |start| {
                    start.min(at.timestamp_millis())
                }));
        }
        // Without a stop reason the usage is a streaming snapshot (subagent
        // transcripts often keep only that), not the response's final count.
        let response = Some(crate::core::ResponseTiming {
            output_tokens: output.saturating_add(reasoning),
            duration_ms: native
                .response_start_ms
                .filter(|_| usage.stop_reason.is_some())
                .map(|start| at.timestamp_millis() - start),
        });
        out.entries.push(RawEntry {
            agent_version: agent_version.clone(),
            response,
            claude_diagnostics: Some(crate::core::ClaudeDiagnostics {
                turn: native.turn,
                turn_start_ms: native.turn_start_ms,
                is_subagent,
                parent_session_id: parent_session_id.clone(),
                compactions: native.compactions.clone(),
                cache_write_reported: usage.counts.cache_write.is_some(),
                model_id: usage.model.clone().unwrap_or_default(),
            }),
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
