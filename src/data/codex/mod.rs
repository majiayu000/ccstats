//! Codex data loader
//!
//! Parses JSONL logs from ~/.codex/sessions/ directory.
//! Codex log format is different from Claude Code:
//! - Uses `event_msg` with `payload.type: "token_count"`
//! - Token usage is cumulative, needs to be delta-computed
//! - Has reasoning_output_tokens field

use chrono::{DateTime, NaiveDate, Utc};
use rayon::prelude::*;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::env;

use crate::data::types::{DayStats, SessionStats, Stats};
use crate::utils::Timezone;

/// Default Codex directory
const DEFAULT_CODEX_DIR: &str = ".codex";
const CODEX_HOME_ENV: &str = "CODEX_HOME";
const SESSION_SUBDIR: &str = "sessions";

/// Raw JSONL entry structure
#[derive(Debug, Deserialize)]
struct RawEntry {
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
            input_tokens: Some((self.input_tokens.unwrap_or(0) - prev.input_tokens.unwrap_or(0)).max(0)),
            cached_input_tokens: Some((self.cached_input() - prev.cached_input()).max(0)),
            _cache_read_input_tokens: None,
            output_tokens: Some((self.output_tokens.unwrap_or(0) - prev.output_tokens.unwrap_or(0)).max(0)),
            reasoning_output_tokens: Some(
                (self.reasoning_output_tokens.unwrap_or(0) - prev.reasoning_output_tokens.unwrap_or(0)).max(0),
            ),
            total_tokens: Some((self.total_tokens.unwrap_or(0) - prev.total_tokens.unwrap_or(0)).max(0)),
        }
    }

    fn is_empty(&self) -> bool {
        self.input_tokens.unwrap_or(0) == 0
            && self.cached_input() == 0
            && self.output_tokens.unwrap_or(0) == 0
            && self.reasoning_output_tokens.unwrap_or(0) == 0
    }
}

/// Parsed token event
#[derive(Debug, Clone)]
struct TokenEvent {
    timestamp: String,
    timestamp_ms: i64,
    date_str: String,
    model: String,
    session_id: String,
    usage: TokenUsage,
}

/// Convert TokenUsage to Stats
///
/// Note: Codex's `input_tokens` INCLUDES `cached_input_tokens`, so we need to
/// subtract cached from total input to get the non-cached input portion.
fn stats_from_token_usage(usage: &TokenUsage) -> Stats {
    let raw_input = usage.input_tokens.unwrap_or(0);
    let cached = usage.cached_input();
    // Non-cached input = total input - cached input
    let non_cached_input = (raw_input - cached).max(0);

    Stats {
        input_tokens: non_cached_input,
        output_tokens: usage.output_tokens.unwrap_or(0),
        cache_creation: 0, // Codex doesn't have cache creation
        cache_read: cached,
        reasoning_tokens: usage.reasoning_output_tokens.unwrap_or(0),
        count: 1,
        skipped_chunks: 0,
    }
}

fn add_event_to_day_stats(day_stats: &mut HashMap<String, DayStats>, event: &TokenEvent) {
    let stats = stats_from_token_usage(&event.usage);
    let day = day_stats.entry(event.date_str.clone()).or_default();
    day.stats.add(&stats);
    day.models.entry(event.model.clone()).or_default().add(&stats);
}

/// Find Codex sessions directory
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
    if path.is_dir() {
        Some(path)
    } else {
        None
    }
}

/// Find all JSONL files in Codex sessions directory
pub fn find_codex_jsonl_files() -> Vec<PathBuf> {
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

/// Extract model name from various locations in the payload
fn extract_model(payload: &Payload) -> Option<String> {
    // Try payload.info.model first
    if let Some(info) = &payload.info {
        if let Some(model) = &info.model {
            if !model.trim().is_empty() {
                return Some(model.clone());
            }
        }
        if let Some(model) = &info.model_name {
            if !model.trim().is_empty() {
                return Some(model.clone());
            }
        }
        if let Some(metadata) = &info.metadata {
            if let Some(model) = &metadata.model {
                if !model.trim().is_empty() {
                    return Some(model.clone());
                }
            }
        }
    }

    // Try payload.model
    if let Some(model) = &payload.model {
        if !model.trim().is_empty() {
            return Some(model.clone());
        }
    }

    None
}

/// Parse a single JSONL file and extract token events
fn parse_codex_file(
    path: &PathBuf,
    since: Option<NaiveDate>,
    until: Option<NaiveDate>,
    timezone: &Timezone,
) -> Vec<TokenEvent> {
    let session_id = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();

    let file = match File::open(path) {
        Ok(f) => f,
        Err(_) => return Vec::new(),
    };
    let reader = BufReader::new(file);

    let mut events = Vec::new();
    let mut previous_totals: Option<TokenUsage> = None;
    let mut current_model: Option<String> = None;

    for line in reader.lines().flatten() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let entry: RawEntry = match serde_json::from_str(trimmed) {
            Ok(e) => e,
            Err(_) => continue,
        };

        let entry_type = match &entry.entry_type {
            Some(t) => t.as_str(),
            None => continue,
        };

        // Handle turn_context to get model info
        if entry_type == "turn_context" {
            if let Some(payload) = &entry.payload {
                if let Some(model) = extract_model(payload) {
                    current_model = Some(model);
                }
            }
            continue;
        }

        // Only process event_msg with token_count
        if entry_type != "event_msg" {
            continue;
        }

        let payload = match &entry.payload {
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

        let timestamp = match &entry.timestamp {
            Some(ts) => ts.clone(),
            None => continue,
        };

        let info = match &payload.info {
            Some(i) => i,
            None => continue,
        };

        // Get delta usage - must check if total changed to avoid duplicates
        // Codex emits multiple events with same total_token_usage, we only count when total changes
        let total = match &info.total_token_usage {
            Some(t) => t,
            None => continue,
        };

        // Skip if total hasn't changed (duplicate event)
        if let Some(prev) = &previous_totals {
            if total.total_tokens == prev.total_tokens {
                continue;
            }
        }

        // Use last_token_usage if available, otherwise compute delta from totals
        let delta = if let Some(last) = &info.last_token_usage {
            last.clone()
        } else {
            match &previous_totals {
                Some(prev) => total.subtract(prev),
                None => total.clone(),
            }
        };

        previous_totals = Some(total.clone());

        // Skip empty events
        if delta.is_empty() {
            continue;
        }

        // Parse timestamp
        let utc_dt = match timestamp.parse::<DateTime<Utc>>() {
            Ok(dt) => dt,
            Err(_) => continue,
        };
        let local_dt = timezone.to_fixed_offset(utc_dt);
        let date = local_dt.date_naive();

        // Date filtering
        if let Some(s) = since {
            if date < s {
                continue;
            }
        }
        if let Some(u) = until {
            if date > u {
                continue;
            }
        }

        // Get model name
        let model = extract_model(payload)
            .or_else(|| current_model.clone())
            .unwrap_or_else(|| "gpt-5".to_string()); // Fallback for legacy logs

        if let Some(m) = extract_model(payload) {
            current_model = Some(m);
        }

        events.push(TokenEvent {
            timestamp,
            timestamp_ms: utc_dt.timestamp_millis(),
            date_str: date.format("%Y-%m-%d").to_string(),
            model,
            session_id: session_id.clone(),
            usage: delta,
        });
    }

    events
}

/// Load Codex usage data aggregated by day
pub fn load_codex_usage_data(
    since: Option<NaiveDate>,
    until: Option<NaiveDate>,
    quiet: bool,
    timezone: &Timezone,
) -> (HashMap<String, DayStats>, i64, i64) {
    if !quiet {
        eprintln!("Scanning Codex JSONL files...");
    }

    let files = find_codex_jsonl_files();

    if files.is_empty() {
        if !quiet {
            eprintln!("No Codex session files found in ~/.codex/sessions/");
        }
        return (HashMap::new(), 0, 0);
    }

    if !quiet {
        eprintln!("Found {} Codex session files", files.len());
        eprintln!("Processing...");
    }

    // Parse all files in parallel
    let all_events: Vec<TokenEvent> = files
        .par_iter()
        .flat_map(|path| parse_codex_file(path, since, until, timezone))
        .collect();

    let valid = all_events.len() as i64;

    // Aggregate into day stats
    let mut day_stats: HashMap<String, DayStats> = HashMap::new();
    for event in &all_events {
        add_event_to_day_stats(&mut day_stats, event);
    }

    if !quiet {
        eprintln!("Found {} token events across {} days", valid, day_stats.len());
    }

    (day_stats, 0, valid)
}

/// Session accumulator for Codex
#[derive(Debug, Default)]
struct SessionAccumulator {
    session_id: String,
    first_timestamp: String,
    last_timestamp: String,
    first_timestamp_ms: i64,
    last_timestamp_ms: i64,
    stats: Stats,
    models: HashMap<String, Stats>,
}

impl SessionAccumulator {
    fn new(session_id: String, timestamp: &str, timestamp_ms: i64) -> Self {
        SessionAccumulator {
            session_id,
            first_timestamp: timestamp.to_string(),
            last_timestamp: timestamp.to_string(),
            first_timestamp_ms: timestamp_ms,
            last_timestamp_ms: timestamp_ms,
            stats: Stats::default(),
            models: HashMap::new(),
        }
    }

    fn add_event(&mut self, event: &TokenEvent) {
        let stats = stats_from_token_usage(&event.usage);
        self.stats.add(&stats);
        self.models.entry(event.model.clone()).or_default().add(&stats);

        if event.timestamp_ms < self.first_timestamp_ms {
            self.first_timestamp = event.timestamp.clone();
            self.first_timestamp_ms = event.timestamp_ms;
        }
        if event.timestamp_ms > self.last_timestamp_ms {
            self.last_timestamp = event.timestamp.clone();
            self.last_timestamp_ms = event.timestamp_ms;
        }
    }
}

impl From<SessionAccumulator> for SessionStats {
    fn from(acc: SessionAccumulator) -> Self {
        SessionStats {
            session_id: acc.session_id,
            project_path: String::new(), // Codex doesn't have project paths
            first_timestamp: acc.first_timestamp,
            last_timestamp: acc.last_timestamp,
            stats: acc.stats,
            models: acc.models,
        }
    }
}

/// Load Codex session data
pub fn load_codex_session_data(
    since: Option<NaiveDate>,
    until: Option<NaiveDate>,
    quiet: bool,
    timezone: &Timezone,
) -> Vec<SessionStats> {
    if !quiet {
        eprintln!("Scanning Codex JSONL files...");
    }

    let files = find_codex_jsonl_files();

    if files.is_empty() {
        if !quiet {
            eprintln!("No Codex session files found in ~/.codex/sessions/");
        }
        return Vec::new();
    }

    if !quiet {
        eprintln!("Found {} Codex session files", files.len());
        eprintln!("Processing sessions...");
    }

    // Parse all files in parallel
    let all_events: Vec<TokenEvent> = files
        .par_iter()
        .flat_map(|path| parse_codex_file(path, since, until, timezone))
        .collect();

    // Aggregate into sessions
    let mut sessions: HashMap<String, SessionAccumulator> = HashMap::new();
    for event in &all_events {
        let session = sessions
            .entry(event.session_id.clone())
            .or_insert_with(|| {
                SessionAccumulator::new(
                    event.session_id.clone(),
                    &event.timestamp,
                    event.timestamp_ms,
                )
            });
        session.add_event(event);
    }

    let result: Vec<SessionStats> = sessions.into_values().map(SessionStats::from).collect();

    if !quiet {
        eprintln!("Found {} sessions with data", result.len());
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_usage_subtract() {
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
    fn test_token_usage_is_empty() {
        let empty = TokenUsage::default();
        assert!(empty.is_empty());

        let non_empty = TokenUsage {
            input_tokens: Some(100),
            ..Default::default()
        };
        assert!(!non_empty.is_empty());
    }

    #[test]
    fn test_stats_from_token_usage() {
        // Codex's input_tokens INCLUDES cached_input_tokens
        // So: raw_input=1000, cached=200, non_cached=800
        let usage = TokenUsage {
            input_tokens: Some(1000),
            cached_input_tokens: Some(200),
            _cache_read_input_tokens: None,
            output_tokens: Some(500),
            reasoning_output_tokens: Some(100),
            total_tokens: Some(1500),
        };

        let stats = stats_from_token_usage(&usage);

        // input_tokens should be non-cached portion (1000 - 200 = 800)
        assert_eq!(stats.input_tokens, 800);
        assert_eq!(stats.output_tokens, 500);
        assert_eq!(stats.cache_read, 200);
        assert_eq!(stats.cache_creation, 0);
        assert_eq!(stats.reasoning_tokens, 100);
        assert_eq!(stats.count, 1);
        // Total should be 800 + 500 + 200 = 1500 (matches original total)
        assert_eq!(stats.total_tokens(), 1500);
    }
}
