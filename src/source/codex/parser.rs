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
use std::path::{Path, PathBuf};

pub(crate) fn codex_root_candidate() -> Option<PathBuf> {
    session_reader::roots(Agent::Codex).codex
}
pub(super) fn codex_sessions_dir_candidate() -> Option<PathBuf> {
    codex_root_candidate().map(|p| p.join("sessions"))
}
fn find_codex_files_in_root(root: &Path) -> Vec<PathBuf> {
    session_reader::files(
        &agent_sessions::Roots {
            claude: None,
            codex: Some(root.into()),
        },
        Agent::Codex,
    )
}
pub(super) fn find_codex_files() -> Vec<PathBuf> {
    codex_root_candidate().map_or_else(Vec::new, |p| find_codex_files_in_root(&p))
}
pub(super) fn parse_codex_file_with_scope(
    path: &Path,
    timezone: Timezone,
    debug: bool,
    scope: CodexScope,
) -> ParseOutput {
    parse_file(path, timezone, debug, scope, "gpt-5")
}
pub(super) fn parse_codex_file_for_quota(path: &Path, timezone: Timezone) -> ParseOutput {
    parse_file(path, timezone, false, CodexScope::All, "unknown-model")
}
fn parse_file(
    path: &Path,
    timezone: Timezone,
    debug: bool,
    scope: CodexScope,
    missing_model: &str,
) -> ParseOutput {
    let (output, has_responses) = parse_mode(
        path,
        timezone,
        debug,
        scope,
        missing_model,
        CodexUsageMode::TokenCount,
    );
    if output.errors == 0 && output.entries.is_empty() && has_responses {
        parse_mode(
            path,
            timezone,
            debug,
            scope,
            missing_model,
            CodexUsageMode::Response,
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
struct Projection<'a> {
    session_key: String,
    logical_key: String,
    session_id: String,
    project_path: String,
    origin: Origin,
    timezone: Timezone,
    missing_model: &'a str,
}
impl<'a> Projection<'a> {
    fn new(path: &Path, timezone: Timezone, missing_model: &'a str) -> Self {
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
            missing_model,
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
    fn entry(&self, event: agent_sessions::Located<Event>) -> Result<Option<RawEntry>, ()> {
        let Event::Usage(u) = event.value else {
            return Ok(None);
        };
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
        let model = u.model.unwrap_or_else(|| self.missing_model.into());
        let message_id = if let Some(total) = u.cumulative {
            usage_id(
                &model,
                &self.logical_key,
                session_reader::buckets(total).ok_or(())?,
                delta,
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
fn parse_mode(
    path: &Path,
    timezone: Timezone,
    debug: bool,
    scope: CodexScope,
    missing_model: &str,
    mode: CodexUsageMode,
) -> (ParseOutput, bool) {
    let mut out = ParseOutput {
        entries: Vec::new(),
        errors: 0,
    };
    let mut reader = match session_reader::reader(
        path,
        Agent::Codex,
        EventKinds::USAGE.union(EventKinds::META),
        mode,
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
    let mut projection = Projection::new(path, timezone, missing_model);
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
                Ok(Some(entry)) => out.entries.push(entry),
                Ok(None) => {}
                Err(()) => out.errors += 1,
            }
        }
    }
    let summary = reader.finish();
    (
        out,
        summary
            .ignored_types
            .get("token_usage_record")
            .is_some_and(|n| *n > 0),
    )
}
fn usage_id(model: &str, session: &str, total: [i64; 7], delta: [i64; 7]) -> String {
    let prefix = if total[..6] == delta[..6] {
        "codex-token-count"
    } else {
        "source-wide:codex-token-count"
    };
    format!(
        "{prefix}:{session}:{model}:total={},{},{},{},{},{}:delta={},{},{},{},{},{}",
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
