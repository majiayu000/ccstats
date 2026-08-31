//! Shared parser for Cline-family VS Code extension task logs.

use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::Value;

use crate::consts::{DATE_FORMAT, UNKNOWN};
use crate::core::{CostKind, Endpoint, RawEntry};
use crate::source::{Capabilities, ParseOutput, Source};
use crate::utils::Timezone;

const ROO_EXTENSION_ID: &str = "rooveterinaryinc.roo-cline";
const KILO_EXTENSION_ID: &str = "kilocode.kilo-code";
pub(super) const CLINE_EXTENSION_ID: &str = "saoudrizwan.claude-dev";

pub(crate) struct RooCodeSource;
pub(crate) struct KiloCodeSource;

impl RooCodeSource {
    pub(crate) const fn new() -> Self {
        Self
    }
}

impl KiloCodeSource {
    pub(crate) const fn new() -> Self {
        Self
    }
}

fn extension_capabilities() -> Capabilities {
    Capabilities {
        has_projects: false,
        has_billing_blocks: false,
        has_reasoning_tokens: false,
        has_cache_creation: true,
        has_cache_read: true,
        needs_dedup: false,
        has_tool_calls: false,
        has_endpoints: false,
    }
}

impl Source for RooCodeSource {
    fn name(&self) -> &'static str {
        "roocode"
    }

    fn display_name(&self) -> &'static str {
        "Roo Code"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["roo"]
    }

    fn capabilities(&self) -> Capabilities {
        extension_capabilities()
    }

    fn find_files(&self) -> Vec<PathBuf> {
        find_extension_files(ROO_EXTENSION_ID)
    }

    fn parse_file(&self, path: &Path, timezone: Timezone, debug: bool) -> ParseOutput {
        parse_extension_file(path, self.name(), timezone, debug)
    }
}

impl Source for KiloCodeSource {
    fn name(&self) -> &'static str {
        "kilocode"
    }

    fn display_name(&self) -> &'static str {
        "Kilo Code"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["kc"]
    }

    fn capabilities(&self) -> Capabilities {
        extension_capabilities()
    }

    fn find_files(&self) -> Vec<PathBuf> {
        find_extension_files(KILO_EXTENSION_ID)
    }

    fn parse_file(&self, path: &Path, timezone: Timezone, debug: bool) -> ParseOutput {
        parse_extension_file(path, self.name(), timezone, debug)
    }
}

fn extension_task_roots(extension_id: &str) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(config) = dirs::config_dir() {
        roots.push(
            config
                .join("Code/User/globalStorage")
                .join(extension_id)
                .join("tasks"),
        );
    }
    if let Some(home) = dirs::home_dir() {
        roots.push(
            home.join(".config/Code/User/globalStorage")
                .join(extension_id)
                .join("tasks"),
        );
        roots.push(
            home.join(".vscode-server/data/User/globalStorage")
                .join(extension_id)
                .join("tasks"),
        );
        #[cfg(target_os = "windows")]
        roots.push(
            home.join("AppData/Roaming/Code/User/globalStorage")
                .join(extension_id)
                .join("tasks"),
        );
    }
    roots.sort();
    roots.dedup();
    roots
}

pub(super) fn find_extension_files(extension_id: &str) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for root in extension_task_roots(extension_id) {
        let pattern = root.join("*").join("ui_messages.json");
        if let Ok(matches) = glob::glob(&pattern.to_string_lossy()) {
            files.extend(matches.flatten().filter(|path| path.is_file()));
        }
    }
    files.sort();
    files.dedup();
    files
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct UiMessage {
    #[serde(rename = "type")]
    kind: Option<String>,
    say: Option<String>,
    text: Option<String>,
    ts: Option<Value>,
    #[serde(rename = "modelInfo")]
    model_info: Option<UiModelInfo>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct UiModelInfo {
    model_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TaskHistoryEntry {
    id: String,
    cwd_on_task_initialization: Option<String>,
}

fn value_i64(value: Option<&Value>) -> Option<i64> {
    value.and_then(|value| {
        value
            .as_i64()
            .or_else(|| value.as_u64().and_then(|number| i64::try_from(number).ok()))
            .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
    })
}

fn parse_timestamp(value: Option<&Value>) -> Option<i64> {
    let value = value?;
    if let Some(text) = value.as_str() {
        if let Ok(timestamp) = DateTime::parse_from_rfc3339(text) {
            return Some(timestamp.timestamp_millis());
        }
        let number = text.parse::<i64>().ok()?;
        return numeric_timestamp(number);
    }
    value_i64(Some(value)).and_then(numeric_timestamp)
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

fn task_project(path: &Path, task_id: &str) -> String {
    let Some(extension_root) = path.parent().and_then(Path::parent).and_then(Path::parent) else {
        return String::new();
    };
    let Ok(content) = fs::read_to_string(extension_root.join("state/taskHistory.json")) else {
        return String::new();
    };
    serde_json::from_str::<Vec<TaskHistoryEntry>>(&content)
        .ok()
        .and_then(|entries| entries.into_iter().find(|entry| entry.id == task_id))
        .and_then(|entry| entry.cwd_on_task_initialization)
        .map(|project| project.trim().to_string())
        .filter(|project| !project.is_empty())
        .unwrap_or_default()
}

fn session_id(path: &Path) -> String {
    path.parent()
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or(UNKNOWN)
        .to_string()
}

fn request_model(model_info: Option<UiModelInfo>) -> Option<String> {
    model_info
        .and_then(|info| info.model_id)
        .map(|model| model.trim().to_string())
        .filter(|model| !model.is_empty())
}

fn usage_token_buckets(payload: &Value) -> Result<Option<(i64, i64, i64, i64)>, &'static str> {
    let buckets = (
        value_i64(payload.get("tokensIn")).unwrap_or(0),
        value_i64(payload.get("tokensOut")).unwrap_or(0),
        value_i64(payload.get("cacheReads")).unwrap_or(0),
        value_i64(payload.get("cacheWrites")).unwrap_or(0),
    );
    if [buckets.0, buckets.1, buckets.2, buckets.3]
        .into_iter()
        .any(|tokens| tokens < 0)
    {
        return Err("negative token bucket");
    }
    Ok((buckets != (0, 0, 0, 0)).then_some(buckets))
}

pub(super) fn parse_extension_file(
    path: &Path,
    source: &str,
    timezone: Timezone,
    debug: bool,
) -> ParseOutput {
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) => {
            if debug {
                eprintln!("Failed to read {}: {error}", path.display());
            }
            return ParseOutput {
                entries: Vec::new(),
                errors: 1,
            };
        }
    };
    let messages: Vec<UiMessage> = match serde_json::from_str(&content) {
        Ok(messages) => messages,
        Err(error) => {
            if debug {
                eprintln!("Invalid JSON in {}: {error}", path.display());
            }
            return ParseOutput {
                entries: Vec::new(),
                errors: 1,
            };
        }
    };
    let session_id = session_id(path);
    let project_path = task_project(path, &session_id);
    let mut output = ParseOutput::default();
    for message in messages {
        if message.kind.as_deref() != Some("say")
            || message.say.as_deref() != Some("api_req_started")
        {
            continue;
        }
        let Some(model) = request_model(message.model_info) else {
            output.errors += 1;
            continue;
        };
        let Some(text) = message.text else {
            output.errors += 1;
            continue;
        };
        let payload: Value = match serde_json::from_str(&text) {
            Ok(payload) => payload,
            Err(error) => {
                output.errors += 1;
                if debug {
                    eprintln!("Invalid usage payload in {}: {error}", path.display());
                }
                continue;
            }
        };
        let Some(timestamp_ms) = parse_timestamp(message.ts.as_ref()) else {
            output.errors += 1;
            continue;
        };
        let Some(utc) = DateTime::<Utc>::from_timestamp_millis(timestamp_ms) else {
            output.errors += 1;
            continue;
        };
        let (input_tokens, output_tokens, cache_read, cache_creation) =
            match usage_token_buckets(&payload) {
                Ok(Some(tokens)) => tokens,
                Ok(None) => continue,
                Err(_) => {
                    output.errors += 1;
                    continue;
                }
            };
        output.entries.push(RawEntry {
            timestamp: utc.to_rfc3339(),
            timestamp_ms,
            date_str: timezone
                .to_fixed_offset(utc)
                .date_naive()
                .format(DATE_FORMAT)
                .to_string(),
            message_id: None,
            session_key: format!("{source}:{}", path.parent().unwrap_or(path).display()),
            session_id: session_id.clone(),
            project_path: project_path.clone(),
            model,
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
            reported_total_tokens: None,
            recorded_cost_usd: None,
        });
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    use tempfile::tempdir;

    #[test]
    fn api_request_event_uses_task_metadata_and_token_buckets() {
        let temp = tempdir().unwrap();
        let task = temp.path().join("tasks/task-1");
        fs::create_dir_all(&task).unwrap();
        let path = task.join("ui_messages.json");
        fs::write(
            &path,
            r#"[{"type":"say","say":"api_req_started","ts":"2026-08-31T03:04:05Z","modelInfo":{"modelId":"claude-sonnet-4"},"text":"{\"tokensIn\":100,\"tokensOut\":20,\"cacheReads\":30,\"cacheWrites\":5,\"apiProtocol\":\"anthropic\"}"}]"#,
        )
        .unwrap();
        fs::write(
            task.join("api_conversation_history.json"),
            "<environment_details><model>claude-sonnet-4</model></environment_details>",
        )
        .unwrap();

        let parsed = parse_extension_file(&path, "roocode", Timezone::Named(chrono_tz::UTC), false);

        assert_eq!(parsed.errors, 0);
        assert_eq!(parsed.entries.len(), 1);
        let entry = &parsed.entries[0];
        assert_eq!(entry.session_id, "task-1");
        assert_eq!(entry.model, "claude-sonnet-4");
        assert_eq!(entry.input_tokens, 100);
        assert_eq!(entry.output_tokens, 20);
        assert_eq!(entry.cache_read, 30);
        assert_eq!(entry.cache_creation, 5);
    }

    #[test]
    fn api_request_events_preserve_each_request_model() {
        let temp = tempdir().unwrap();
        let task = temp.path().join("tasks/task-1");
        fs::create_dir_all(&task).unwrap();
        let path = task.join("ui_messages.json");
        fs::write(
            &path,
            r#"[
                {"type":"say","say":"api_req_started","ts":"2026-08-31T03:04:05Z","modelInfo":{"modelId":"claude-sonnet-4"},"text":"{\"tokensIn\":10}"},
                {"type":"say","say":"api_req_started","ts":"2026-08-31T03:05:05Z","modelInfo":{"modelId":"gpt-5"},"text":"{\"tokensIn\":20}"}
            ]"#,
        )
        .unwrap();
        fs::write(
            task.join("api_conversation_history.json"),
            "<environment_details><model>gpt-5</model></environment_details>",
        )
        .unwrap();

        let parsed = parse_extension_file(&path, "cline", Timezone::Named(chrono_tz::UTC), false);

        assert_eq!(parsed.entries.len(), 2);
        assert_eq!(parsed.entries[0].model, "claude-sonnet-4");
        assert_eq!(parsed.entries[1].model, "gpt-5");
    }

    #[test]
    fn api_request_without_request_local_model_is_rejected() {
        let temp = tempdir().unwrap();
        let task = temp.path().join("tasks/task-1");
        fs::create_dir_all(&task).unwrap();
        let path = task.join("ui_messages.json");
        fs::write(
            &path,
            r#"[{"type":"say","say":"api_req_started","ts":"2026-08-31T03:04:05Z","text":"{\"tokensIn\":10}"}]"#,
        )
        .unwrap();
        fs::write(
            task.join("api_conversation_history.json"),
            "<environment_details><model>final-task-model</model></environment_details>",
        )
        .unwrap();

        let parsed = parse_extension_file(&path, "cline", Timezone::Named(chrono_tz::UTC), false);

        assert!(parsed.entries.is_empty());
        assert_eq!(parsed.errors, 1);
    }

    #[test]
    fn api_request_with_negative_token_bucket_is_rejected() {
        let temp = tempdir().unwrap();
        let task = temp.path().join("tasks/task-1");
        fs::create_dir_all(&task).unwrap();
        let path = task.join("ui_messages.json");
        fs::write(
            &path,
            r#"[{"type":"say","say":"api_req_started","ts":"2026-08-31T03:04:05Z","modelInfo":{"modelId":"gpt-5"},"text":"{\"tokensIn\":-1,\"tokensOut\":20}"}]"#,
        )
        .unwrap();

        let parsed = parse_extension_file(&path, "cline", Timezone::Named(chrono_tz::UTC), false);

        assert!(parsed.entries.is_empty());
        assert_eq!(parsed.errors, 1);
    }

    #[test]
    fn extension_entries_use_task_history_project() {
        let temp = tempdir().unwrap();
        let extension = temp.path().join("extension");
        let task = extension.join("tasks/task-1");
        fs::create_dir_all(&task).unwrap();
        fs::create_dir_all(extension.join("state")).unwrap();
        let path = task.join("ui_messages.json");
        fs::write(
            &path,
            r#"[{"type":"say","say":"api_req_started","ts":"2026-08-31T03:04:05Z","modelInfo":{"modelId":"gpt-5"},"text":"{\"tokensIn\":10}"}]"#,
        )
        .unwrap();
        fs::write(
            extension.join("state/taskHistory.json"),
            r#"[{"id":"task-1","cwdOnTaskInitialization":"/work/project"}]"#,
        )
        .unwrap();

        let parsed = parse_extension_file(&path, "cline", Timezone::Named(chrono_tz::UTC), false);

        assert_eq!(parsed.entries.len(), 1);
        assert_eq!(parsed.entries[0].project_path, "/work/project");
    }
}
