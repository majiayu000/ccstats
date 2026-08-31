//! Cline CLI and VS Code extension local usage source.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::Value;

use super::cline_extension::{CLINE_EXTENSION_ID, find_extension_files, parse_extension_file};
use crate::consts::{DATE_FORMAT, UNKNOWN};
use crate::core::{CostKind, Endpoint, RawEntry};
use crate::source::{Capabilities, ParseOutput, Source};
use crate::utils::Timezone;

const CLINE_SESSION_DATA_DIR_ENV: &str = "CLINE_SESSION_DATA_DIR";
const CLINE_DATA_DIR_ENV: &str = "CLINE_DATA_DIR";
const CLINE_DIR_ENV: &str = "CLINE_DIR";

pub(crate) struct ClineSource;

impl ClineSource {
    pub(crate) const fn new() -> Self {
        Self
    }
}

impl Source for ClineSource {
    fn name(&self) -> &'static str {
        "cline"
    }

    fn display_name(&self) -> &'static str {
        "Cline"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["cl"]
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            has_projects: true,
            has_billing_blocks: false,
            has_reasoning_tokens: false,
            has_cache_creation: true,
            has_cache_read: true,
            needs_dedup: false,
            has_tool_calls: false,
            has_endpoints: false,
        }
    }

    fn find_files(&self) -> Vec<PathBuf> {
        let mut files = find_cline_cli_files();
        files.extend(find_extension_files(CLINE_EXTENSION_ID));
        files.sort();
        files.dedup();
        files
    }

    fn parse_file(&self, path: &Path, timezone: Timezone, debug: bool) -> ParseOutput {
        if is_cli_messages_path(path) {
            parse_cline_cli_file(path, timezone, debug)
        } else {
            parse_extension_file(path, self.name(), timezone, debug)
        }
    }
}

fn non_empty_env(name: &str) -> Option<PathBuf> {
    env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn cline_cli_sessions_dir() -> Option<PathBuf> {
    if let Some(path) = non_empty_env(CLINE_SESSION_DATA_DIR_ENV) {
        return Some(path);
    }
    if let Some(path) = non_empty_env(CLINE_DATA_DIR_ENV) {
        return Some(path.join("sessions"));
    }
    if let Some(path) = non_empty_env(CLINE_DIR_ENV) {
        return Some(path.join("data/sessions"));
    }
    dirs::home_dir().map(|home| home.join(".cline/data/sessions"))
}

fn find_cline_cli_files() -> Vec<PathBuf> {
    let Some(root) = cline_cli_sessions_dir() else {
        return Vec::new();
    };
    let pattern = root.join("**").join("*.messages.json");
    let mut files = glob::glob(&pattern.to_string_lossy())
        .into_iter()
        .flatten()
        .flatten()
        .filter(|path| path.is_file())
        .collect::<Vec<_>>();
    files.sort();
    files.dedup();
    files
}

fn is_cli_messages_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with(".messages.json"))
}

fn manifest_path(path: &Path) -> PathBuf {
    let stem = path
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    let session_stem = stem.strip_suffix(".messages").unwrap_or(stem);
    path.parent()
        .unwrap_or_else(|| Path::new("."))
        .join(format!("{session_stem}.json"))
}

fn file_modified_ms(path: &Path) -> Option<i64> {
    let millis = fs::metadata(path)
        .ok()?
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_millis();
    i64::try_from(millis).ok()
}

fn value_i64(value: Option<&Value>) -> Option<i64> {
    value.and_then(|value| {
        value
            .as_i64()
            .or_else(|| value.as_u64().and_then(|number| i64::try_from(number).ok()))
            .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
    })
}

fn numeric_timestamp(value: i64) -> Option<i64> {
    if value <= 0 {
        None
    } else if value >= 1_000_000_000_000 {
        Some(value)
    } else {
        Some(value.saturating_mul(1_000))
    }
}

fn parse_timestamp(value: Option<&Value>) -> Option<i64> {
    let value = value?;
    if let Some(text) = value.as_str() {
        if let Ok(timestamp) = DateTime::parse_from_rfc3339(text) {
            return Some(timestamp.timestamp_millis());
        }
        return text.parse().ok().and_then(numeric_timestamp);
    }
    value_i64(Some(value)).and_then(numeric_timestamp)
}

fn non_empty(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[derive(Debug, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct ClineMessagesFile {
    session_id: Option<String>,
    messages: Vec<ClineMessage>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct ClineMessage {
    id: Option<String>,
    role: Option<String>,
    ts: Option<Value>,
    model_info: Option<ModelInfo>,
    metrics: Option<Metrics>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct ModelInfo {
    id: Option<String>,
    provider: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct Metrics {
    #[serde(rename = "inputTokens")]
    input: Option<Value>,
    #[serde(rename = "outputTokens")]
    output: Option<Value>,
    #[serde(rename = "cacheReadTokens")]
    cache_read: Option<Value>,
    #[serde(rename = "cacheWriteTokens")]
    cache_write: Option<Value>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct Manifest {
    session_id: Option<String>,
    model: Option<String>,
    provider: Option<String>,
    workspace_root: Option<String>,
    cwd: Option<String>,
}

fn read_manifest(path: &Path, debug: bool) -> (Manifest, usize) {
    let path = manifest_path(path);
    let content = match fs::read_to_string(&path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return (Manifest::default(), 0);
        }
        Err(error) => {
            if debug {
                eprintln!("Failed to read {}: {error}", path.display());
            }
            return (Manifest::default(), 1);
        }
    };
    match serde_json::from_str(&content) {
        Ok(manifest) => (manifest, 0),
        Err(error) => {
            if debug {
                eprintln!("Invalid JSON in {}: {error}", path.display());
            }
            (Manifest::default(), 1)
        }
    }
}

fn filename_session_id(path: &Path) -> String {
    path.file_stem()
        .and_then(|name| name.to_str())
        .and_then(|name| name.strip_suffix(".messages"))
        .filter(|name| !name.is_empty())
        .unwrap_or(UNKNOWN)
        .to_string()
}

fn read_messages_file(path: &Path, debug: bool) -> Result<ClineMessagesFile, ()> {
    let content = fs::read_to_string(path).map_err(|error| {
        if debug {
            eprintln!("Failed to read {}: {error}", path.display());
        }
    })?;
    serde_json::from_str(&content).map_err(|error| {
        if debug {
            eprintln!("Invalid JSON in {}: {error}", path.display());
        }
    })
}

fn parse_cline_cli_file(path: &Path, timezone: Timezone, debug: bool) -> ParseOutput {
    let Ok(file) = read_messages_file(path, debug) else {
        return ParseOutput {
            entries: Vec::new(),
            errors: 1,
        };
    };
    let (manifest, manifest_errors) = read_manifest(path, debug);
    let session_id = non_empty(file.session_id)
        .or_else(|| non_empty(manifest.session_id))
        .unwrap_or_else(|| filename_session_id(path));
    let project_path = non_empty(manifest.workspace_root)
        .or_else(|| non_empty(manifest.cwd))
        .unwrap_or_default();
    let mut current_model = non_empty(manifest.model).unwrap_or_else(|| UNKNOWN.to_string());
    let mut _current_provider = non_empty(manifest.provider).unwrap_or_else(|| UNKNOWN.to_string());
    let fallback_timestamp = file_modified_ms(path);
    let mut output = ParseOutput {
        entries: Vec::new(),
        errors: manifest_errors,
    };
    let mut assistant_index = 0usize;

    for message in file.messages {
        if message.role.as_deref() != Some("assistant") {
            continue;
        }
        if let Some(model_info) = message.model_info {
            if let Some(model) = non_empty(model_info.id) {
                current_model = model;
            }
            if let Some(provider) = non_empty(model_info.provider) {
                _current_provider = provider;
            }
        }
        let Some(metrics) = message.metrics else {
            continue;
        };
        let raw_input = value_i64(metrics.input.as_ref()).unwrap_or(0).max(0);
        let output_tokens = value_i64(metrics.output.as_ref()).unwrap_or(0).max(0);
        let cache_read = value_i64(metrics.cache_read.as_ref()).unwrap_or(0).max(0);
        let cache_creation = value_i64(metrics.cache_write.as_ref()).unwrap_or(0).max(0);
        let input_tokens = raw_input
            .saturating_sub(cache_read)
            .saturating_sub(cache_creation);
        if input_tokens == 0 && output_tokens == 0 && cache_read == 0 && cache_creation == 0 {
            continue;
        }
        let timestamp_ms = parse_timestamp(message.ts.as_ref()).or(fallback_timestamp);
        let Some(timestamp_ms) = timestamp_ms else {
            output.errors += 1;
            continue;
        };
        let Some(utc) = DateTime::<Utc>::from_timestamp_millis(timestamp_ms) else {
            output.errors += 1;
            continue;
        };
        let message_id = non_empty(message.id)
            .map(|id| format!("cline-cli:{session_id}:{id}"))
            .or_else(|| {
                let id = format!("cline-cli:{session_id}:{assistant_index}");
                Some(id)
            });
        assistant_index += 1;
        output.entries.push(RawEntry {
            timestamp: utc.to_rfc3339(),
            timestamp_ms,
            date_str: timezone
                .to_fixed_offset(utc)
                .date_naive()
                .format(DATE_FORMAT)
                .to_string(),
            message_id,
            session_key: format!("cline-cli:{}::{session_id}", path.display()),
            session_id: session_id.clone(),
            project_path: project_path.clone(),
            model: current_model.clone(),
            input_tokens,
            output_tokens,
            cache_creation,
            cache_creation_1h: 0,
            cache_read,
            reasoning_tokens: 0,
            stop_reason: None,
            cost_kind: CostKind::Real,
            endpoint: Endpoint::Unknown,
            call_count: 1,
            recorded_cost_usd: None,
            api_equivalent_priced_tokens: 0,
            api_equivalent_coverage_tokens: 0,
        });
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    use tempfile::tempdir;

    #[test]
    fn cli_metrics_remove_cache_from_inclusive_input() {
        let temp = tempdir().unwrap();
        let session = temp.path().join("session-1");
        fs::create_dir_all(&session).unwrap();
        let path = session.join("session-1.messages.json");
        fs::write(
            &path,
            r#"{
                "sessionId": "cline-session",
                "messages": [{
                    "id": "assistant-1",
                    "role": "assistant",
                    "ts": "2026-08-31T03:04:05Z",
                    "modelInfo": {"id": "claude-sonnet-4", "provider": "anthropic"},
                    "metrics": {
                        "inputTokens": 100,
                        "outputTokens": 20,
                        "cacheReadTokens": 30,
                        "cacheWriteTokens": 10
                    }
                }]
            }"#,
        )
        .unwrap();
        fs::write(
            session.join("session-1.json"),
            r#"{"workspace_root":"/work/project","model":"fallback-model"}"#,
        )
        .unwrap();

        let parsed = parse_cline_cli_file(&path, Timezone::Named(chrono_tz::UTC), false);

        assert_eq!(parsed.errors, 0);
        assert_eq!(parsed.entries.len(), 1);
        let entry = &parsed.entries[0];
        assert_eq!(entry.session_id, "cline-session");
        assert_eq!(entry.project_path, "/work/project");
        assert_eq!(entry.model, "claude-sonnet-4");
        assert_eq!(entry.input_tokens, 60);
        assert_eq!(entry.output_tokens, 20);
        assert_eq!(entry.cache_read, 30);
        assert_eq!(entry.cache_creation, 10);
    }
}
