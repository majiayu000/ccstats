//! Grok session signal parser
//!
//! Grok persists session metadata under `~/.grok/sessions/<cwd>/<session>/`.
//! Local files currently expose context-token snapshots rather than precise
//! provider input/output billing or remote account quota usage, so this parser
//! reports those context tokens as input tokens.

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

const DEFAULT_GROK_DIR: &str = ".grok";
const GROK_HOME_ENV: &str = "GROK_HOME";
const SESSIONS_SUBDIR: &str = "sessions";
const SIGNALS_FILE: &str = "signals.json";
const SUMMARY_FILE: &str = "summary.json";
const UPDATES_FILE: &str = "updates.jsonl";
const GROK_MODEL: &str = "grok";

#[derive(Debug, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct Signals {
    context_tokens_used: Option<i64>,
    total_tokens_before_compaction: Option<i64>,
    primary_model_id: Option<String>,
    models_used: Vec<String>,
}

#[derive(Debug, Deserialize, Default)]
struct Summary {
    created_at: Option<String>,
    updated_at: Option<String>,
    last_active_at: Option<String>,
    current_model_id: Option<String>,
    git_root_dir: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct UpdateMeta {
    total_tokens: Option<i64>,
    prompt_id: Option<String>,
    turn_start_ms: Option<i64>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct UpdateParams {
    #[serde(rename = "_meta")]
    meta: Option<UpdateMeta>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct UpdateEnvelope {
    params: Option<UpdateParams>,
}

fn get_grok_sessions_dir() -> Option<PathBuf> {
    if let Ok(grok_home) = env::var(GROK_HOME_ENV) {
        let path = PathBuf::from(grok_home).join(SESSIONS_SUBDIR);
        if path.is_dir() {
            return Some(path);
        }
    }

    let home = dirs::home_dir()?;
    let path = home.join(DEFAULT_GROK_DIR).join(SESSIONS_SUBDIR);
    path.is_dir().then_some(path)
}

pub(super) fn find_grok_files() -> Vec<PathBuf> {
    let Some(sessions_dir) = get_grok_sessions_dir() else {
        return Vec::new();
    };

    let mut files = Vec::new();
    if let Ok(entries) = glob::glob(&format!("{}/**/{SUMMARY_FILE}", sessions_dir.display())) {
        files.extend(entries.flatten().filter(|path| path.is_file()));
    }
    files.sort();
    files.dedup();
    files
}

fn read_json<T>(path: &Path, debug: bool) -> Result<T, ()>
where
    T: for<'de> Deserialize<'de>,
{
    let content = fs::read_to_string(path).map_err(|err| {
        if debug {
            eprintln!("Failed to read {}: {}", path.display(), err);
        }
    })?;
    serde_json::from_str(&content).map_err(|err| {
        if debug {
            eprintln!("Invalid JSON in {}: {}", path.display(), err);
        }
    })
}

fn read_optional_json<T>(path: &Path, debug: bool) -> Result<Option<T>, ()>
where
    T: for<'de> Deserialize<'de>,
{
    match fs::read_to_string(path) {
        Ok(content) => serde_json::from_str(&content).map(Some).map_err(|err| {
            if debug {
                eprintln!("Invalid JSON in {}: {}", path.display(), err);
            }
        }),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => {
            if debug {
                eprintln!("Failed to read {}: {}", path.display(), err);
            }
            Err(())
        }
    }
}

fn first_non_empty(values: &[Option<&str>]) -> Option<String> {
    values
        .iter()
        .flatten()
        .map(|value| value.trim())
        .find(|value| !value.is_empty())
        .map(std::string::ToString::to_string)
}

fn parse_timestamp(summary: &Summary) -> Option<DateTime<Utc>> {
    first_non_empty(&[
        summary.updated_at.as_deref(),
        summary.last_active_at.as_deref(),
        summary.created_at.as_deref(),
    ])
    .and_then(|timestamp| timestamp.parse::<DateTime<Utc>>().ok())
}

fn total_context_tokens(signals: &Signals) -> i64 {
    signals.context_tokens_used.unwrap_or(0).max(0)
        + signals.total_tokens_before_compaction.unwrap_or(0).max(0)
}

fn total_update_tokens(path: &Path, debug: bool) -> (i64, usize) {
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return (0, 0),
        Err(err) => {
            if debug {
                eprintln!("Failed to read {}: {}", path.display(), err);
            }
            return (0, 1);
        }
    };

    let mut keyed_maxes: HashMap<String, i64> = HashMap::new();
    let mut session_max = 0;
    let mut errors = 0;

    for (line_index, line) in content.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let envelope: UpdateEnvelope = match serde_json::from_str(line) {
            Ok(envelope) => envelope,
            Err(err) => {
                errors += 1;
                if debug {
                    eprintln!(
                        "Invalid JSON in {} line {}: {}",
                        path.display(),
                        line_index + 1,
                        err
                    );
                }
                continue;
            }
        };
        let Some(meta) = envelope.params.and_then(|params| params.meta) else {
            continue;
        };
        let Some(total_tokens) = meta.total_tokens.map(|tokens| tokens.max(0)) else {
            continue;
        };
        session_max = session_max.max(total_tokens);

        let group_key = first_non_empty(&[meta.prompt_id.as_deref()])
            .map(|prompt_id| format!("prompt:{prompt_id}"))
            .or_else(|| {
                meta.turn_start_ms
                    .filter(|turn_start_ms| *turn_start_ms > 0)
                    .map(|turn_start_ms| format!("turn:{turn_start_ms}"))
            });
        if let Some(group_key) = group_key {
            let group_max = keyed_maxes.entry(group_key).or_default();
            *group_max = (*group_max).max(total_tokens);
        }
    }

    let total = if keyed_maxes.is_empty() {
        session_max
    } else {
        keyed_maxes.values().sum()
    };
    (total, errors)
}

fn model_name(signals: Option<&Signals>, summary: &Summary) -> String {
    first_non_empty(&[
        signals.and_then(|value| value.primary_model_id.as_deref()),
        summary.current_model_id.as_deref(),
        signals.and_then(|value| value.models_used.first().map(String::as_str)),
    ])
    .unwrap_or_else(|| GROK_MODEL.to_string())
}

fn project_path(path: &Path, summary: &Summary) -> String {
    if let Some(project) = summary
        .git_root_dir
        .as_deref()
        .map(str::trim)
        .filter(|project| !project.is_empty())
    {
        return project.to_string();
    }

    path.parent()
        .and_then(Path::parent)
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        .map(percent_decode_component)
        .unwrap_or_default()
}

fn percent_decode_component(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(hi), Some(lo)) = (hex_value(bytes[i + 1]), hex_value(bytes[i + 2]))
        {
            output.push((hi << 4) | lo);
            i += 3;
            continue;
        }
        output.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(output).unwrap_or_else(|_| value.to_string())
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

pub(super) fn parse_grok_signal_file_with_debug(
    path: &Path,
    timezone: Timezone,
    debug: bool,
) -> ParseOutput {
    let summary_path = if path.file_name().and_then(|name| name.to_str()) == Some(SUMMARY_FILE) {
        path.to_path_buf()
    } else {
        let Some(session_dir) = path.parent() else {
            return ParseOutput {
                entries: Vec::new(),
                errors: 1,
            };
        };
        session_dir.join(SUMMARY_FILE)
    };

    let Some(session_dir) = summary_path.parent() else {
        return ParseOutput {
            entries: Vec::new(),
            errors: 1,
        };
    };
    let summary: Summary = match read_json(&summary_path, debug) {
        Ok(summary) => summary,
        Err(()) => {
            return ParseOutput {
                entries: Vec::new(),
                errors: 1,
            };
        }
    };

    let signals_path = session_dir.join(SIGNALS_FILE);
    let (signals, mut errors) = match read_optional_json::<Signals>(&signals_path, debug) {
        Ok(signals) => (signals, 0),
        Err(()) => (None, 1),
    };
    let signal_tokens = signals.as_ref().map_or(0, total_context_tokens);
    let total_tokens = if signal_tokens > 0 {
        signal_tokens
    } else {
        let (update_tokens, update_errors) =
            total_update_tokens(&session_dir.join(UPDATES_FILE), debug);
        errors += update_errors;
        update_tokens
    };
    if total_tokens == 0 {
        return ParseOutput {
            entries: Vec::new(),
            errors,
        };
    }

    let Some(utc_dt) = parse_timestamp(&summary) else {
        if debug {
            eprintln!("Missing valid timestamp in {}", summary_path.display());
        }
        return ParseOutput {
            entries: Vec::new(),
            errors: 1,
        };
    };
    let local_dt = timezone.to_fixed_offset(utc_dt);
    let session_id = session_dir
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(UNKNOWN)
        .to_string();

    ParseOutput {
        entries: vec![RawEntry {
            timestamp: utc_dt.to_rfc3339(),
            timestamp_ms: utc_dt.timestamp_millis(),
            date_str: local_dt.date_naive().format(DATE_FORMAT).to_string(),
            message_id: Some(session_id.clone()),
            session_key: session_dir.display().to_string(),
            session_id,
            project_path: project_path(path, &summary),
            model: model_name(signals.as_ref(), &summary),
            input_tokens: total_tokens,
            output_tokens: 0,
            cache_creation: 0,
            cache_creation_1h: 0,
            cache_read: 0,
            reasoning_tokens: 0,
            stop_reason: Some("context_snapshot".to_string()),
            cost_kind: CostKind::EstimatedProxy,
        }],
        errors,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::Timezone;
    use tempfile::tempdir;

    fn tz() -> Timezone {
        Timezone::parse(Some("UTC")).unwrap()
    }

    #[test]
    fn percent_decode_decodes_encoded_project_path() {
        assert_eq!(
            percent_decode_component("%2FUsers%2Fme%2Fproject"),
            "/Users/me/project"
        );
    }

    #[test]
    fn parse_grok_signal_file_reads_context_tokens() {
        let root = tempdir().expect("temp dir");
        let session_dir = root
            .path()
            .join("%2FUsers%2Fme%2Fproject")
            .join("session-1");
        fs::create_dir_all(&session_dir).expect("create session dir");
        fs::write(
            session_dir.join(SIGNALS_FILE),
            r#"{
  "contextTokensUsed": 1200,
  "totalTokensBeforeCompaction": 300,
  "primaryModelId": "grok-build",
  "modelsUsed": ["grok-build"]
}"#,
        )
        .expect("write signals");
        fs::write(
            session_dir.join(SUMMARY_FILE),
            r#"{
  "updated_at": "2026-05-26T03:35:24.335481Z",
  "current_model_id": "grok-build"
}"#,
        )
        .expect("write summary");

        let parsed = parse_grok_signal_file_with_debug(&session_dir.join(SIGNALS_FILE), tz(), true);
        assert_eq!(parsed.errors, 0);
        assert_eq!(parsed.entries.len(), 1);
        let entry = &parsed.entries[0];
        assert_eq!(entry.input_tokens, 1500);
        assert_eq!(entry.output_tokens, 0);
        assert_eq!(entry.model, "grok-build");
        assert_eq!(entry.project_path, "/Users/me/project");
        assert_eq!(entry.date_str, "2026-05-26");
    }

    #[test]
    fn parse_grok_signal_file_prefers_summary_project_path() {
        let root = tempdir().expect("temp dir");
        let session_dir = root.path().join("%2Fencoded").join("session-1");
        fs::create_dir_all(&session_dir).expect("create session dir");
        fs::write(
            session_dir.join(SIGNALS_FILE),
            r#"{"contextTokensUsed": 10, "primaryModelId": "grok-4.3"}"#,
        )
        .expect("write signals");
        fs::write(
            session_dir.join(SUMMARY_FILE),
            r#"{
  "updated_at": "2026-05-26T03:35:24Z",
  "git_root_dir": "/repo/from-summary/",
  "current_model_id": "grok-build"
}"#,
        )
        .expect("write summary");

        let parsed = parse_grok_signal_file_with_debug(&session_dir.join(SIGNALS_FILE), tz(), true);
        assert_eq!(parsed.entries[0].project_path, "/repo/from-summary/");
        assert_eq!(parsed.entries[0].model, "grok-4.3");
    }

    #[test]
    fn parse_grok_signal_file_falls_back_to_update_metadata() -> std::io::Result<()> {
        let root = tempdir()?;
        let session_dir = root.path().join("%2Fencoded").join("session-1");
        fs::create_dir_all(&session_dir)?;
        fs::write(
            session_dir.join(SUMMARY_FILE),
            r#"{
  "updated_at": "2026-05-26T03:35:24Z",
  "current_model_id": "grok-build"
}"#,
        )?;
        fs::write(
            session_dir.join(UPDATES_FILE),
            r#"{"params":{"_meta":{"totalTokens":100}}}
{"params":{"_meta":{"totalTokens":250}}}
"#,
        )?;

        let parsed = parse_grok_signal_file_with_debug(&session_dir.join(SUMMARY_FILE), tz(), true);
        assert_eq!(parsed.errors, 0);
        assert_eq!(parsed.entries.len(), 1);
        assert_eq!(parsed.entries[0].input_tokens, 250);
        assert_eq!(parsed.entries[0].model, "grok-build");
        Ok(())
    }
}
