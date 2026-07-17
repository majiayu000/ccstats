//! Kimi Code CLI wire log parser
//!
//! Kimi Code persists session wire logs under
//! `$KIMI_CODE_HOME/sessions/<workDirKey>/<sessionId>/agents/<agent>/wire.jsonl`
//! (default root: `~/.kimi-code`). Each `usage.record` line carries one turn's
//! API-reported token usage:
//!
//! ```json
//! {"type":"usage.record","model":"kimi-code/k3","usage":{"inputOther":5904,"output":658,"inputCacheRead":19200,"inputCacheCreation":0},"usageScope":"turn","time":1784247404495}
//! ```
//!
//! Session directories hold one `wire.jsonl` per agent (`main`, `agent-0`, ...);
//! all of them bill against the same session, so every wire file is parsed.
//! Project paths come from `$KIMI_CODE_HOME/session_index.jsonl`
//! (`sessionId` → `workDir`), falling back to the `<workDirKey>` slug.

use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::consts::{DATE_FORMAT, UNKNOWN};
use crate::core::{CostKind, RawEntry};
use crate::source::ParseOutput;
use crate::utils::Timezone;

const DEFAULT_KIMI_DIR: &str = ".kimi-code";
const KIMI_HOME_ENV: &str = "KIMI_CODE_HOME";
const SESSIONS_SUBDIR: &str = "sessions";
const AGENTS_SUBDIR: &str = "agents";
const WIRE_FILE: &str = "wire.jsonl";
const SESSION_INDEX_FILE: &str = "session_index.jsonl";
const USAGE_RECORD_TYPE: &str = "usage.record";
const TURN_SCOPE: &str = "turn";
const KIMI_MODEL: &str = "kimi";

#[derive(Debug, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct UsageTokens {
    input_other: Option<i64>,
    output: Option<i64>,
    input_cache_read: Option<i64>,
    input_cache_creation: Option<i64>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct UsageRecord {
    #[serde(rename = "type")]
    kind: String,
    model: Option<String>,
    usage: Option<UsageTokens>,
    usage_scope: Option<String>,
    time: Option<i64>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct SessionIndexEntry {
    session_id: Option<String>,
    work_dir: Option<String>,
}

fn get_kimi_sessions_dir() -> Option<PathBuf> {
    if let Ok(kimi_home) = env::var(KIMI_HOME_ENV) {
        let path = PathBuf::from(kimi_home).join(SESSIONS_SUBDIR);
        if path.is_dir() {
            return Some(path);
        }
    }

    let home = dirs::home_dir()?;
    let path = home.join(DEFAULT_KIMI_DIR).join(SESSIONS_SUBDIR);
    path.is_dir().then_some(path)
}

pub(super) fn find_kimi_files() -> Vec<PathBuf> {
    let Some(sessions_dir) = get_kimi_sessions_dir() else {
        return Vec::new();
    };

    let pattern = format!(
        "{}/**/{AGENTS_SUBDIR}/*/{WIRE_FILE}",
        sessions_dir.display()
    );
    let mut files = Vec::new();
    if let Ok(entries) = glob::glob(&pattern) {
        files.extend(entries.flatten().filter(|path| path.is_file()));
    }
    files.sort();
    files.dedup();
    files
}

/// `<sessions>/<workDirKey>/<sessionId>/agents/<agent>/wire.jsonl`
/// ancestors: 0=file, 1=agent, 2=agents, 3=session, 4=workDirKey, 5=sessions,
/// 6=root.
fn session_dir_of(path: &Path) -> Option<&Path> {
    path.ancestors().nth(3)
}

fn load_session_index(root: &Path, debug: bool) -> HashMap<String, String> {
    let mut index = HashMap::new();
    let index_path = root.join(SESSION_INDEX_FILE);
    let content = match fs::read_to_string(&index_path) {
        Ok(content) => content,
        Err(err) => {
            if err.kind() != std::io::ErrorKind::NotFound && debug {
                eprintln!("Failed to read {}: {}", index_path.display(), err);
            }
            return index;
        }
    };

    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let Ok(entry) = serde_json::from_str::<SessionIndexEntry>(line) else {
            if debug {
                eprintln!("Invalid JSON in {}: {line}", index_path.display());
            }
            continue;
        };
        if let (Some(session_id), Some(work_dir)) = (entry.session_id, entry.work_dir)
            && !session_id.is_empty()
            && !work_dir.is_empty()
        {
            index.insert(session_id, work_dir);
        }
    }
    index
}

/// Lossy fallback when the session index is unavailable: `wd_<slug>_<12 hex>`.
fn project_from_work_dir_key(work_dir_key: &str) -> Option<String> {
    let key = work_dir_key.strip_prefix("wd_")?;
    let (slug, hash) = key.rsplit_once('_')?;
    if slug.is_empty() || hash.len() != 12 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    Some(slug.to_string())
}

fn project_path(path: &Path, session_index: &HashMap<String, String>, session_id: &str) -> String {
    if let Some(work_dir) = session_index.get(session_id) {
        return work_dir.clone();
    }

    path.ancestors()
        .nth(4)
        .and_then(|dir| dir.file_name())
        .and_then(|name| name.to_str())
        .and_then(project_from_work_dir_key)
        .unwrap_or_default()
}

fn non_empty_model(model: Option<&str>) -> String {
    model
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(KIMI_MODEL)
        .to_string()
}

/// Per-file session identity shared by every entry parsed from one wire file.
struct SessionContext {
    session_id: String,
    session_key: String,
    project_path: String,
}

fn session_context(path: &Path, debug: bool) -> SessionContext {
    let session_dir = session_dir_of(path);
    let session_id = session_dir
        .and_then(|dir| dir.file_name())
        .and_then(|name| name.to_str())
        .unwrap_or(UNKNOWN)
        .to_string();
    let session_key =
        session_dir.map_or_else(|| session_id.clone(), |dir| dir.display().to_string());
    let session_index = path
        .ancestors()
        .nth(6)
        .map_or_else(HashMap::new, |root| load_session_index(root, debug));
    SessionContext {
        project_path: project_path(path, &session_index, &session_id),
        session_id,
        session_key,
    }
}

fn parse_usage_line(
    line: &str,
    line_index: usize,
    path: &Path,
    timezone: Timezone,
    debug: bool,
    ctx: &SessionContext,
    errors: &mut usize,
) -> Option<RawEntry> {
    // Cheap pre-filter: wire files carry large conversation payloads.
    if !line.contains(USAGE_RECORD_TYPE) {
        return None;
    }
    let record: UsageRecord = match serde_json::from_str(line) {
        Ok(record) => record,
        Err(err) => {
            *errors += 1;
            if debug {
                eprintln!(
                    "Invalid JSON in {} line {}: {}",
                    path.display(),
                    line_index + 1,
                    err
                );
            }
            return None;
        }
    };
    if record.kind != USAGE_RECORD_TYPE {
        // Conversation payloads may mention the token; only typed records count.
        return None;
    }
    if let Some(scope) = record.usage_scope.as_deref()
        && scope != TURN_SCOPE
    {
        // Non-turn scopes (e.g. cumulative session totals) would double count.
        return None;
    }

    let Some(time_ms) = record.time.filter(|time| *time > 0) else {
        *errors += 1;
        if debug {
            eprintln!(
                "Missing valid time in {} line {}",
                path.display(),
                line_index + 1
            );
        }
        return None;
    };
    let Some(utc_dt) = DateTime::<Utc>::from_timestamp_millis(time_ms) else {
        *errors += 1;
        if debug {
            eprintln!(
                "Out-of-range time in {} line {}",
                path.display(),
                line_index + 1
            );
        }
        return None;
    };

    let usage = record.usage.unwrap_or_default();
    let input_tokens = usage.input_other.unwrap_or(0).max(0);
    let output_tokens = usage.output.unwrap_or(0).max(0);
    let cache_read = usage.input_cache_read.unwrap_or(0).max(0);
    let cache_creation = usage.input_cache_creation.unwrap_or(0).max(0);
    if input_tokens == 0 && output_tokens == 0 && cache_read == 0 && cache_creation == 0 {
        return None;
    }

    let local_dt = timezone.to_fixed_offset(utc_dt);
    Some(RawEntry {
        timestamp: utc_dt.to_rfc3339(),
        timestamp_ms: time_ms,
        date_str: local_dt.date_naive().format(DATE_FORMAT).to_string(),
        message_id: None,
        session_key: ctx.session_key.clone(),
        session_id: ctx.session_id.clone(),
        project_path: ctx.project_path.clone(),
        model: non_empty_model(record.model.as_deref()),
        input_tokens,
        output_tokens,
        cache_creation,
        cache_creation_1h: 0,
        cache_read,
        reasoning_tokens: 0,
        stop_reason: None,
        cost_kind: CostKind::Real,
        endpoint: crate::core::Endpoint::Unknown,
    })
}

pub(super) fn parse_kimi_wire_file_with_debug(
    path: &Path,
    timezone: Timezone,
    debug: bool,
) -> ParseOutput {
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(err) => {
            if debug {
                eprintln!("Failed to read {}: {}", path.display(), err);
            }
            return ParseOutput {
                entries: Vec::new(),
                errors: 1,
            };
        }
    };

    let ctx = session_context(path, debug);
    let mut entries = Vec::new();
    let mut errors = 0;
    for (line_index, line) in content.lines().enumerate() {
        if let Some(entry) =
            parse_usage_line(line, line_index, path, timezone, debug, &ctx, &mut errors)
        {
            entries.push(entry);
        }
    }

    ParseOutput { entries, errors }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn tz() -> Timezone {
        Timezone::parse(Some("UTC")).unwrap()
    }

    fn write_wire_session(root: &Path, work_dir_key: &str, session_id: &str) -> PathBuf {
        write_wire_session_with_index(root, work_dir_key, session_id, true)
    }

    fn write_wire_session_with_index(
        root: &Path,
        work_dir_key: &str,
        session_id: &str,
        with_index: bool,
    ) -> PathBuf {
        let session_dir = root
            .join(SESSIONS_SUBDIR)
            .join(work_dir_key)
            .join(session_id);
        let main_dir = session_dir.join(AGENTS_SUBDIR).join("main");
        fs::create_dir_all(&main_dir).expect("create agent dir");
        if with_index {
            fs::write(
                root.join(SESSION_INDEX_FILE),
                format!(
                    r#"{{"sessionId":"{session_id}","sessionDir":"{}","workDir":"/tmp/kimi-project"}}"#,
                    session_dir.display()
                ),
            )
            .expect("write session index");
        }
        main_dir.join(WIRE_FILE)
    }

    #[test]
    fn parses_turn_usage_records() {
        let root = tempdir().expect("temp dir");
        let wire = write_wire_session(root.path(), "wd_proj_6c618ba503c5", "session-1");
        fs::write(
            &wire,
            r#"{"type":"metadata","protocol_version":1,"created_at":"2026-07-16T00:00:00Z"}
{"type":"turn.prompt","time":1784247400000,"input":"hello, do not count me"}
{"type":"usage.record","model":"kimi-code/k3","usage":{"inputOther":1000,"output":500,"inputCacheRead":200,"inputCacheCreation":300},"usageScope":"turn","time":1784247404495}
{"type":"usage.record","model":"kimi-code/k3","usage":{"inputOther":2000,"output":1000,"inputCacheRead":0,"inputCacheCreation":0},"usageScope":"turn","time":1784247422916}
"#,
        )
        .expect("write wire");

        let parsed = parse_kimi_wire_file_with_debug(&wire, tz(), true);
        assert_eq!(parsed.errors, 0);
        assert_eq!(parsed.entries.len(), 2);

        let entry = &parsed.entries[0];
        assert_eq!(entry.input_tokens, 1000);
        assert_eq!(entry.output_tokens, 500);
        assert_eq!(entry.cache_read, 200);
        assert_eq!(entry.cache_creation, 300);
        assert_eq!(entry.reasoning_tokens, 0);
        assert_eq!(entry.model, "kimi-code/k3");
        assert_eq!(entry.session_id, "session-1");
        assert_eq!(entry.project_path, "/tmp/kimi-project");
        assert_eq!(entry.date_str, "2026-07-17");
        assert_eq!(entry.timestamp_ms, 1_784_247_404_495);
        assert_eq!(entry.cost_kind, CostKind::Real);

        let total_input: i64 = parsed.entries.iter().map(|e| e.input_tokens).sum();
        assert_eq!(total_input, 3000);
    }

    #[test]
    fn skips_lines_that_only_mention_usage_record() {
        let root = tempdir().expect("temp dir");
        let wire = write_wire_session(root.path(), "wd_proj_6c618ba503c5", "session-1");
        fs::write(
            &wire,
            r#"{"type":"turn.prompt","time":1784247400000,"input":"explain what a usage.record is"}
{"type":"context.append_message","time":1784247401000,"message":"usage.record lines are JSON"}
"#,
        )
        .expect("write wire");

        let parsed = parse_kimi_wire_file_with_debug(&wire, tz(), true);
        assert_eq!(parsed.errors, 0);
        assert!(parsed.entries.is_empty());
    }

    #[test]
    fn counts_malformed_usage_record_as_error() {
        let root = tempdir().expect("temp dir");
        let wire = write_wire_session(root.path(), "wd_proj_6c618ba503c5", "session-1");
        fs::write(
            &wire,
            r#"{"type":"usage.record","model":"kimi-code/k3","usage":broken
{"type":"usage.record","model":"kimi-code/k3","usage":{"inputOther":10,"output":5},"usageScope":"turn"}
"#,
        )
        .expect("write wire");

        let parsed = parse_kimi_wire_file_with_debug(&wire, tz(), true);
        assert_eq!(parsed.errors, 2); // broken JSON + missing time
        assert!(parsed.entries.is_empty());
    }

    #[test]
    fn skips_non_turn_scopes_to_avoid_double_counting() {
        let root = tempdir().expect("temp dir");
        let wire = write_wire_session(root.path(), "wd_proj_6c618ba503c5", "session-1");
        fs::write(
            &wire,
            r#"{"type":"usage.record","model":"kimi-code/k3","usage":{"inputOther":100,"output":50},"usageScope":"turn","time":1784247404495}
{"type":"usage.record","model":"kimi-code/k3","usage":{"inputOther":99999,"output":99999},"usageScope":"session","time":1784247404500}
"#,
        )
        .expect("write wire");

        let parsed = parse_kimi_wire_file_with_debug(&wire, tz(), true);
        assert_eq!(parsed.errors, 0);
        assert_eq!(parsed.entries.len(), 1);
        assert_eq!(parsed.entries[0].input_tokens, 100);
    }

    #[test]
    fn skips_all_zero_usage_records() {
        let root = tempdir().expect("temp dir");
        let wire = write_wire_session(root.path(), "wd_proj_6c618ba503c5", "session-1");
        fs::write(
            &wire,
            r#"{"type":"usage.record","model":"kimi-code/k3","usage":{"inputOther":0,"output":0,"inputCacheRead":0,"inputCacheCreation":0},"usageScope":"turn","time":1784247404495}
"#,
        )
        .expect("write wire");

        let parsed = parse_kimi_wire_file_with_debug(&wire, tz(), true);
        assert_eq!(parsed.errors, 0);
        assert!(parsed.entries.is_empty());
    }

    #[test]
    fn falls_back_to_work_dir_key_slug_without_index() {
        let root = tempdir().expect("temp dir");
        let wire = write_wire_session_with_index(
            root.path(),
            "wd_ccstats_6c618ba503c5",
            "session-9",
            false,
        );
        fs::write(
            &wire,
            r#"{"type":"usage.record","model":"kimi-code/k3","usage":{"inputOther":10,"output":5},"usageScope":"turn","time":1784247404495}
"#,
        )
        .expect("write wire");

        let parsed = parse_kimi_wire_file_with_debug(&wire, tz(), true);
        assert_eq!(parsed.entries[0].project_path, "ccstats");
    }

    #[test]
    fn falls_back_to_default_model_name() {
        let root = tempdir().expect("temp dir");
        let wire = write_wire_session(root.path(), "wd_proj_6c618ba503c5", "session-1");
        fs::write(
            &wire,
            r#"{"type":"usage.record","usage":{"inputOther":10,"output":5},"usageScope":"turn","time":1784247404495}
"#,
        )
        .expect("write wire");

        let parsed = parse_kimi_wire_file_with_debug(&wire, tz(), true);
        assert_eq!(parsed.entries[0].model, "kimi");
    }

    #[test]
    fn clamps_negative_token_counts() {
        let root = tempdir().expect("temp dir");
        let wire = write_wire_session(root.path(), "wd_proj_6c618ba503c5", "session-1");
        fs::write(
            &wire,
            r#"{"type":"usage.record","model":"kimi-code/k3","usage":{"inputOther":-10,"output":5,"inputCacheRead":-3},"usageScope":"turn","time":1784247404495}
"#,
        )
        .expect("write wire");

        let parsed = parse_kimi_wire_file_with_debug(&wire, tz(), true);
        assert_eq!(parsed.entries[0].input_tokens, 0);
        assert_eq!(parsed.entries[0].output_tokens, 5);
        assert_eq!(parsed.entries[0].cache_read, 0);
    }

    #[test]
    fn sub_agent_wire_files_share_session_identity() {
        let root = tempdir().expect("temp dir");
        let wire = write_wire_session(root.path(), "wd_proj_6c618ba503c5", "session-1");
        let sub_agent = wire
            .parent()
            .and_then(Path::parent)
            .expect("agents dir")
            .join("agent-0")
            .join(WIRE_FILE);
        fs::create_dir_all(sub_agent.parent().expect("agent dir")).expect("create sub agent dir");
        fs::write(
            &sub_agent,
            r#"{"type":"usage.record","model":"kimi-code/k3","usage":{"inputOther":7,"output":3},"usageScope":"turn","time":1784247404495}
"#,
        )
        .expect("write wire");

        let parsed = parse_kimi_wire_file_with_debug(&sub_agent, tz(), true);
        assert_eq!(parsed.entries.len(), 1);
        assert_eq!(parsed.entries[0].session_id, "session-1");
        assert_eq!(parsed.entries[0].project_path, "/tmp/kimi-project");
    }

    #[test]
    fn work_dir_key_requires_wd_prefix_and_hash() {
        assert_eq!(
            project_from_work_dir_key("wd_ccstats_6c618ba503c5"),
            Some("ccstats".to_string())
        );
        assert_eq!(project_from_work_dir_key("ccstats"), None);
        assert_eq!(project_from_work_dir_key("wd_ccstats_short"), None);
        assert_eq!(project_from_work_dir_key("wd__6c618ba503c5"), None);
    }
}
