//! Codex native events projected into ccstats accounting and scope policy.
use super::config::CodexScope;
use crate::source::session_reader;
use crate::{
    consts::{DATE_FORMAT, UNKNOWN},
    core::RawEntry,
    source::ParseOutput,
    utils::Timezone,
};
use agent_sessions::{Agent, CodexUsageMode, Event, EventKinds, Origin};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

pub(crate) fn codex_root_candidate() -> Option<PathBuf> {
    session_reader::roots(Agent::Codex).codex
}
pub(super) fn codex_sessions_dir_candidate() -> Option<PathBuf> {
    codex_root_candidate().map(|p| p.join("sessions"))
}
#[cfg(test)]
fn find_codex_files_in_root(root: &Path) -> Vec<PathBuf> {
    session_reader::files(
        &agent_sessions::Roots {
            claude: None,
            codex: Some(root.into()),
        },
        Agent::Codex,
    )
    .0
}
pub(super) fn parse_codex_file_with_scope(
    path: &Path,
    timezone: Timezone,
    debug: bool,
    scope: CodexScope,
) -> ParseOutput {
    parse_file(path, timezone, debug, scope, false)
}
pub(super) fn parse_codex_file_for_quota(path: &Path, timezone: Timezone) -> ParseOutput {
    parse_file(path, timezone, false, CodexScope::All, false)
}
pub(super) fn parse_codex_file_with_diagnostics(
    path: &Path,
    timezone: Timezone,
    debug: bool,
    scope: CodexScope,
) -> ParseOutput {
    parse_file(path, timezone, debug, scope, true)
}
fn parse_file(
    path: &Path,
    timezone: Timezone,
    debug: bool,
    scope: CodexScope,
    accounting_diagnostics: bool,
) -> ParseOutput {
    let (output, has_responses) = parse_mode(
        path,
        timezone,
        debug,
        scope,
        CodexUsageMode::TokenCount,
        accounting_diagnostics,
    );
    if output.errors == 0 && output.entries.is_empty() && has_responses {
        parse_mode(
            path,
            timezone,
            debug,
            scope,
            CodexUsageMode::Response,
            accounting_diagnostics,
        )
        .0
    } else {
        output
    }
}
fn included(scope: CodexScope, origin: Origin) -> bool {
    match scope {
        CodexScope::All => true,
        CodexScope::Interactive => matches!(origin, Origin::Interactive | Origin::Ide),
        CodexScope::Exec => origin == Origin::Exec,
        CodexScope::Subagent => origin == Origin::Subagent,
    }
}
struct Projection {
    session_key: String,
    logical_key: String,
    session_id: String,
    project_path: String,
    origin: Origin,
    timezone: Timezone,
    previous_cumulative: Option<[i64; 7]>,
    cumulative_epoch: u64,
}
impl Projection {
    fn new(path: &Path, timezone: Timezone) -> Self {
        let key = path.display().to_string();
        Self {
            session_key: key.clone(),
            logical_key: key,
            session_id: path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(UNKNOWN)
                .into(),
            project_path: String::new(),
            origin: Origin::Unknown,
            timezone,
            previous_cumulative: None,
            cumulative_epoch: 0,
        }
    }
    fn metadata(&mut self, m: &agent_sessions::MetaUpdate) {
        if let Some(id) = m.session_id.as_ref().filter(|s| !s.trim().is_empty()) {
            self.logical_key = format!("codex-session:{id}");
            self.session_id.clone_from(id);
        }
        if let Some(cwd) = m.cwd.as_ref().filter(|s| !s.is_empty()) {
            self.project_path.clone_from(cwd);
        }
        self.origin = m.origin.unwrap_or_default();
    }
    fn entry(&mut self, event: agent_sessions::Located<Event>) -> Result<Option<RawEntry>, ()> {
        let Event::Usage(u) = event.value else {
            return Ok(None);
        };
        // Observe even zero-delta reset records before filtering them out.
        // The reset sequence is stable across copies of the same log, unlike
        // file paths or record indexes, and distinguishes reused total/delta IDs.
        let cumulative = u
            .cumulative
            .map(|total| session_reader::buckets(total).ok_or(()))
            .transpose()?;
        if let Some(total) = cumulative {
            if self
                .previous_cumulative
                .is_some_and(|previous| total.iter().zip(previous).any(|(next, prev)| *next < prev))
            {
                self.cumulative_epoch += 1;
            }
            self.previous_cumulative = Some(total);
        }
        let (Some(timestamp), Some(at)) = (event.timestamp_text, event.at) else {
            return Err(());
        };
        let delta = session_reader::buckets(u.counts).ok_or(())?;
        let [input, read, write, output, reasoning, _, _] = delta;
        if [input, read, write, output, reasoning]
            .iter()
            .all(|n| *n == 0)
        {
            return Ok(None);
        }
        if read.checked_add(write).is_none_or(|n| n > input) {
            return Err(());
        }
        let model = u.model.unwrap_or_else(|| "unknown-model".into());
        let message_id = if let Some(total) = cumulative {
            usage_id(
                &model,
                &self.logical_key,
                total,
                delta,
                self.cumulative_epoch,
            )
        } else if let Some(id) = u.dedup_key {
            crate::core::source_wide_message_id(
                "codex-response",
                &format!("{}:{id}", self.logical_key),
            )
        } else {
            format!(
                "codex-response:{}:{}",
                self.logical_key, event.location.record_index
            )
        };
        Ok(Some(RawEntry {
            agent_version: None,
            claude_diagnostics: None,
            response: None,
            timestamp,
            timestamp_ms: at.timestamp_millis(),
            date_str: self
                .timezone
                .to_fixed_offset(at)
                .date_naive()
                .format(DATE_FORMAT)
                .to_string(),
            message_id: Some(message_id),
            session_key: self.session_key.clone(),
            session_id: self.session_id.clone(),
            project_path: self.project_path.clone(),
            model,
            input_tokens: input - read - write,
            output_tokens: output.saturating_sub(reasoning).max(0),
            cache_creation: write,
            cache_creation_1h: 0,
            cache_read: read,
            reasoning_tokens: reasoning,
            stop_reason: Some("complete".into()),
            cost_kind: crate::core::CostKind::Real,
            endpoint: crate::core::Endpoint::Unknown,
            call_count: 1,
            reported_total_tokens: None,
            recorded_cost_usd: None,
            api_equivalent_priced_tokens: 0,
            api_equivalent_coverage_tokens: 0,
        }))
    }
}
/// Record kind and timestamp, read without scanning the rest of the line.
///
/// Rollout lines start with `timestamp`, `type`, then `payload`, whose `type`
/// and `role` precede its content. Tool outputs can be megabytes, so the
/// visitors stop once these fields are known and the trailing-data error that
/// follows is ignored.
#[derive(Default)]
struct NativeHead {
    timestamp: Option<String>,
    kind: Option<String>,
    payload_kind: Option<String>,
    role: Option<String>,
}

impl NativeHead {
    fn read(line: &[u8]) -> Self {
        let mut head = Self::default();
        let mut reader = serde_json::Deserializer::from_slice(line);
        let _stopped_early = serde::Deserializer::deserialize_map(&mut reader, Record(&mut head));
        head
    }
}

struct Record<'a>(&'a mut NativeHead);
struct Payload<'a>(&'a mut NativeHead);

impl<'de> serde::de::Visitor<'de> for Record<'_> {
    type Value = ();

    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("a rollout record")
    }

    fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        while let Some(key) = map.next_key::<&str>()? {
            match key {
                "timestamp" => self.0.timestamp = Some(map.next_value()?),
                "type" => self.0.kind = Some(map.next_value()?),
                "payload" => return map.next_value_seed(Payload(self.0)),
                _ => {
                    map.next_value::<serde::de::IgnoredAny>()?;
                }
            }
        }
        Ok(())
    }
}

impl<'de> serde::de::DeserializeSeed<'de> for Payload<'_> {
    type Value = ();

    fn deserialize<D: serde::Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_map(self)
    }
}

impl<'de> serde::de::Visitor<'de> for Payload<'_> {
    type Value = ();

    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("a rollout payload")
    }

    fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        while let Some(key) = map.next_key::<&str>()? {
            match key {
                "type" => self.0.payload_kind = Some(map.next_value()?),
                "role" => self.0.role = Some(map.next_value()?),
                _ => {
                    map.next_value::<serde::de::IgnoredAny>()?;
                }
            }
            // Only messages need a role.
            if self
                .0
                .payload_kind
                .as_deref()
                .is_some_and(|kind| kind != "message" || self.0.role.is_some())
            {
                break;
            }
        }
        Ok(())
    }
}

struct Pending {
    start_ms: Option<i64>,
    end_ms: i64,
}

/// Response timing from native rollout order.
///
/// A response is the run of generated items (assistant messages, reasoning,
/// tool calls) before its usage record. It starts at the latest trigger before
/// its first item (turn start, user or developer message, tool output,
/// inter-agent message), never before the previous response ended, and ends at
/// its last generated item or `token_usage_record`, whichever is later. Usage
/// without generated items, or a response without a trigger, has no duration.
#[derive(Default)]
struct ResponseClock {
    trigger_ms: Option<i64>,
    pending: Option<Pending>,
    previous_end_ms: Option<i64>,
}

impl ResponseClock {
    fn output(&mut self, at: i64) {
        if self.trigger_ms.is_none()
            && let Some(pending) = &mut self.pending
        {
            pending.end_ms = pending.end_ms.max(at);
            return;
        }
        // A trigger after generated items starts another request; a response
        // that never reported usage is dropped here.
        if let Some(abandoned) = self.pending.take() {
            self.previous_end_ms = Some(abandoned.end_ms);
        }
        let start_ms = self
            .trigger_ms
            .take()
            .map(|trigger| trigger.max(self.previous_end_ms.unwrap_or(i64::MIN)));
        self.pending = Some(Pending {
            start_ms,
            end_ms: at,
        });
    }

    /// Duration of the response that the usage on the current line reports.
    fn finish(&mut self) -> Option<i64> {
        let pending = self.pending.take()?;
        self.previous_end_ms = Some(pending.end_ms);
        pending.start_ms.map(|start| pending.end_ms - start)
    }
}

impl session_reader::LineObserver for ResponseClock {
    fn observe(&mut self, line: &[u8]) {
        let head = NativeHead::read(line);
        let Some(at) = head
            .timestamp
            .as_deref()
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|at| at.timestamp_millis())
        else {
            return;
        };
        let payload_kind = head.payload_kind.as_deref().unwrap_or_default();
        match head.kind.as_deref() {
            Some("token_usage_record") => {
                if let Some(pending) = &mut self.pending {
                    pending.end_ms = pending.end_ms.max(at);
                }
            }
            Some("event_msg") if matches!(payload_kind, "task_started" | "user_message") => {
                self.trigger_ms = Some(at);
            }
            Some("response_item") => match payload_kind {
                "message" if head.role.as_deref() == Some("assistant") => self.output(at),
                "message" | "agent_message" => self.trigger_ms = Some(at),
                kind if kind.ends_with("_output") => self.trigger_ms = Some(at),
                kind if kind == "reasoning" || kind.ends_with("_call") => self.output(at),
                _ => {}
            },
            _ => {}
        }
    }
}

fn parse_mode(
    path: &Path,
    timezone: Timezone,
    debug: bool,
    scope: CodexScope,
    mode: CodexUsageMode,
    accounting_diagnostics: bool,
) -> (ParseOutput, bool) {
    let mut out = ParseOutput {
        entries: Vec::new(),
        errors: 0,
    };
    let clock = Rc::new(RefCell::new(ResponseClock::default()));
    let mut reader = match session_reader::observed_reader(
        path,
        Agent::Codex,
        EventKinds::USAGE.union(EventKinds::META),
        mode,
        Rc::clone(&clock),
    ) {
        Ok(r) => r,
        Err(e) => {
            out.errors = 1;
            if debug {
                eprintln!("Cannot read {}: {e}", path.display());
            }
            return (out, false);
        }
    };
    let mut projection = Projection::new(path, timezone);
    for result in reader.by_ref() {
        let event = match result {
            Ok(e) => e,
            Err(e) => {
                out.errors += 1;
                if debug {
                    eprintln!("{}: {e}", path.display());
                }
                if matches!(
                    e,
                    agent_sessions::StreamError::Line {
                        kind: agent_sessions::LineErrorKind::InvalidUtf8,
                        ..
                    }
                ) {
                    break;
                }
                continue;
            }
        };
        if let Event::Meta(m) = &event.value
            && event.record_type.as_deref() == Some("session_meta")
        {
            projection.metadata(m);
        } else if included(scope, projection.origin) {
            match projection.entry(event) {
                Ok(Some(mut entry)) => {
                    entry.response = Some(crate::core::ResponseTiming {
                        output_tokens: entry.output_tokens.saturating_add(entry.reasoning_tokens),
                        duration_ms: clock.borrow_mut().finish(),
                    });
                    out.entries.push(entry);
                }
                Ok(None) => {}
                Err(()) => out.errors += 1,
            }
        }
    }
    let summary = reader.finish();
    if accounting_diagnostics {
        out.errors += summary
            .ignored_types
            .get(agent_sessions::CODEX_MISSING_TOTAL_USAGE)
            .copied()
            .unwrap_or(0) as usize;
    }
    (
        out,
        summary
            .ignored_types
            .get("token_usage_record")
            .is_some_and(|n| *n > 0),
    )
}
fn usage_id(model: &str, session: &str, total: [i64; 7], delta: [i64; 7], epoch: u64) -> String {
    let prefix = if epoch == 0 && total[..6] == delta[..6] {
        "codex-token-count"
    } else {
        "source-wide:codex-token-count"
    };
    format!(
        "{prefix}:{session}:{model}:epoch={epoch}:total={},{},{},{},{},{}:delta={},{},{},{},{},{}",
        total[0],
        total[1],
        total[2],
        total[3],
        total[4],
        total[5],
        delta[0],
        delta[1],
        delta[2],
        delta[3],
        delta[4],
        delta[5]
    )
}

#[cfg(test)]
#[path = "parser_tests.rs"]
mod parser_tests;
