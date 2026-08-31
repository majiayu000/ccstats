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

fn extract_tag(block: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = block.find(&open)? + open.len();
    let rest = &block[start..];
    let end = rest.find(&close)?;
    let value = rest[..end].trim();
    (!value.is_empty()).then(|| value.to_string())
}

fn task_model(path: &Path) -> String {
    const START: &str = "<environment_details>";
    const END: &str = "</environment_details>";

    let history = path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("api_conversation_history.json");
    let Ok(content) = fs::read_to_string(history) else {
        return UNKNOWN.to_string();
    };
    let mut offset = 0;
    let mut model = None;
    while let Some(relative_start) = content[offset..].find(START) {
        let start = offset + relative_start + START.len();
        let Some(relative_end) = content[start..].find(END) else {
            break;
        };
        let end = start + relative_end;
        if let Some(found) = extract_tag(&content[start..end], "model") {
            model = Some(found);
        }
        offset = end + END.len();
    }
    model.unwrap_or_else(|| UNKNOWN.to_string())
}

fn session_id(path: &Path) -> String {
    path.parent()
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or(UNKNOWN)
        .to_string()
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
    let model = task_model(path);
    let mut output = ParseOutput::default();
    for message in messages {
        if message.kind.as_deref() != Some("say")
            || message.say.as_deref() != Some("api_req_started")
        {
            continue;
        }
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
        let input_tokens = value_i64(payload.get("tokensIn")).unwrap_or(0).max(0);
        let output_tokens = value_i64(payload.get("tokensOut")).unwrap_or(0).max(0);
        let cache_read = value_i64(payload.get("cacheReads")).unwrap_or(0).max(0);
        let cache_creation = value_i64(payload.get("cacheWrites")).unwrap_or(0).max(0);
        if input_tokens == 0 && output_tokens == 0 && cache_read == 0 && cache_creation == 0 {
            continue;
        }
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
            project_path: String::new(),
            model: model.clone(),
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
            r#"[{"type":"say","say":"api_req_started","ts":"2026-08-31T03:04:05Z","text":"{\"tokensIn\":100,\"tokensOut\":20,\"cacheReads\":30,\"cacheWrites\":5,\"apiProtocol\":\"anthropic\"}"}]"#,
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
}
