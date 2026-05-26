//! Grok session signal parser
//!
//! Grok persists session metadata under `~/.grok/sessions/<cwd>/<session>/`.
//! Local files currently expose context token usage rather than precise
//! provider input/output billing usage, so this parser reports those context
//! tokens as input tokens.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::consts::{DATE_FORMAT, UNKNOWN};
use crate::core::RawEntry;
use crate::source::ParseOutput;
use crate::utils::Timezone;

const DEFAULT_GROK_DIR: &str = ".grok";
const GROK_HOME_ENV: &str = "GROK_HOME";
const SESSIONS_SUBDIR: &str = "sessions";
const SIGNALS_FILE: &str = "signals.json";
const SUMMARY_FILE: &str = "summary.json";
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
    if let Ok(entries) = glob::glob(&format!("{}/**/{SIGNALS_FILE}", sessions_dir.display())) {
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

fn model_name(signals: &Signals, summary: &Summary) -> String {
    first_non_empty(&[
        signals.primary_model_id.as_deref(),
        summary.current_model_id.as_deref(),
        signals.models_used.first().map(String::as_str),
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
    let signals: Signals = match read_json(path, debug) {
        Ok(signals) => signals,
        Err(()) => {
            return ParseOutput {
                entries: Vec::new(),
                errors: 1,
            };
        }
    };

    let Some(session_dir) = path.parent() else {
        return ParseOutput {
            entries: Vec::new(),
            errors: 1,
        };
    };
    let summary_path = session_dir.join(SUMMARY_FILE);
    let summary: Summary = match read_json(&summary_path, debug) {
        Ok(summary) => summary,
        Err(()) => {
            return ParseOutput {
                entries: Vec::new(),
                errors: 1,
            };
        }
    };

    let total_tokens = total_context_tokens(&signals);
    if total_tokens == 0 {
        return ParseOutput::default();
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
            model: model_name(&signals, &summary),
            input_tokens: total_tokens,
            output_tokens: 0,
            cache_creation: 0,
            cache_read: 0,
            reasoning_tokens: 0,
            stop_reason: Some("context_snapshot".to_string()),
        }],
        errors: 0,
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
}
