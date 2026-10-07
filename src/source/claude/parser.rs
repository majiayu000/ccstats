//! Claude usage mapped from the shared session event reader.
use crate::source::session_reader;
use crate::{
    consts::{DATE_FORMAT, UNKNOWN},
    core::{RawEntry, source_wide_message_id},
    source::ParseOutput,
    utils::Timezone,
};
use agent_sessions::{AccountingPolicy, Agent, Event, EventKinds, ReadOptions};
use std::cell::RefCell;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::rc::Rc;

// Stream native fields the shared reader does not yet project. Each line is
// read once, and the shared reader remains authoritative for usage/errors.
#[derive(Default)]
struct NativeContext {
    turn: Option<u64>,
    turn_start_ms: Option<i64>,
    compactions: Vec<i64>,
}

#[derive(serde::Deserialize)]
struct NativeHeader {
    #[serde(rename = "type")]
    kind: Option<String>,
    subtype: Option<String>,
    timestamp: Option<String>,
}

impl NativeContext {
    fn observe(&mut self, bytes: &[u8]) {
        let Ok(header) = serde_json::from_slice::<NativeHeader>(bytes) else {
            return;
        };
        if header.kind.as_deref() == Some("system")
            && header.subtype.as_deref() == Some("compact_boundary")
        {
            if let Some(at) = header
                .timestamp
                .as_deref()
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            {
                self.compactions.push(at.timestamp_millis());
            }
            return;
        }
        if header.kind.as_deref() != Some("user") {
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
                self.turn_start_ms = header
                    .timestamp
                    .as_deref()
                    .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                    .map(|at| at.timestamp_millis());
            }
        }
    }
}

struct NativeLines {
    source: BufReader<File>,
    line: Vec<u8>,
    position: usize,
    context: Rc<RefCell<NativeContext>>,
}

impl Read for NativeLines {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        let n = self.fill_buf()?.read(bytes)?;
        self.consume(n);
        Ok(n)
    }
}

impl BufRead for NativeLines {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        if self.position == self.line.len() {
            self.line.clear();
            self.position = 0;
            self.source.read_until(b'\n', &mut self.line)?;
            self.context.borrow_mut().observe(&self.line);
        }
        Ok(&self.line[self.position..])
    }

    fn consume(&mut self, n: usize) {
        self.position = (self.position + n).min(self.line.len());
    }
}

pub(super) fn find_claude_files() -> Vec<PathBuf> {
    session_reader::files(&session_reader::roots(Agent::ClaudeCode), Agent::ClaudeCode)
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
    let file = match File::open(path) {
        Ok(file) => file,
        Err(e) => {
            if debug {
                eprintln!("Cannot read {}: {e}", path.display());
            }
            out.errors = 1;
            return out;
        }
    };
    let mut reader = match agent_sessions::read_from(
        Agent::ClaudeCode,
        NativeLines {
            source: BufReader::new(file),
            line: Vec::new(),
            position: 0,
            context: Rc::clone(&context),
        },
        &ReadOptions {
            include: EventKinds::USAGE.union(EventKinds::META),
            accounting: AccountingPolicy::UsageStatistics,
            max_file_bytes: None,
            max_line_bytes: None,
            ..Default::default()
        },
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
        out.entries.push(RawEntry {
            agent_version: agent_version.clone(),
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
