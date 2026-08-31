//! Cursor usage-event parser
//!
//! Converts Admin API and dashboard usage events into `RawEntry` records.
//! Local `SQLite` state is ignored because current Cursor databases do not store
//! billed token counts.

use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, TimeZone, Utc};
use serde_json::Value;

use crate::consts::{DATE_FORMAT, UNKNOWN};
use crate::core::{DateFilter, RawEntry};
use crate::source::ParseOutput;
use crate::utils::Timezone;

use super::client::{fetch_usage_events, has_api_credentials};

pub(super) const USAGE_FILE_ENV: &str = "CURSOR_USAGE_FILE";
const API_SENTINEL: &str = "<cursor-usage-api>";
const CURSOR_MODEL: &str = "cursor";

pub(super) fn find_cursor_files(filter: &DateFilter, timezone: Timezone) -> Vec<PathBuf> {
    if let Ok(path) = env::var(USAGE_FILE_ENV)
        && !path.trim().is_empty()
    {
        return vec![PathBuf::from(path)];
    }

    if has_api_credentials() {
        let start_ms = filter
            .since
            .and_then(|date| timezone.date_start_utc_millis(date));
        let end_ms = filter.until.and_then(|date| {
            date.succ_opt()
                .and_then(|next| timezone.date_start_utc_millis(next))
                .map(|next_start| next_start.saturating_sub(1))
        });
        vec![PathBuf::from(format!(
            "{API_SENTINEL}|{}|{}",
            start_ms.map_or_else(String::new, |value| value.to_string()),
            end_ms.map_or_else(String::new, |value| value.to_string())
        ))]
    } else {
        Vec::new()
    }
}

pub(super) fn parse_cursor_with_debug(path: &Path, timezone: Timezone, debug: bool) -> ParseOutput {
    if path.to_string_lossy().starts_with(API_SENTINEL) {
        let sentinel = path.to_string_lossy();
        let mut parts = sentinel.split('|').skip(1);
        let start_ms = parts.next().and_then(|value| value.parse().ok());
        let end_ms = parts.next().and_then(|value| value.parse().ok());
        return match fetch_usage_events(start_ms, end_ms, debug) {
            Ok(events) => entries_from_events(&events, timezone),
            Err(err) => {
                eprintln!("Error: {err}");
                ParseOutput {
                    entries: Vec::new(),
                    errors: 1,
                }
            }
        };
    }

    match fs::read_to_string(path) {
        Ok(text) => match serde_json::from_str::<Value>(&text) {
            Ok(payload) => parse_usage_payload(&payload, timezone),
            Err(err) => {
                if debug {
                    eprintln!(
                        "Failed to parse Cursor usage file {}: {}",
                        path.display(),
                        err
                    );
                }
                ParseOutput {
                    entries: Vec::new(),
                    errors: 1,
                }
            }
        },
        Err(err) => {
            if debug {
                eprintln!(
                    "Failed to read Cursor usage file {}: {}",
                    path.display(),
                    err
                );
            }
            ParseOutput {
                entries: Vec::new(),
                errors: 1,
            }
        }
    }
}

pub(super) fn events_from_payload(value: &Value) -> Option<Vec<&Value>> {
    if let Some(items) = value.as_array() {
        return Some(items.iter().collect());
    }

    for key in ["usageEventsDisplay", "usageEvents", "events"] {
        if let Some(items) = value.get(key).and_then(Value::as_array) {
            return Some(items.iter().collect());
        }
    }

    None
}

fn parse_usage_payload(payload: &Value, timezone: Timezone) -> ParseOutput {
    let Some(events) = events_from_payload(payload) else {
        return ParseOutput {
            entries: Vec::new(),
            errors: 1,
        };
    };
    let events: Vec<Value> = events.into_iter().cloned().collect();
    entries_from_events(&events, timezone)
}

fn entries_from_events(events: &[Value], timezone: Timezone) -> ParseOutput {
    let mut entries = Vec::new();
    let mut seen = HashSet::new();
    let mut errors = 0usize;

    for (ordinal, event) in events.iter().enumerate() {
        if !event.is_object() {
            errors += 1;
            continue;
        }
        let Some(entry) = entry_from_event(event, ordinal, timezone) else {
            continue;
        };
        if !seen.insert(entry.message_id.clone().unwrap_or_default()) {
            continue;
        }
        entries.push(entry);
    }

    ParseOutput { entries, errors }
}

fn entry_from_event(event: &Value, ordinal: usize, timezone: Timezone) -> Option<RawEntry> {
    let utc_dt = timestamp_from_value(event)?;
    let local_dt = timezone.to_fixed_offset(utc_dt);
    let (input_tokens, output_tokens, cache_creation, cache_read) = token_counts(event);
    let recorded_cost_usd =
        float_at(event, &["chargedCents"]).map(|cents| (cents / 100.0).max(0.0));
    if input_tokens == 0
        && output_tokens == 0
        && cache_creation == 0
        && cache_read == 0
        && recorded_cost_usd.unwrap_or(0.0) <= 0.0
    {
        return None;
    }

    let model = first_string(event, &[&["model"], &["modelName"]])
        .unwrap_or_else(|| CURSOR_MODEL.to_string());
    let session_id = first_string(
        event,
        &[&["conversationId"], &["generationUUID"], &["generationId"]],
    )
    .unwrap_or_else(|| UNKNOWN.to_string());
    let message_id = first_string(
        event,
        &[
            &["usageEventId"],
            &["requestId"],
            &["id"],
            &["generationUUID"],
            &["generationId"],
        ],
    )
    .map_or_else(|| format!("event:{ordinal}"), |id| format!("event:{id}"));

    Some(RawEntry {
        timestamp: utc_dt.to_rfc3339(),
        timestamp_ms: utc_dt.timestamp_millis(),
        date_str: local_dt.date_naive().format(DATE_FORMAT).to_string(),
        message_id: Some(message_id),
        session_key: format!("cursor:{session_id}"),
        session_id,
        project_path: String::new(),
        model,
        input_tokens,
        output_tokens,
        cache_creation,
        cache_creation_1h: 0,
        cache_read,
        reasoning_tokens: 0,
        stop_reason: Some("complete".to_string()),
        cost_kind: crate::core::CostKind::Real,
        endpoint: crate::core::Endpoint::Unknown,
        call_count: 1,
        reported_total_tokens: None,
        recorded_cost_usd,
    })
}

fn token_counts(event: &Value) -> (i64, i64, i64, i64) {
    let input = token_count_at(
        event,
        &[
            &["tokenUsage", "inputTokens"],
            &["tokenUsage", "input_tokens"],
            &["tokenCount", "inputTokens"],
            &["inputTokens"],
        ],
    );
    let output = token_count_at(
        event,
        &[
            &["tokenUsage", "outputTokens"],
            &["tokenUsage", "output_tokens"],
            &["tokenCount", "outputTokens"],
            &["outputTokens"],
        ],
    );
    let cache_creation = token_count_at(
        event,
        &[
            &["tokenUsage", "cacheWriteTokens"],
            &["tokenUsage", "cache_write_tokens"],
            &["cacheWriteTokens"],
        ],
    );
    let cache_read = token_count_at(
        event,
        &[
            &["tokenUsage", "cacheReadTokens"],
            &["tokenUsage", "cache_read_tokens"],
            &["cacheReadTokens"],
        ],
    );
    (input, output, cache_creation, cache_read)
}

fn token_count_at(value: &Value, paths: &[&[&str]]) -> i64 {
    paths
        .iter()
        .find_map(|path| integer_at(value, path))
        .unwrap_or(0)
        .max(0)
}

fn timestamp_from_value(value: &Value) -> Option<DateTime<Utc>> {
    if let Some(timestamp) = first_string(value, &[&["timestamp"], &["createdAt"], &["time"]]) {
        if let Ok(parsed) = timestamp.parse::<DateTime<Utc>>() {
            return Some(parsed);
        }
        if let Ok(millis) = timestamp.parse::<i64>() {
            return Utc.timestamp_millis_opt(millis).single();
        }
    }

    let millis = integer_at(value, &["timestamp"])
        .or_else(|| integer_at(value, &["createdAt"]))
        .or_else(|| integer_at(value, &["unixMs"]))
        .or_else(|| integer_at(value, &["timestampMs"]))?;
    Utc.timestamp_millis_opt(millis).single()
}

fn integer_at(value: &Value, path: &[&str]) -> Option<i64> {
    let mut current = value;
    for part in path {
        current = current.get(*part)?;
    }
    current
        .as_i64()
        .or_else(|| current.as_u64().and_then(|n| i64::try_from(n).ok()))
        .or_else(|| current.as_f64().map(|n| n as i64))
}

fn float_at(value: &Value, path: &[&str]) -> Option<f64> {
    let mut current = value;
    for part in path {
        current = current.get(*part)?;
    }
    current
        .as_f64()
        .or_else(|| current.as_i64().map(|n| n as f64))
        .or_else(|| current.as_u64().map(|n| n as f64))
}

fn string_at<'a>(value: &'a Value, path: &[&str]) -> Option<&'a str> {
    let mut current = value;
    for part in path {
        current = current.get(*part)?;
    }
    current.as_str()
}

fn first_string(value: &Value, paths: &[&[&str]]) -> Option<String> {
    paths
        .iter()
        .find_map(|path| string_at(value, path))
        .filter(|s| !s.trim().is_empty())
        .map(std::string::ToString::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tz() -> Timezone {
        Timezone::parse(Some("UTC")).unwrap()
    }

    #[test]
    fn parse_dashboard_usage_events_display() {
        let payload = json!({
            "totalUsageEventsCount": 1,
            "usageEventsDisplay": [{
                "timestamp": "1770372000000",
                "model": "claude-4-sonnet",
                "conversationId": "composer-1",
                "tokenUsage": {
                    "inputTokens": 100,
                    "outputTokens": 40,
                    "cacheWriteTokens": 8,
                    "cacheReadTokens": 12
                },
                "chargedCents": 12.5
            }]
        });
        let parsed = parse_usage_payload(&payload, tz());
        assert_eq!(parsed.errors, 0);
        assert_eq!(parsed.entries.len(), 1);
        let entry = &parsed.entries[0];
        assert_eq!(entry.session_id, "composer-1");
        assert_eq!(entry.model, "claude-4-sonnet");
        assert_eq!(entry.input_tokens, 100);
        assert_eq!(entry.output_tokens, 40);
        assert_eq!(entry.cache_creation, 8);
        assert_eq!(entry.cache_read, 12);
        assert_eq!(entry.recorded_cost_usd, Some(0.125));
    }

    #[test]
    fn parse_admin_usage_events_array() {
        let payload = json!({
            "usageEvents": [{
                "timestamp": "2026-02-06T10:00:00Z",
                "model": "composer-2.5",
                "conversationId": "conv-1",
                "tokenUsage": {
                    "inputTokens": 25,
                    "outputTokens": 10,
                    "cacheWriteTokens": 0,
                    "cacheReadTokens": 5
                },
                "chargedCents": 0
            }]
        });
        let parsed = parse_usage_payload(&payload, tz());
        assert_eq!(parsed.entries.len(), 1);
        assert_eq!(parsed.entries[0].date_str, "2026-02-06");
        assert_eq!(parsed.entries[0].recorded_cost_usd, Some(0.0));
        assert_eq!(parsed.entries[0].cache_read, 5);
    }

    #[test]
    fn parse_bare_event_array() {
        let payload = json!([{
            "timestamp": 1_770_372_000_000_i64,
            "model": "grok-4.6",
            "tokenUsage": {"inputTokens": 3, "outputTokens": 7}
        }]);
        let parsed = parse_usage_payload(&payload, tz());
        assert_eq!(parsed.entries.len(), 1);
        assert_eq!(parsed.entries[0].model, "grok-4.6");
        assert_eq!(parsed.entries[0].input_tokens, 3);
        assert_eq!(parsed.entries[0].output_tokens, 7);
    }

    #[test]
    fn skips_zero_token_events_without_cost() {
        let payload = json!([{
            "timestamp": "2026-02-06T10:00:00Z",
            "model": "claude-4-sonnet",
            "tokenUsage": {"inputTokens": 0, "outputTokens": 0}
        }]);
        let parsed = parse_usage_payload(&payload, tz());
        assert!(parsed.entries.is_empty());
    }

    #[test]
    fn keeps_zero_token_events_with_recorded_cost() {
        let payload = json!([{
            "timestamp": "2026-02-06T10:00:00Z",
            "model": "claude-4-sonnet",
            "chargedCents": 8
        }]);
        let parsed = parse_usage_payload(&payload, tz());
        assert_eq!(parsed.entries.len(), 1);
        assert_eq!(parsed.entries[0].recorded_cost_usd, Some(0.08));
        assert_eq!(parsed.entries[0].input_tokens, 0);
    }

    #[test]
    fn clamps_negative_token_counts() {
        let payload = json!([{
            "timestamp": "2026-02-06T10:00:00Z",
            "model": "gpt-4o-mini",
            "tokenUsage": {"inputTokens": -25, "outputTokens": 40}
        }]);
        let parsed = parse_usage_payload(&payload, tz());
        assert_eq!(parsed.entries.len(), 1);
        assert_eq!(parsed.entries[0].input_tokens, 0);
        assert_eq!(parsed.entries[0].output_tokens, 40);
    }

    #[test]
    fn skips_all_negative_token_counts_without_cost() {
        let payload = json!([{
            "timestamp": "2026-02-06T10:00:00Z",
            "model": "gpt-4o-mini",
            "tokenUsage": {"inputTokens": -7, "outputTokens": -3}
        }]);
        let parsed = parse_usage_payload(&payload, tz());
        assert!(parsed.entries.is_empty());
    }

    #[test]
    fn counts_malformed_non_object_events() {
        let payload = json!(["not-an-event", {
            "timestamp": "2026-02-06T10:00:00Z",
            "model": "claude-4-sonnet",
            "tokenUsage": {"inputTokens": 1, "outputTokens": 1}
        }]);
        let parsed = parse_usage_payload(&payload, tz());
        assert_eq!(parsed.errors, 1);
        assert_eq!(parsed.entries.len(), 1);
    }

    #[test]
    fn preserves_identical_events_without_provider_ids() {
        let event = json!({
            "timestamp": "2026-02-06T10:00:00Z",
            "model": "claude-4-sonnet",
            "conversationId": "composer-1",
            "tokenUsage": {"inputTokens": 10, "outputTokens": 4}
        });
        let payload = json!([event.clone(), event]);
        let parsed = parse_usage_payload(&payload, tz());
        assert_eq!(parsed.entries.len(), 2);
    }

    #[test]
    fn deduplicates_repeated_provider_event_ids() {
        let event = json!({
            "id": "event-1",
            "timestamp": "2026-02-06T10:00:00Z",
            "model": "claude-4-sonnet",
            "tokenUsage": {"inputTokens": 10, "outputTokens": 4}
        });
        let payload = json!([event.clone(), event]);
        let parsed = parse_usage_payload(&payload, tz());
        assert_eq!(parsed.entries.len(), 1);
    }

    #[test]
    fn rejects_unsupported_usage_file_schema() {
        let parsed = parse_usage_payload(&json!({"unexpected": []}), tz());
        assert!(parsed.entries.is_empty());
        assert_eq!(parsed.errors, 1);
    }
}
