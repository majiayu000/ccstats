//! `OpenAI` Codex CLI JSONL parser
//!
//! Parses JSONL logs from ~/.codex/sessions/ directory.
//! Codex log format uses cumulative token counts that need delta computation.

use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::env;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use crate::consts::{DATE_FORMAT, UNKNOWN};
use crate::core::RawEntry;
use crate::source::ParseOutput;
use crate::utils::Timezone;

const DEFAULT_CODEX_DIR: &str = ".codex";
const CODEX_HOME_ENV: &str = "CODEX_HOME";
const SESSION_SUBDIR: &str = "sessions";

// ============================================================================
// Internal types for JSONL parsing
// ============================================================================

#[derive(Debug, Deserialize)]
struct RawJsonEntry<'a> {
    timestamp: Option<&'a str>,
    #[serde(rename = "type")]
    entry_type: Option<&'a str>,
    payload: Option<Payload<'a>>,
}

#[derive(Debug, Deserialize)]
#[allow(clippy::struct_field_names)] // field names match JSON schema
struct Payload<'a> {
    #[serde(rename = "type")]
    payload_type: Option<&'a str>,
    info: Option<TokenInfo<'a>>,
    model: Option<&'a str>,
}

#[derive(Debug, Deserialize)]
struct TokenInfo<'a> {
    total_token_usage: Option<TokenUsage>,
    last_token_usage: Option<TokenUsage>,
    model: Option<&'a str>,
    model_name: Option<&'a str>,
    metadata: Option<Metadata<'a>>,
}

#[derive(Debug, Deserialize)]
struct Metadata<'a> {
    model: Option<&'a str>,
}

#[derive(Debug, Deserialize, Clone, Default)]
#[allow(clippy::struct_field_names)] // field names match JSON schema
struct TokenUsage {
    input_tokens: Option<i64>,
    cached_input_tokens: Option<i64>,
    #[serde(alias = "cache_read_input_tokens")]
    alt_cache_read_input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    reasoning_output_tokens: Option<i64>,
    total_tokens: Option<i64>,
}

impl TokenUsage {
    fn cached_input(&self) -> i64 {
        self.cached_input_tokens
            .or(self.alt_cache_read_input_tokens)
            .unwrap_or(0)
    }

    #[cfg(test)]
    fn subtract(&self, prev: &TokenUsage) -> TokenUsage {
        TokenUsage {
            input_tokens: Some(
                (self.input_tokens.unwrap_or(0) - prev.input_tokens.unwrap_or(0)).max(0),
            ),
            cached_input_tokens: Some((self.cached_input() - prev.cached_input()).max(0)),
            alt_cache_read_input_tokens: None,
            output_tokens: Some(
                (self.output_tokens.unwrap_or(0) - prev.output_tokens.unwrap_or(0)).max(0),
            ),
            reasoning_output_tokens: Some(
                (self.reasoning_output_tokens.unwrap_or(0)
                    - prev.reasoning_output_tokens.unwrap_or(0))
                .max(0),
            ),
            total_tokens: Some(
                (self.total_tokens.unwrap_or(0) - prev.total_tokens.unwrap_or(0)).max(0),
            ),
        }
    }

    #[cfg(test)]
    fn is_empty(&self) -> bool {
        self.input_tokens.unwrap_or(0) == 0
            && self.cached_input() == 0
            && self.output_tokens.unwrap_or(0) == 0
            && self.reasoning_output_tokens.unwrap_or(0) == 0
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct UsageTotals {
    input_tokens: i64,
    cached_input_tokens: i64,
    output_tokens: i64,
    reasoning_output_tokens: i64,
    total_tokens: i64,
}

impl UsageTotals {
    fn from_usage(usage: &TokenUsage) -> Self {
        Self {
            input_tokens: usage.input_tokens.unwrap_or(0),
            cached_input_tokens: usage.cached_input(),
            output_tokens: usage.output_tokens.unwrap_or(0),
            reasoning_output_tokens: usage.reasoning_output_tokens.unwrap_or(0),
            total_tokens: usage.total_tokens.unwrap_or(0),
        }
    }

    fn subtract(self, prev: Self) -> Self {
        Self {
            input_tokens: (self.input_tokens - prev.input_tokens).max(0),
            cached_input_tokens: (self.cached_input_tokens - prev.cached_input_tokens).max(0),
            output_tokens: (self.output_tokens - prev.output_tokens).max(0),
            reasoning_output_tokens: (self.reasoning_output_tokens - prev.reasoning_output_tokens)
                .max(0),
            total_tokens: (self.total_tokens - prev.total_tokens).max(0),
        }
    }

    fn is_empty(self) -> bool {
        self.input_tokens == 0
            && self.cached_input_tokens == 0
            && self.output_tokens == 0
            && self.reasoning_output_tokens == 0
    }
}

// ============================================================================
// File discovery
// ============================================================================

fn get_codex_sessions_dir() -> Option<PathBuf> {
    // Check CODEX_HOME env var first
    if let Ok(codex_home) = env::var(CODEX_HOME_ENV) {
        let path = PathBuf::from(codex_home).join(SESSION_SUBDIR);
        if path.is_dir() {
            return Some(path);
        }
    }

    // Fall back to ~/.codex/sessions
    let home = dirs::home_dir()?;
    let path = home.join(DEFAULT_CODEX_DIR).join(SESSION_SUBDIR);
    if path.is_dir() { Some(path) } else { None }
}

pub(super) fn find_codex_files() -> Vec<PathBuf> {
    let Some(sessions_dir) = get_codex_sessions_dir() else {
        return Vec::new();
    };

    let mut files = Vec::new();
    if let Ok(entries) = glob::glob(&format!("{}/**/*.jsonl", sessions_dir.display())) {
        for entry in entries.flatten() {
            files.push(entry);
        }
    }
    files
}

// ============================================================================
// Parsing
// ============================================================================

fn estimate_entry_capacity(file: &File, approx_bytes_per_entry: u64) -> usize {
    let estimate = file
        .metadata()
        .ok()
        .map(|meta| meta.len() / approx_bytes_per_entry)
        .and_then(|n| usize::try_from(n).ok())
        .unwrap_or(0);
    estimate.saturating_add(1)
}

fn non_empty_model(model: Option<&str>) -> Option<&str> {
    model.and_then(|m| if m.trim().is_empty() { None } else { Some(m) })
}

fn extract_model_ref<'a>(payload: &'a Payload<'a>) -> Option<&'a str> {
    if let Some(info) = &payload.info
        && let Some(model) = non_empty_model(info.model.as_deref())
            .or_else(|| non_empty_model(info.model_name.as_deref()))
            .or_else(|| {
                non_empty_model(
                    info.metadata
                        .as_ref()
                        .and_then(|metadata| metadata.model.as_deref()),
                )
            })
    {
        return Some(model);
    }

    non_empty_model(payload.model.as_deref())
}

#[cfg(test)]
fn extract_model(payload: &Payload<'_>) -> Option<String> {
    extract_model_ref(payload).map(std::string::ToString::to_string)
}

#[allow(clippy::too_many_lines)]
pub(super) fn parse_codex_file_with_debug(
    path: &Path,
    timezone: Timezone,
    debug: bool,
) -> ParseOutput {
    let session_id = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(UNKNOWN)
        .to_string();

    let file = match File::open(path) {
        Ok(f) => f,
        Err(err) => {
            if debug {
                eprintln!("Failed to open {}: {}", path.display(), err);
            }
            return ParseOutput {
                entries: Vec::new(),
                errors: 1,
            };
        }
    };
    let estimated_capacity = estimate_entry_capacity(&file, 260);
    let reader = BufReader::new(file);

    let mut entries = Vec::with_capacity(estimated_capacity);
    let mut parse_errors = 0usize;
    let mut previous_totals: Option<UsageTotals> = None;
    let mut current_model: Option<String> = None;

    for (line_no, line) in reader.lines().enumerate() {
        let line = match line {
            Ok(line) => line,
            Err(err) => {
                if debug {
                    eprintln!(
                        "Failed to read line {} in {}: {}",
                        line_no + 1,
                        path.display(),
                        err
                    );
                }
                parse_errors += 1;
                continue;
            }
        };

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let raw_entry: RawJsonEntry<'_> = match serde_json::from_str(trimmed) {
            Ok(e) => e,
            Err(err) => {
                if debug {
                    eprintln!(
                        "Invalid JSON at {}:{}: {}",
                        path.display(),
                        line_no + 1,
                        err
                    );
                }
                parse_errors += 1;
                continue;
            }
        };

        let Some(entry_type) = raw_entry.entry_type else {
            continue;
        };

        // Handle turn_context to get model info
        if entry_type == "turn_context" {
            if let Some(payload) = &raw_entry.payload
                && let Some(model) = extract_model_ref(payload)
            {
                current_model = Some(model.to_string());
            }
            continue;
        }

        // Only process event_msg with token_count
        if entry_type != "event_msg" {
            continue;
        }

        let Some(payload) = &raw_entry.payload else {
            continue;
        };

        let Some(payload_type) = payload.payload_type else {
            continue;
        };

        if payload_type != "token_count" {
            continue;
        }

        let Some(timestamp) = &raw_entry.timestamp else {
            continue;
        };

        let Some(info) = &payload.info else { continue };

        // Get delta usage
        let Some(total) = &info.total_token_usage else {
            continue;
        };
        let total = UsageTotals::from_usage(total);

        // Skip if total hasn't changed (duplicate event)
        if let Some(prev) = &previous_totals
            && total.total_tokens == prev.total_tokens
        {
            continue;
        }

        // Use last_token_usage if available, otherwise compute delta
        let delta = if let Some(last) = &info.last_token_usage {
            UsageTotals::from_usage(last)
        } else {
            previous_totals.map_or(total, |prev| total.subtract(prev))
        };

        previous_totals = Some(total);

        if delta.is_empty() {
            continue;
        }

        // Parse timestamp
        let utc_dt = match timestamp.parse::<DateTime<Utc>>() {
            Ok(dt) => dt,
            Err(err) => {
                if debug {
                    eprintln!(
                        "Invalid timestamp at {}:{}: {} ({})",
                        path.display(),
                        line_no + 1,
                        timestamp,
                        err
                    );
                }
                parse_errors += 1;
                continue;
            }
        };
        let local_dt = timezone.to_fixed_offset(utc_dt);
        let date = local_dt.date_naive();

        // Get model name
        let model = if let Some(parsed_model) = extract_model_ref(payload) {
            let parsed_model = parsed_model.to_string();
            current_model = Some(parsed_model.clone());
            parsed_model
        } else {
            current_model.clone().unwrap_or_else(|| "gpt-5".to_string())
        };

        // Codex's input_tokens INCLUDES cached_input_tokens
        let raw_input = delta.input_tokens;
        let cached = delta.cached_input_tokens;
        let non_cached_input = (raw_input - cached).max(0);

        // OpenAI's output_tokens INCLUDES reasoning_output_tokens as a subset.
        // Separate them so total_tokens() and calculate_cost() don't double-count.
        let raw_output = delta.output_tokens;
        let reasoning = delta.reasoning_output_tokens;
        let non_reasoning_output = (raw_output - reasoning).max(0);

        entries.push(RawEntry {
            timestamp: timestamp.to_string(),
            timestamp_ms: utc_dt.timestamp_millis(),
            date_str: date.format(DATE_FORMAT).to_string(),
            message_id: None, // Codex doesn't use message IDs for dedup
            session_id: session_id.clone(),
            project_path: String::new(), // Codex doesn't track projects
            model,
            input_tokens: non_cached_input,
            output_tokens: non_reasoning_output,
            cache_creation: 0, // Codex doesn't have cache creation
            cache_read: cached,
            reasoning_tokens: reasoning,
            stop_reason: Some("complete".to_string()), // Codex events are always complete
        });
    }

    ParseOutput {
        entries,
        errors: parse_errors,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================================================
    // TokenUsage::subtract
    // ========================================================================

    #[test]
    fn test_subtract_normal() {
        let total = TokenUsage {
            input_tokens: Some(1000),
            cached_input_tokens: Some(200),
            alt_cache_read_input_tokens: None,
            output_tokens: Some(500),
            reasoning_output_tokens: Some(100),
            total_tokens: Some(1500),
        };
        let prev = TokenUsage {
            input_tokens: Some(400),
            cached_input_tokens: Some(100),
            alt_cache_read_input_tokens: None,
            output_tokens: Some(200),
            reasoning_output_tokens: Some(50),
            total_tokens: Some(600),
        };
        let delta = total.subtract(&prev);
        assert_eq!(delta.input_tokens, Some(600));
        assert_eq!(delta.cached_input_tokens, Some(100));
        assert_eq!(delta.output_tokens, Some(300));
        assert_eq!(delta.reasoning_output_tokens, Some(50));
        assert_eq!(delta.total_tokens, Some(900));
    }

    #[test]
    fn test_subtract_clamps_negative_to_zero() {
        let total = TokenUsage {
            input_tokens: Some(100),
            cached_input_tokens: Some(50),
            alt_cache_read_input_tokens: None,
            output_tokens: Some(10),
            reasoning_output_tokens: Some(0),
            total_tokens: Some(110),
        };
        let prev = TokenUsage {
            input_tokens: Some(500),
            cached_input_tokens: Some(200),
            alt_cache_read_input_tokens: None,
            output_tokens: Some(300),
            reasoning_output_tokens: Some(100),
            total_tokens: Some(800),
        };
        let delta = total.subtract(&prev);
        assert_eq!(delta.input_tokens, Some(0));
        assert_eq!(delta.cached_input_tokens, Some(0));
        assert_eq!(delta.output_tokens, Some(0));
        assert_eq!(delta.reasoning_output_tokens, Some(0));
        assert_eq!(delta.total_tokens, Some(0));
    }

    #[test]
    fn test_subtract_none_fields_treated_as_zero() {
        let total = TokenUsage {
            input_tokens: Some(100),
            ..Default::default()
        };
        let prev = TokenUsage::default();
        let delta = total.subtract(&prev);
        assert_eq!(delta.input_tokens, Some(100));
        assert_eq!(delta.output_tokens, Some(0));
        assert_eq!(delta.reasoning_output_tokens, Some(0));
    }

    // ========================================================================
    // TokenUsage::is_empty
    // ========================================================================

    #[test]
    fn test_is_empty_default() {
        assert!(TokenUsage::default().is_empty());
    }

    #[test]
    fn test_is_empty_with_input() {
        let usage = TokenUsage {
            input_tokens: Some(1),
            ..Default::default()
        };
        assert!(!usage.is_empty());
    }

    #[test]
    fn test_is_empty_with_cached_only() {
        let usage = TokenUsage {
            cached_input_tokens: Some(50),
            ..Default::default()
        };
        assert!(!usage.is_empty());
    }

    #[test]
    fn test_is_empty_with_reasoning_only() {
        let usage = TokenUsage {
            reasoning_output_tokens: Some(10),
            ..Default::default()
        };
        assert!(!usage.is_empty());
    }

    // ========================================================================
    // TokenUsage::cached_input (fallback logic)
    // ========================================================================

    #[test]
    fn test_cached_input_prefers_cached_input_tokens() {
        let usage = TokenUsage {
            cached_input_tokens: Some(100),
            alt_cache_read_input_tokens: Some(50),
            ..Default::default()
        };
        assert_eq!(usage.cached_input(), 100);
    }

    #[test]
    fn test_cached_input_falls_back_to_cache_read() {
        let usage = TokenUsage {
            cached_input_tokens: None,
            alt_cache_read_input_tokens: Some(75),
            ..Default::default()
        };
        assert_eq!(usage.cached_input(), 75);
    }

    #[test]
    fn test_cached_input_both_none_returns_zero() {
        let usage = TokenUsage::default();
        assert_eq!(usage.cached_input(), 0);
    }

    // ========================================================================
    // extract_model (priority chain)
    // ========================================================================

    #[test]
    fn test_extract_model_from_info_model() {
        let payload = Payload {
            payload_type: None,
            model: Some("fallback-model"),
            info: Some(TokenInfo {
                total_token_usage: None,
                last_token_usage: None,
                model: Some("info-model"),
                model_name: Some("info-model-name"),
                metadata: Some(Metadata {
                    model: Some("meta-model"),
                }),
            }),
        };
        assert_eq!(extract_model(&payload), Some("info-model".to_string()));
    }

    #[test]
    fn test_extract_model_falls_back_to_model_name() {
        let payload = Payload {
            payload_type: None,
            model: Some("fallback"),
            info: Some(TokenInfo {
                total_token_usage: None,
                last_token_usage: None,
                model: None,
                model_name: Some("model-name"),
                metadata: None,
            }),
        };
        assert_eq!(extract_model(&payload), Some("model-name".to_string()));
    }

    #[test]
    fn test_extract_model_falls_back_to_metadata() {
        let payload = Payload {
            payload_type: None,
            model: Some("fallback"),
            info: Some(TokenInfo {
                total_token_usage: None,
                last_token_usage: None,
                model: None,
                model_name: None,
                metadata: Some(Metadata {
                    model: Some("meta-model"),
                }),
            }),
        };
        assert_eq!(extract_model(&payload), Some("meta-model".to_string()));
    }

    #[test]
    fn test_extract_model_falls_back_to_payload_model() {
        let payload = Payload {
            payload_type: None,
            model: Some("payload-model"),
            info: Some(TokenInfo {
                total_token_usage: None,
                last_token_usage: None,
                model: None,
                model_name: None,
                metadata: None,
            }),
        };
        assert_eq!(extract_model(&payload), Some("payload-model".to_string()));
    }

    #[test]
    fn test_extract_model_no_info_uses_payload() {
        let payload = Payload {
            payload_type: None,
            model: Some("payload-only"),
            info: None,
        };
        assert_eq!(extract_model(&payload), Some("payload-only".to_string()));
    }

    #[test]
    fn test_extract_model_all_none_returns_none() {
        let payload = Payload {
            payload_type: None,
            model: None,
            info: None,
        };
        assert_eq!(extract_model(&payload), None);
    }

    #[test]
    fn test_extract_model_empty_strings_skipped() {
        let payload = Payload {
            payload_type: None,
            model: Some("real-model"),
            info: Some(TokenInfo {
                total_token_usage: None,
                last_token_usage: None,
                model: Some("  "),
                model_name: Some(""),
                metadata: None,
            }),
        };
        assert_eq!(extract_model(&payload), Some("real-model".to_string()));
    }
}
