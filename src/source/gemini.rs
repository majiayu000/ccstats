//! Gemini CLI local usage source.

use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use chrono::{DateTime, NaiveDateTime, Utc};
use serde_json::Value;

use crate::consts::{DATE_FORMAT, UNKNOWN};
use crate::core::{CostKind, Endpoint, RawEntry};
use crate::source::{Capabilities, ParseOutput, Source};
use crate::utils::Timezone;

const GEMINI_HOME_ENV: &str = "GEMINI_CLI_HOME";
const DEFAULT_GEMINI_HOME: &str = ".gemini";

pub(crate) struct GeminiSource;

impl GeminiSource {
    pub(crate) const fn new() -> Self {
        Self
    }
}

impl Source for GeminiSource {
    fn name(&self) -> &'static str {
        "gemini"
    }

    fn display_name(&self) -> &'static str {
        "Gemini CLI"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["gm"]
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            has_projects: false,
            has_billing_blocks: false,
            has_reasoning_tokens: true,
            has_cache_creation: false,
            has_cache_read: true,
            needs_dedup: false,
            has_tool_calls: false,
            has_endpoints: false,
        }
    }

    fn find_files(&self) -> Vec<PathBuf> {
        find_gemini_files()
    }

    fn parse_file(&self, path: &Path, timezone: Timezone, debug: bool) -> ParseOutput {
        parse_gemini_file(path, timezone, debug)
    }
}

fn gemini_home() -> Option<PathBuf> {
    match env::var(GEMINI_HOME_ENV) {
        Ok(value) if !value.trim().is_empty() => Some(PathBuf::from(value)),
        Ok(_) | Err(_) => dirs::home_dir().map(|home| home.join(DEFAULT_GEMINI_HOME)),
    }
}

fn find_gemini_files() -> Vec<PathBuf> {
    let Some(tmp) = gemini_home().map(|root| root.join("tmp")) else {
        return Vec::new();
    };
    if !tmp.is_dir() {
        return Vec::new();
    }

    let patterns = [
        tmp.join("*").join("chats").join("*.json"),
        tmp.join("**").join("*.jsonl"),
    ];
    let mut files = Vec::new();
    for pattern in patterns {
        if let Ok(matches) = glob::glob(&pattern.to_string_lossy()) {
            files.extend(matches.flatten().filter(|path| path.is_file()));
        }
    }
    files.sort();
    files.dedup();
    files
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

fn first_i64(value: &Value, keys: &[&str]) -> Option<i64> {
    keys.iter().find_map(|key| value_i64(value.get(*key)))
}

fn non_empty_string(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn parse_timestamp_text(value: &str) -> Option<i64> {
    if let Ok(timestamp) = DateTime::parse_from_rfc3339(value) {
        return Some(timestamp.timestamp_millis());
    }
    for format in [
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%d %H:%M:%S",
    ] {
        if let Ok(timestamp) = NaiveDateTime::parse_from_str(value, format) {
            return Some(timestamp.and_utc().timestamp_millis());
        }
    }
    value.parse::<i64>().ok().and_then(parse_numeric_timestamp)
}

fn parse_numeric_timestamp(value: i64) -> Option<i64> {
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
    value
        .as_str()
        .and_then(parse_timestamp_text)
        .or_else(|| value_i64(Some(value)).and_then(parse_numeric_timestamp))
}

#[derive(Clone, Copy, Debug, Default)]
struct GeminiTokens {
    input: i64,
    output: i64,
    cached: i64,
    reasoning: i64,
    tool: i64,
}

fn session_tokens(value: &Value) -> GeminiTokens {
    GeminiTokens {
        input: first_i64(
            value,
            &[
                "input",
                "prompt",
                "input_tokens",
                "prompt_tokens",
                "promptTokenCount",
            ],
        )
        .unwrap_or(0),
        output: first_i64(
            value,
            &[
                "output",
                "candidates",
                "output_tokens",
                "completion_tokens",
                "candidatesTokenCount",
            ],
        )
        .unwrap_or(0),
        cached: first_i64(
            value,
            &["cached", "cached_tokens", "cachedContentTokenCount"],
        )
        .unwrap_or(0),
        reasoning: first_i64(value, &["thoughts", "reasoning", "thoughts_tokens"]).unwrap_or(0),
        tool: first_i64(value, &["tool", "tool_tokens"]).unwrap_or(0),
    }
}

fn normalized_session_input(tokens: &GeminiTokens) -> (i64, i64) {
    let input = tokens.input.max(0);
    let cached = tokens.cached.max(0);
    (input.saturating_sub(cached.min(input)), cached)
}

fn build_entry(
    path: &Path,
    timezone: Timezone,
    session_id: &str,
    model: String,
    timestamp_ms: i64,
    tokens: GeminiTokens,
    message_id: Option<String>,
) -> Option<RawEntry> {
    let utc = DateTime::<Utc>::from_timestamp_millis(timestamp_ms)?;
    let (input, cache_read) = normalized_session_input(&tokens);
    let input_tokens = input.saturating_add(tokens.tool.max(0));
    let output_tokens = tokens.output.max(0);
    let reasoning_tokens = tokens.reasoning.max(0);
    if input_tokens == 0 && output_tokens == 0 && cache_read == 0 && reasoning_tokens == 0 {
        return None;
    }
    let session_id = if session_id.trim().is_empty() {
        UNKNOWN
    } else {
        session_id
    };
    Some(RawEntry {
        timestamp: utc.to_rfc3339(),
        timestamp_ms,
        date_str: timezone
            .to_fixed_offset(utc)
            .date_naive()
            .format(DATE_FORMAT)
            .to_string(),
        message_id,
        session_key: format!("{}::{session_id}", path.display()),
        session_id: session_id.to_string(),
        project_path: String::new(),
        model,
        input_tokens,
        output_tokens,
        cache_creation: 0,
        cache_creation_1h: 0,
        cache_read,
        reasoning_tokens,
        stop_reason: None,
        cost_kind: CostKind::Real,
        endpoint: Endpoint::Unknown,
        call_count: 1,
        recorded_cost_usd: None,
        api_equivalent_priced_tokens: 0,
        api_equivalent_coverage_tokens: 0,
    })
}

fn fallback_session_id(path: &Path) -> String {
    path.file_stem()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or(UNKNOWN)
        .to_string()
}

fn parse_session_json(
    path: &Path,
    value: &Value,
    timezone: Timezone,
    fallback_timestamp: Option<i64>,
    errors: &mut usize,
) -> Vec<RawEntry> {
    let session_id = non_empty_string(value.get("sessionId").or_else(|| value.get("session_id")))
        .unwrap_or_else(|| fallback_session_id(path));
    let Some(messages) = value.get("messages").and_then(Value::as_array) else {
        return Vec::new();
    };

    messages
        .iter()
        .filter_map(|message| {
            let model = non_empty_string(message.get("model"))?;
            let token_value = message.get("tokens")?;
            let timestamp_ms = parse_timestamp(message.get("timestamp")).or(fallback_timestamp);
            let Some(timestamp_ms) = timestamp_ms else {
                *errors += 1;
                return None;
            };
            let message_id =
                non_empty_string(message.get("id")).map(|id| format!("gemini:{session_id}:{id}"));
            build_entry(
                path,
                timezone,
                &session_id,
                model,
                timestamp_ms,
                session_tokens(token_value),
                message_id,
            )
        })
        .collect()
}

struct HeadlessUsage {
    model: String,
    input: i64,
    output: i64,
    cached: i64,
    reasoning: i64,
    input_includes_cache: bool,
}

fn headless_usage(model: String, value: &Value) -> Option<HeadlessUsage> {
    let has_wrapper = value.get("tokens").is_some();
    let tokens = value.get("tokens").unwrap_or(value);
    let prompt_input = first_i64(tokens, &["prompt", "input_tokens", "prompt_tokens"]);
    let net_input = first_i64(tokens, &["input"]);
    let wrapper_input = has_wrapper.then_some(net_input).flatten();
    let input = prompt_input.or(wrapper_input).or(net_input).unwrap_or(0);
    let output = first_i64(
        tokens,
        &["candidates", "output", "output_tokens", "candidates_tokens"],
    )
    .unwrap_or(0);
    let cached = first_i64(tokens, &["cached", "cached_tokens"]).unwrap_or(0);
    let reasoning = first_i64(
        tokens,
        &[
            "thoughts",
            "thoughts_tokens",
            "reasoning",
            "reasoning_tokens",
        ],
    )
    .unwrap_or(0);
    if input == 0 && output == 0 && cached == 0 && reasoning == 0 {
        return None;
    }
    Some(HeadlessUsage {
        model,
        input,
        output,
        cached,
        reasoning,
        input_includes_cache: prompt_input.is_some() || wrapper_input.is_some(),
    })
}

fn entries_from_stats(
    path: &Path,
    stats: &Value,
    timezone: Timezone,
    session_id: &str,
    model_hint: Option<String>,
    timestamp_ms: i64,
) -> Vec<RawEntry> {
    let usages = stats
        .get("models")
        .and_then(Value::as_object)
        .map(|models| {
            models
                .iter()
                .filter_map(|(model, value)| headless_usage(model.clone(), value))
                .collect::<Vec<_>>()
        })
        .filter(|usages| !usages.is_empty())
        .unwrap_or_else(|| {
            headless_usage(model_hint.unwrap_or_else(|| UNKNOWN.to_string()), stats)
                .into_iter()
                .collect()
        });

    usages
        .into_iter()
        .filter_map(|usage| {
            let (input, cached) = if usage.input_includes_cache {
                let input = usage.input.max(0);
                let cached = usage.cached.max(0);
                (input, cached)
            } else {
                let input = usage.input.max(0);
                let cached = usage.cached.max(0);
                (input.saturating_add(cached), cached)
            };
            build_entry(
                path,
                timezone,
                session_id,
                usage.model,
                timestamp_ms,
                GeminiTokens {
                    input,
                    output: usage.output,
                    cached,
                    reasoning: usage.reasoning,
                    ..GeminiTokens::default()
                },
                None,
            )
        })
        .collect()
}

fn parse_headless_value(
    path: &Path,
    value: &Value,
    timezone: Timezone,
    session_id: &str,
    model_hint: Option<String>,
    fallback_timestamp: Option<i64>,
    errors: &mut usize,
) -> Vec<RawEntry> {
    let is_direct_usage = value.get("type").and_then(Value::as_str) == Some("gemini");
    let stats = value
        .get("stats")
        .or_else(|| value.get("result").and_then(|result| result.get("stats")));
    if !is_direct_usage && stats.is_none() {
        return Vec::new();
    }

    let timestamp_ms = parse_timestamp(value.get("timestamp").or_else(|| value.get("created_at")))
        .or(fallback_timestamp);
    let Some(timestamp_ms) = timestamp_ms else {
        *errors += 1;
        return Vec::new();
    };

    if is_direct_usage {
        let Some(tokens) = value.get("tokens") else {
            return Vec::new();
        };
        let Some(model) = non_empty_string(value.get("model")).or(model_hint) else {
            return Vec::new();
        };
        let id = non_empty_string(value.get("id")).map(|id| format!("gemini:{session_id}:{id}"));
        return build_entry(
            path,
            timezone,
            session_id,
            model,
            timestamp_ms,
            session_tokens(tokens),
            id,
        )
        .into_iter()
        .collect();
    }

    stats.map_or_else(Vec::new, |stats| {
        entries_from_stats(path, stats, timezone, session_id, model_hint, timestamp_ms)
    })
}

fn parse_jsonl(
    path: &Path,
    timezone: Timezone,
    fallback_timestamp: Option<i64>,
    debug: bool,
) -> ParseOutput {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) => {
            if debug {
                eprintln!("Failed to open {}: {error}", path.display());
            }
            return ParseOutput {
                entries: Vec::new(),
                errors: 1,
            };
        }
    };
    let mut output = ParseOutput::default();
    let mut session_id = fallback_session_id(path);
    let mut current_model = None;
    let mut direct_indices = HashMap::<String, usize>::new();

    for (line_index, line) in BufReader::new(file).lines().enumerate() {
        let line = match line {
            Ok(line) => line,
            Err(error) => {
                output.errors += 1;
                if debug {
                    eprintln!(
                        "Failed to read {} line {}: {error}",
                        path.display(),
                        line_index + 1
                    );
                }
                continue;
            }
        };
        if line.trim().is_empty() {
            continue;
        }
        let value: Value = match serde_json::from_str(&line) {
            Ok(value) => value,
            Err(error) => {
                output.errors += 1;
                if debug {
                    eprintln!(
                        "Invalid JSON in {} line {}: {error}",
                        path.display(),
                        line_index + 1
                    );
                }
                continue;
            }
        };
        let event_type = value.get("type").and_then(Value::as_str);
        if event_type == Some("init") {
            if let Some(model) = non_empty_string(value.get("model")) {
                current_model = Some(model);
            }
            if let Some(id) =
                non_empty_string(value.get("session_id").or_else(|| value.get("sessionId")))
            {
                session_id = id;
            }
            continue;
        }
        if let Some(id) =
            non_empty_string(value.get("session_id").or_else(|| value.get("sessionId")))
        {
            session_id = id;
        }
        if let Some(model) = non_empty_string(value.get("model")) {
            current_model = Some(model);
        }
        let mut entries = parse_headless_value(
            path,
            &value,
            timezone,
            &session_id,
            current_model.clone(),
            fallback_timestamp,
            &mut output.errors,
        );
        if event_type == Some("gemini")
            && let Some(id) = non_empty_string(value.get("id"))
            && let Some(entry) = entries.pop()
        {
            if let Some(index) = direct_indices.get(&id).copied() {
                output.entries[index] = entry;
            } else {
                direct_indices.insert(id, output.entries.len());
                output.entries.push(entry);
            }
        } else {
            output.entries.extend(entries);
        }
    }
    output
}

fn parse_gemini_file(path: &Path, timezone: Timezone, debug: bool) -> ParseOutput {
    let fallback_timestamp = file_modified_ms(path);
    if path.extension().and_then(|extension| extension.to_str()) == Some("jsonl") {
        return parse_jsonl(path, timezone, fallback_timestamp, debug);
    }

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
    let value: Value = match serde_json::from_str(&content) {
        Ok(value) => value,
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
    let mut errors = 0;
    let entries = if value.get("messages").and_then(Value::as_array).is_some() {
        parse_session_json(path, &value, timezone, fallback_timestamp, &mut errors)
    } else {
        let session_id =
            non_empty_string(value.get("sessionId").or_else(|| value.get("session_id")))
                .unwrap_or_else(|| fallback_session_id(path));
        let model = non_empty_string(value.get("model"));
        parse_headless_value(
            path,
            &value,
            timezone,
            &session_id,
            model,
            fallback_timestamp,
            &mut errors,
        )
    };
    ParseOutput { entries, errors }
}

#[cfg(test)]
mod tests {
    use super::*;

    use tempfile::tempdir;

    #[test]
    fn session_json_normalizes_cache_inclusive_input() {
        let temp = tempdir().unwrap();
        let chats = temp.path().join("tmp/project/chats");
        fs::create_dir_all(&chats).unwrap();
        let path = chats.join("session.json");
        fs::write(
            &path,
            r#"{
                "sessionId": "gemini-session",
                "messages": [{
                    "id": "message-1",
                    "timestamp": "2026-08-31T03:04:05Z",
                    "model": "gemini-2.5-pro",
                    "tokens": {
                        "input": 1000,
                        "output": 50,
                        "cached": 200,
                        "thoughts": 20,
                        "tool": 5,
                        "total": 1075
                    }
                }]
            }"#,
        )
        .unwrap();

        let parsed = parse_gemini_file(&path, Timezone::Named(chrono_tz::UTC), false);

        assert_eq!(parsed.errors, 0);
        assert_eq!(parsed.entries.len(), 1);
        let entry = &parsed.entries[0];
        assert_eq!(entry.session_id, "gemini-session");
        assert_eq!(entry.model, "gemini-2.5-pro");
        assert_eq!(entry.input_tokens, 805);
        assert_eq!(entry.output_tokens, 50);
        assert_eq!(entry.cache_read, 200);
        assert_eq!(entry.reasoning_tokens, 20);
        assert_eq!(entry.date_str, "2026-08-31");
    }

    #[test]
    fn session_json_normalizes_cached_input_without_total() {
        let tokens = GeminiTokens {
            input: 1_000,
            output: 50,
            cached: 200,
            reasoning: 20,
            tool: 5,
        };

        assert_eq!(normalized_session_input(&tokens), (800, 200));
    }

    #[test]
    fn headless_cache_inclusive_input_is_normalized_once() {
        let mut errors = 0;
        let entries = parse_headless_value(
            Path::new("event.jsonl"),
            &serde_json::json!({
                "timestamp": "2026-08-31T03:04:05Z",
                "stats": {
                    "tokens": {"input": 100, "output": 5, "cached": 20}
                }
            }),
            Timezone::Named(chrono_tz::UTC),
            "session-1",
            Some("gemini-2.5-pro".to_string()),
            None,
            &mut errors,
        );

        assert_eq!(errors, 0);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].input_tokens, 80);
        assert_eq!(entries[0].cache_read, 20);
    }

    #[test]
    fn unrelated_headless_event_without_timestamp_is_ignored() {
        let mut errors = 0;
        let entries = parse_headless_value(
            Path::new("event.jsonl"),
            &serde_json::json!({"type": "tool"}),
            Timezone::Named(chrono_tz::UTC),
            "session-1",
            None,
            None,
            &mut errors,
        );

        assert!(entries.is_empty());
        assert_eq!(errors, 0);
    }
}
