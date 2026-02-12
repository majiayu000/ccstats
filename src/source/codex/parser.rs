//! OpenAI Codex CLI JSONL parser
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
use crate::utils::{Timezone, parse_debug_enabled};

const DEFAULT_CODEX_DIR: &str = ".codex";
const CODEX_HOME_ENV: &str = "CODEX_HOME";
const SESSION_SUBDIR: &str = "sessions";

// ============================================================================
// Internal types for JSONL parsing
// ============================================================================

#[derive(Debug, Deserialize)]
struct RawJsonEntry {
    timestamp: Option<String>,
    #[serde(rename = "type")]
    entry_type: Option<String>,
    payload: Option<Payload>,
}

#[derive(Debug, Deserialize)]
struct Payload {
    #[serde(rename = "type")]
    payload_type: Option<String>,
    info: Option<TokenInfo>,
    model: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TokenInfo {
    total_token_usage: Option<TokenUsage>,
    last_token_usage: Option<TokenUsage>,
    model: Option<String>,
    model_name: Option<String>,
    metadata: Option<Metadata>,
}

#[derive(Debug, Deserialize)]
struct Metadata {
    model: Option<String>,
}

#[derive(Debug, Deserialize, Clone, Default)]
struct TokenUsage {
    input_tokens: Option<i64>,
    cached_input_tokens: Option<i64>,
    #[serde(alias = "cache_read_input_tokens")]
    _cache_read_input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    reasoning_output_tokens: Option<i64>,
    total_tokens: Option<i64>,
}

impl TokenUsage {
    fn cached_input(&self) -> i64 {
        self.cached_input_tokens
            .or(self._cache_read_input_tokens)
            .unwrap_or(0)
    }

    fn subtract(&self, prev: &TokenUsage) -> TokenUsage {
        TokenUsage {
            input_tokens: Some(
                (self.input_tokens.unwrap_or(0) - prev.input_tokens.unwrap_or(0)).max(0),
            ),
            cached_input_tokens: Some((self.cached_input() - prev.cached_input()).max(0)),
            _cache_read_input_tokens: None,
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

    fn is_empty(&self) -> bool {
        self.input_tokens.unwrap_or(0) == 0
            && self.cached_input() == 0
            && self.output_tokens.unwrap_or(0) == 0
            && self.reasoning_output_tokens.unwrap_or(0) == 0
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

fn extract_model(payload: &Payload) -> Option<String> {
    let non_empty = |model: Option<&String>| {
        model
            .filter(|m| !m.trim().is_empty())
            .map(std::string::ToString::to_string)
    };

    if let Some(info) = &payload.info
        && let Some(model) = non_empty(info.model.as_ref())
            .or_else(|| non_empty(info.model_name.as_ref()))
            .or_else(|| non_empty(info.metadata.as_ref().and_then(|m| m.model.as_ref())))
    {
        return Some(model);
    }

    non_empty(payload.model.as_ref())
}

pub(super) fn parse_codex_file(path: &Path, timezone: &Timezone) -> Vec<RawEntry> {
    let session_id = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(UNKNOWN)
        .to_string();

    let file = match File::open(path) {
        Ok(f) => f,
        Err(err) => {
            if parse_debug_enabled() {
                eprintln!("Failed to open {}: {}", path.display(), err);
            }
            return Vec::new();
        }
    };
    let reader = BufReader::new(file);

    let mut entries = Vec::new();
    let mut previous_totals: Option<TokenUsage> = None;
    let mut current_model: Option<String> = None;

    for (line_no, line) in reader.lines().enumerate() {
        let line = match line {
            Ok(line) => line,
            Err(err) => {
                if parse_debug_enabled() {
                    eprintln!(
                        "Failed to read line {} in {}: {}",
                        line_no + 1,
                        path.display(),
                        err
                    );
                }
                continue;
            }
        };

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let raw_entry: RawJsonEntry = match serde_json::from_str(trimmed) {
            Ok(e) => e,
            Err(err) => {
                if parse_debug_enabled() {
                    eprintln!(
                        "Invalid JSON at {}:{}: {}",
                        path.display(),
                        line_no + 1,
                        err
                    );
                }
                continue;
            }
        };

        let entry_type = match &raw_entry.entry_type {
            Some(t) => t.as_str(),
            None => continue,
        };

        // Handle turn_context to get model info
        if entry_type == "turn_context" {
            if let Some(payload) = &raw_entry.payload
                && let Some(model) = extract_model(payload)
            {
                current_model = Some(model);
            }
            continue;
        }

        // Only process event_msg with token_count
        if entry_type != "event_msg" {
            continue;
        }

        let payload = match &raw_entry.payload {
            Some(p) => p,
            None => continue,
        };

        let payload_type = match &payload.payload_type {
            Some(t) => t.as_str(),
            None => continue,
        };

        if payload_type != "token_count" {
            continue;
        }

        let timestamp = match &raw_entry.timestamp {
            Some(ts) => ts.clone(),
            None => continue,
        };

        let info = match &payload.info {
            Some(i) => i,
            None => continue,
        };

        // Get delta usage
        let total = match &info.total_token_usage {
            Some(t) => t,
            None => continue,
        };

        // Skip if total hasn't changed (duplicate event)
        if let Some(prev) = &previous_totals
            && total.total_tokens == prev.total_tokens
        {
            continue;
        }

        // Use last_token_usage if available, otherwise compute delta
        let delta = if let Some(last) = &info.last_token_usage {
            last.clone()
        } else {
            match &previous_totals {
                Some(prev) => total.subtract(prev),
                None => total.clone(),
            }
        };

        previous_totals = Some(total.clone());

        if delta.is_empty() {
            continue;
        }

        // Parse timestamp
        let utc_dt = match timestamp.parse::<DateTime<Utc>>() {
            Ok(dt) => dt,
            Err(err) => {
                if parse_debug_enabled() {
                    eprintln!(
                        "Invalid timestamp at {}:{}: {} ({})",
                        path.display(),
                        line_no + 1,
                        timestamp,
                        err
                    );
                }
                continue;
            }
        };
        let local_dt = timezone.to_fixed_offset(utc_dt);
        let date = local_dt.date_naive();

        // Get model name
        let model = extract_model(payload)
            .or_else(|| current_model.clone())
            .unwrap_or_else(|| "gpt-5".to_string());

        if let Some(m) = extract_model(payload) {
            current_model = Some(m);
        }

        // Codex's input_tokens INCLUDES cached_input_tokens
        let raw_input = delta.input_tokens.unwrap_or(0);
        let cached = delta.cached_input();
        let non_cached_input = (raw_input - cached).max(0);

        // OpenAI's output_tokens INCLUDES reasoning_output_tokens as a subset.
        // Separate them so total_tokens() and calculate_cost() don't double-count.
        let raw_output = delta.output_tokens.unwrap_or(0);
        let reasoning = delta.reasoning_output_tokens.unwrap_or(0);
        let non_reasoning_output = (raw_output - reasoning).max(0);

        entries.push(RawEntry {
            timestamp,
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

    entries
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
            _cache_read_input_tokens: None,
            output_tokens: Some(500),
            reasoning_output_tokens: Some(100),
            total_tokens: Some(1500),
        };
        let prev = TokenUsage {
            input_tokens: Some(400),
            cached_input_tokens: Some(100),
            _cache_read_input_tokens: None,
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
            _cache_read_input_tokens: None,
            output_tokens: Some(10),
            reasoning_output_tokens: Some(0),
            total_tokens: Some(110),
        };
        let prev = TokenUsage {
            input_tokens: Some(500),
            cached_input_tokens: Some(200),
            _cache_read_input_tokens: None,
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
            _cache_read_input_tokens: Some(50),
            ..Default::default()
        };
        assert_eq!(usage.cached_input(), 100);
    }

    #[test]
    fn test_cached_input_falls_back_to_cache_read() {
        let usage = TokenUsage {
            cached_input_tokens: None,
            _cache_read_input_tokens: Some(75),
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
            model: Some("fallback-model".to_string()),
            info: Some(TokenInfo {
                total_token_usage: None,
                last_token_usage: None,
                model: Some("info-model".to_string()),
                model_name: Some("info-model-name".to_string()),
                metadata: Some(Metadata {
                    model: Some("meta-model".to_string()),
                }),
            }),
        };
        assert_eq!(extract_model(&payload), Some("info-model".to_string()));
    }

    #[test]
    fn test_extract_model_falls_back_to_model_name() {
        let payload = Payload {
            payload_type: None,
            model: Some("fallback".to_string()),
            info: Some(TokenInfo {
                total_token_usage: None,
                last_token_usage: None,
                model: None,
                model_name: Some("model-name".to_string()),
                metadata: None,
            }),
        };
        assert_eq!(extract_model(&payload), Some("model-name".to_string()));
    }

    #[test]
    fn test_extract_model_falls_back_to_metadata() {
        let payload = Payload {
            payload_type: None,
            model: Some("fallback".to_string()),
            info: Some(TokenInfo {
                total_token_usage: None,
                last_token_usage: None,
                model: None,
                model_name: None,
                metadata: Some(Metadata {
                    model: Some("meta-model".to_string()),
                }),
            }),
        };
        assert_eq!(extract_model(&payload), Some("meta-model".to_string()));
    }

    #[test]
    fn test_extract_model_falls_back_to_payload_model() {
        let payload = Payload {
            payload_type: None,
            model: Some("payload-model".to_string()),
            info: Some(TokenInfo {
                total_token_usage: None,
                last_token_usage: None,
                model: None,
                model_name: None,
                metadata: None,
            }),
        };
        assert_eq!(
            extract_model(&payload),
            Some("payload-model".to_string())
        );
    }

    #[test]
    fn test_extract_model_no_info_uses_payload() {
        let payload = Payload {
            payload_type: None,
            model: Some("payload-only".to_string()),
            info: None,
        };
        assert_eq!(
            extract_model(&payload),
            Some("payload-only".to_string())
        );
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
            model: Some("real-model".to_string()),
            info: Some(TokenInfo {
                total_token_usage: None,
                last_token_usage: None,
                model: Some("  ".to_string()),
                model_name: Some("".to_string()),
                metadata: None,
            }),
        };
        assert_eq!(extract_model(&payload), Some("real-model".to_string()));
    }
}
