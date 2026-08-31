//! Per-turn Grok usage from `updates.jsonl`.
//!
//! `turn_completed.usage` is the provider-reported request total for that turn
//! (including every model call in the tool loop). `inputTokens` includes cache
//! reads and `outputTokens` includes reasoning, so those nested counts are
//! subtracted before they are stored on `RawEntry`.

use std::collections::{HashMap, hash_map::DefaultHasher};
use std::fs::File;
use std::hash::{Hash, Hasher};
use std::io::{BufRead, BufReader};
use std::path::Path;

use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::consts::DATE_FORMAT;
use crate::core::{CostKind, RawEntry};
use crate::utils::Timezone;

use super::parser::{GrokSessionContext, first_non_empty};

const TURN_COMPLETED: &str = "turn_completed";
const USD_TICKS_PER_DOLLAR: f64 = 10_000_000_000.0;

#[derive(Debug, Deserialize, Default, Clone)]
#[serde(default, rename_all = "camelCase")]
struct UsageTokens {
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    cached_read_tokens: Option<i64>,
    cache_creation_tokens: Option<i64>,
    reasoning_tokens: Option<i64>,
    model_calls: Option<i64>,
    cost_usd_ticks: Option<i64>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct TurnUsage {
    #[serde(flatten)]
    tokens: UsageTokens,
    model_usage: HashMap<String, UsageTokens>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct SessionUpdate {
    #[serde(rename = "sessionUpdate", alias = "session_update")]
    kind: Option<String>,
    #[serde(alias = "promptId")]
    prompt_id: Option<String>,
    #[serde(alias = "stopReason")]
    stop_reason: Option<String>,
    usage: Option<TurnUsage>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct UpdateMeta {
    event_id: Option<String>,
    agent_timestamp_ms: Option<i64>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct UpdateParams {
    session_id: Option<String>,
    update: Option<SessionUpdate>,
    #[serde(rename = "_meta")]
    meta: Option<UpdateMeta>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct UpdateEnvelope {
    timestamp: Option<i64>,
    params: Option<UpdateParams>,
}

#[derive(Clone, Default)]
struct NormalizedUsage {
    input_tokens: i64,
    output_tokens: i64,
    cache_creation: i64,
    cache_read: i64,
    reasoning_tokens: i64,
    call_count: i64,
    recorded_cost_usd: Option<f64>,
}

pub(super) struct TurnUsageParseOutput {
    pub(super) entries: Vec<RawEntry>,
    pub(super) errors: usize,
    pub(super) saw_usage_record: bool,
}

pub(super) fn parse_turn_completed_usage(
    path: &Path,
    timezone: Timezone,
    debug: bool,
    ctx: &GrokSessionContext,
) -> TurnUsageParseOutput {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return TurnUsageParseOutput {
                entries: Vec::new(),
                errors: 0,
                saw_usage_record: false,
            };
        }
        Err(err) => {
            if debug {
                eprintln!("Failed to read {}: {}", path.display(), err);
            }
            return TurnUsageParseOutput {
                entries: Vec::new(),
                errors: 1,
                saw_usage_record: false,
            };
        }
    };

    let mut entries = Vec::new();
    let mut errors = 0;
    let mut saw_usage_record = false;
    for (line_index, line) in BufReader::new(file).lines().enumerate() {
        let line = match line {
            Ok(line) => line,
            Err(err) => {
                errors += 1;
                if debug {
                    eprintln!(
                        "Failed to read {} line {}: {}",
                        path.display(),
                        line_index + 1,
                        err
                    );
                }
                continue;
            }
        };
        if !looks_like_turn_completed_record(&line) {
            continue;
        }
        // A turn-completed record is authoritative even when it is truncated
        // before the usage object. Never replace it with a cumulative snapshot.
        saw_usage_record = true;
        match parse_turn_line(&line, timezone, ctx) {
            Ok(Some(turn_entries)) => {
                saw_usage_record = true;
                entries.extend(turn_entries);
            }
            Ok(None) => {
                errors += 1;
            }
            Err(err) => {
                errors += 1;
                if debug {
                    eprintln!(
                        "Invalid JSON in {} line {}: {err}",
                        path.display(),
                        line_index + 1
                    );
                }
            }
        }
    }

    TurnUsageParseOutput {
        entries,
        errors,
        saw_usage_record,
    }
}

fn looks_like_turn_completed_record(line: &str) -> bool {
    ["\"sessionUpdate\"", "\"session_update\""]
        .into_iter()
        .filter_map(|key| line.split_once(key).map(|(_, value)| value))
        .any(|value| {
            value
                .trim_start()
                .strip_prefix(':')
                .is_some_and(|value| value.trim_start().starts_with("\"turn_completed\""))
        })
}

fn parse_turn_line(
    line: &str,
    timezone: Timezone,
    ctx: &GrokSessionContext,
) -> Result<Option<Vec<RawEntry>>, serde_json::Error> {
    let envelope: UpdateEnvelope = serde_json::from_str(line)?;
    let params = envelope.params.as_ref();
    let update = params.and_then(|params| params.update.as_ref());
    if update.and_then(|update| update.kind.as_deref()) != Some(TURN_COMPLETED) {
        return Ok(None);
    }
    let Some(usage) = update.and_then(|update| update.usage.as_ref()) else {
        return Ok(None);
    };

    let Some(utc_dt) = event_time(&envelope) else {
        return Ok(None);
    };
    let local_dt = timezone.to_fixed_offset(utc_dt);
    let date_str = local_dt.date_naive().format(DATE_FORMAT).to_string();
    let session_id = first_non_empty(&[params.and_then(|params| params.session_id.as_deref())])
        .unwrap_or_else(|| ctx.session_id.clone());
    let prompt_id = update.and_then(|update| update.prompt_id.as_deref());
    let event_id = params
        .and_then(|params| params.meta.as_ref())
        .and_then(|meta| meta.event_id.as_deref());
    let base_message_id = first_non_empty(&[event_id, prompt_id]).unwrap_or_else(|| {
        let mut hasher = DefaultHasher::new();
        line.hash(&mut hasher);
        format!("turn:{:016x}", hasher.finish())
    });
    let stop_reason = update.and_then(|update| update.stop_reason.clone());

    let total_usage = normalize_usage(&usage.tokens);
    let has_authoritative_total_cost = total_usage.recorded_cost_usd.is_some();
    let mut model_usages: Vec<(String, NormalizedUsage)> = usage
        .model_usage
        .iter()
        .filter(|(model, _)| !model.trim().is_empty())
        .map(|(model, tokens)| {
            let mut normalized = normalize_usage(tokens);
            if has_authoritative_total_cost && normalized.recorded_cost_usd.is_none() {
                normalized.recorded_cost_usd = Some(0.0);
            }
            (model.clone(), normalized)
        })
        .collect();
    model_usages.sort_by(|left, right| left.0.cmp(&right.0));
    if model_usages.is_empty() {
        model_usages.push((ctx.fallback_model.clone(), total_usage));
    } else {
        let attributed =
            model_usages
                .iter()
                .fold(NormalizedUsage::default(), |mut sum, (_, current)| {
                    sum.add(current);
                    sum
                });
        let residual = total_usage.residual_after(&attributed);
        if has_usage(&residual) {
            if let Some((_, fallback)) = model_usages
                .iter_mut()
                .find(|(model, _)| model == &ctx.fallback_model)
            {
                fallback.add(&residual);
            } else {
                model_usages.push((ctx.fallback_model.clone(), residual));
            }
        }
    }

    let mut entries = Vec::new();
    for (model, normalized) in model_usages {
        if !has_usage(&normalized) {
            continue;
        }
        let message_id = Some(format!("{base_message_id}:{model}"));
        entries.push(RawEntry {
            timestamp: utc_dt.to_rfc3339(),
            timestamp_ms: utc_dt.timestamp_millis(),
            date_str: date_str.clone(),
            message_id,
            session_key: ctx.session_key.clone(),
            session_id: session_id.clone(),
            project_path: ctx.project_path.clone(),
            model,
            input_tokens: normalized.input_tokens,
            output_tokens: normalized.output_tokens,
            cache_creation: normalized.cache_creation,
            cache_creation_1h: 0,
            cache_read: normalized.cache_read,
            reasoning_tokens: normalized.reasoning_tokens,
            stop_reason: stop_reason.clone(),
            cost_kind: CostKind::Real,
            endpoint: crate::core::Endpoint::Unknown,
            call_count: normalized.call_count,
            reported_total_tokens: None,
            recorded_cost_usd: normalized.recorded_cost_usd,
        });
    }
    Ok(Some(entries).filter(|entries| !entries.is_empty()))
}

fn event_time(envelope: &UpdateEnvelope) -> Option<DateTime<Utc>> {
    let meta_ms = envelope
        .params
        .as_ref()
        .and_then(|params| params.meta.as_ref())
        .and_then(|meta| meta.agent_timestamp_ms)
        .filter(|ms| *ms > 0);
    if let Some(timestamp) = meta_ms.and_then(DateTime::<Utc>::from_timestamp_millis) {
        return Some(timestamp);
    }

    let timestamp = envelope.timestamp.filter(|timestamp| *timestamp > 0)?;
    if timestamp > 1_000_000_000_000 {
        DateTime::<Utc>::from_timestamp_millis(timestamp)
    } else {
        DateTime::<Utc>::from_timestamp(timestamp, 0)
    }
}

fn normalize_usage(tokens: &UsageTokens) -> NormalizedUsage {
    let cache_read = tokens.cached_read_tokens.unwrap_or(0).max(0);
    let cache_creation = tokens.cache_creation_tokens.unwrap_or(0).max(0);
    let reasoning_tokens = tokens.reasoning_tokens.unwrap_or(0).max(0);
    let raw_input = tokens.input_tokens.unwrap_or(0).max(0);
    let raw_output = tokens.output_tokens.unwrap_or(0).max(0);
    let model_calls = tokens.model_calls.unwrap_or(0).max(0);
    NormalizedUsage {
        input_tokens: (raw_input - cache_read).max(0),
        output_tokens: (raw_output - reasoning_tokens).max(0),
        cache_creation,
        cache_read,
        reasoning_tokens,
        call_count: if model_calls > 0 { model_calls } else { 1 },
        recorded_cost_usd: tokens
            .cost_usd_ticks
            .filter(|ticks| *ticks >= 0)
            .map(|ticks| ticks as f64 / USD_TICKS_PER_DOLLAR),
    }
}

impl NormalizedUsage {
    fn add(&mut self, other: &Self) {
        self.input_tokens = self.input_tokens.saturating_add(other.input_tokens);
        self.output_tokens = self.output_tokens.saturating_add(other.output_tokens);
        self.cache_creation = self.cache_creation.saturating_add(other.cache_creation);
        self.cache_read = self.cache_read.saturating_add(other.cache_read);
        self.reasoning_tokens = self.reasoning_tokens.saturating_add(other.reasoning_tokens);
        self.call_count = self.call_count.saturating_add(other.call_count);
        self.recorded_cost_usd = match (self.recorded_cost_usd, other.recorded_cost_usd) {
            (Some(left), Some(right)) => Some(left + right),
            (Some(value), None) | (None, Some(value)) => Some(value),
            (None, None) => None,
        };
    }

    fn residual_after(&self, attributed: &Self) -> Self {
        Self {
            input_tokens: self
                .input_tokens
                .saturating_sub(attributed.input_tokens)
                .max(0),
            output_tokens: self
                .output_tokens
                .saturating_sub(attributed.output_tokens)
                .max(0),
            cache_creation: self
                .cache_creation
                .saturating_sub(attributed.cache_creation)
                .max(0),
            cache_read: self.cache_read.saturating_sub(attributed.cache_read).max(0),
            reasoning_tokens: self
                .reasoning_tokens
                .saturating_sub(attributed.reasoning_tokens)
                .max(0),
            call_count: self.call_count.saturating_sub(attributed.call_count).max(0),
            recorded_cost_usd: self
                .recorded_cost_usd
                .map(|total| (total - attributed.recorded_cost_usd.unwrap_or_default()).max(0.0)),
        }
    }
}

fn has_usage(usage: &NormalizedUsage) -> bool {
    usage.input_tokens > 0
        || usage.output_tokens > 0
        || usage.cache_creation > 0
        || usage.cache_read > 0
        || usage.reasoning_tokens > 0
        || usage.recorded_cost_usd.is_some_and(|cost| cost > 0.0)
        || usage.call_count > 1
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::DedupAccumulator;
    use crate::source::grok::parser::GrokSessionContext;
    use crate::utils::Timezone;
    use std::fs;
    use tempfile::tempdir;

    fn tz() -> Timezone {
        Timezone::parse(Some("UTC")).unwrap()
    }

    fn ctx() -> GrokSessionContext {
        GrokSessionContext {
            session_id: "session-1".to_string(),
            session_key: "/tmp/session-1".to_string(),
            project_path: "/tmp/grok-project/".to_string(),
            fallback_model: "grok-build".to_string(),
        }
    }

    #[test]
    fn parses_turn_completed_usage_and_server_cost() {
        let root = tempdir().expect("temp dir");
        let path = root.path().join("updates.jsonl");
        fs::write(
            &path,
            r#"{"timestamp":1776374400,"method":"session/update","params":{"sessionId":"session-1","update":{"sessionUpdate":"tool_call"},"_meta":{}}}
{"timestamp":1776374400,"method":"_x.ai/session/update","params":{"sessionId":"session-1","update":{"sessionUpdate":"turn_completed","prompt_id":"p1","stop_reason":"end_turn","usage":{"inputTokens":165026,"outputTokens":3270,"cachedReadTokens":127360,"cacheCreationTokens":0,"reasoningTokens":1145,"modelCalls":5,"costUsdTicks":1381600000,"modelUsage":{"grok-4.5-build":{"inputTokens":165026,"outputTokens":3270,"cachedReadTokens":127360,"cacheCreationTokens":0,"reasoningTokens":1145,"modelCalls":5,"costUsdTicks":1381600000}}}},"_meta":{"eventId":"e1","agentTimestampMs":1776374400123}}}
"#,
        )
        .expect("write updates");

        let parsed = parse_turn_completed_usage(&path, tz(), true, &ctx());
        assert_eq!(parsed.errors, 0);
        assert_eq!(parsed.entries.len(), 1);
        let entry = &parsed.entries[0];
        assert_eq!(entry.model, "grok-4.5-build");
        assert_eq!(entry.input_tokens, 37666);
        assert_eq!(entry.cache_read, 127_360);
        assert_eq!(entry.output_tokens, 2125);
        assert_eq!(entry.reasoning_tokens, 1145);
        assert_eq!(entry.call_count, 5);
        assert_eq!(entry.date_str, "2026-04-16");
        assert_eq!(entry.message_id.as_deref(), Some("e1:grok-4.5-build"));
        assert_eq!(entry.stop_reason.as_deref(), Some("end_turn"));
        assert_eq!(entry.cost_kind, CostKind::Real);
        assert_eq!(entry.recorded_cost_usd, Some(0.13816));
        let stats = entry.to_stats();
        assert_eq!(stats.count, 5);
        assert_eq!(stats.recorded_cost_entries, 1);
        assert!((stats.recorded_cost_usd - 0.13816).abs() < 1e-12);
    }

    #[test]
    fn dates_from_event_time_not_session_updated_at() {
        let root = tempdir().expect("temp dir");
        let path = root.path().join("updates.jsonl");
        fs::write(
            &path,
            r#"{"timestamp":1786896000,"params":{"update":{"sessionUpdate":"turn_completed","usage":{"inputTokens":10,"outputTokens":2,"modelCalls":1,"costUsdTicks":100000000}}}}
"#,
        )
        .expect("write updates");

        let parsed = parse_turn_completed_usage(&path, tz(), true, &ctx());
        assert_eq!(parsed.entries[0].date_str, "2026-08-16");
    }

    #[test]
    fn splits_model_usage_and_keeps_per_model_cost() {
        let root = tempdir().expect("temp dir");
        let path = root.path().join("updates.jsonl");
        fs::write(
            &path,
            r#"{"timestamp":1786896000,"params":{"update":{"sessionUpdate":"turn_completed","prompt_id":"p2","usage":{"inputTokens":30,"outputTokens":6,"cachedReadTokens":0,"reasoningTokens":0,"modelCalls":3,"costUsdTicks":300,"modelUsage":{"grok-a":{"inputTokens":10,"outputTokens":2,"modelCalls":1,"costUsdTicks":100},"grok-b":{"inputTokens":20,"outputTokens":4,"modelCalls":2,"costUsdTicks":200}}}}}}
"#,
        )
        .expect("write updates");

        let parsed = parse_turn_completed_usage(&path, tz(), true, &ctx());
        assert_eq!(parsed.entries.len(), 2);
        assert_eq!(parsed.entries[0].model, "grok-a");
        assert_eq!(parsed.entries[0].input_tokens, 10);
        assert_eq!(parsed.entries[0].call_count, 1);
        assert_eq!(parsed.entries[0].message_id.as_deref(), Some("p2:grok-a"));
        assert_eq!(parsed.entries[1].model, "grok-b");
        assert_eq!(parsed.entries[1].call_count, 2);
        assert_eq!(parsed.entries[1].message_id.as_deref(), Some("p2:grok-b"));
    }

    #[test]
    fn clamps_negative_token_fields() {
        let usage = normalize_usage(&UsageTokens {
            input_tokens: Some(-8),
            output_tokens: Some(-3),
            cached_read_tokens: Some(-1),
            cache_creation_tokens: Some(-2),
            reasoning_tokens: Some(-4),
            model_calls: Some(-5),
            cost_usd_ticks: Some(-9),
        });
        assert_eq!(usage.input_tokens, 0);
        assert_eq!(usage.output_tokens, 0);
        assert_eq!(usage.cache_read, 0);
        assert_eq!(usage.cache_creation, 0);
        assert_eq!(usage.reasoning_tokens, 0);
        assert_eq!(usage.call_count, 1);
        assert_eq!(usage.recorded_cost_usd, None);
    }

    #[test]
    fn preserves_top_level_residual_when_model_breakdown_is_partial() {
        let root = tempdir().expect("temp dir");
        let path = root.path().join("updates.jsonl");
        fs::write(
            &path,
            r#"{"timestamp":1786896000,"params":{"update":{"sessionUpdate":"turn_completed","prompt_id":"p3","usage":{"inputTokens":40,"outputTokens":8,"modelCalls":4,"costUsdTicks":400,"modelUsage":{"grok-a":{"inputTokens":10,"outputTokens":2,"modelCalls":1},"grok-build":{"inputTokens":10,"outputTokens":2,"modelCalls":1,"costUsdTicks":100}}}}}}
"#,
        )
        .expect("write updates");

        let parsed = parse_turn_completed_usage(&path, tz(), true, &ctx());
        assert_eq!(parsed.entries.len(), 2);
        let fallback = parsed
            .entries
            .iter()
            .find(|entry| entry.model == "grok-build")
            .expect("fallback model");
        assert_eq!(fallback.input_tokens, 30);
        assert_eq!(fallback.output_tokens, 6);
        assert_eq!(fallback.call_count, 3);
        let stats: Vec<_> = parsed.entries.iter().map(RawEntry::to_stats).collect();
        assert!(stats.iter().all(|stats| !stats.priced_tokens.has_entries()));
        assert!(
            (stats
                .iter()
                .map(|stats| stats.recorded_cost_usd)
                .sum::<f64>()
                - 0.000_000_04)
                .abs()
                < 1e-18
        );
    }

    #[test]
    fn invalid_agent_timestamp_falls_back_to_envelope_timestamp() {
        let root = tempdir().expect("temp dir");
        let path = root.path().join("updates.jsonl");
        fs::write(
            &path,
            r#"{"timestamp":1786896000,"params":{"update":{"sessionUpdate":"turn_completed","usage":{"inputTokens":10}},"_meta":{"agentTimestampMs":9223372036854775807}}}
"#,
        )
        .expect("write updates");

        let parsed = parse_turn_completed_usage(&path, tz(), true, &ctx());
        assert_eq!(parsed.entries[0].date_str, "2026-08-16");
    }

    #[test]
    fn duplicate_turns_deduplicate_shared_models_without_dropping_new_models() {
        let root = tempdir().expect("temp dir");
        let path = root.path().join("updates.jsonl");
        fs::write(
            &path,
            r#"{"timestamp":1786896000,"params":{"_meta":{"eventId":"same"},"update":{"sessionUpdate":"turn_completed","usage":{"inputTokens":10,"modelUsage":{"z-grok":{"inputTokens":10}}}}}}
{"timestamp":1786896001,"params":{"_meta":{"eventId":"same"},"update":{"sessionUpdate":"turn_completed","usage":{"inputTokens":20,"modelUsage":{"a-grok":{"inputTokens":10},"z-grok":{"inputTokens":10}}}}}}
"#,
        )
        .expect("write updates");

        let parsed = parse_turn_completed_usage(&path, tz(), true, &ctx());
        let mut dedup = DedupAccumulator::new();
        dedup.extend(parsed.entries);
        let (entries, skipped) = dedup.finalize();
        assert_eq!(skipped, 1);
        assert_eq!(entries.len(), 2);
        assert!(entries.iter().any(|entry| entry.model == "a-grok"));
        assert!(entries.iter().any(|entry| entry.model == "z-grok"));
    }

    #[test]
    fn residual_tokens_do_not_invent_a_model_call() {
        let line = r#"{"timestamp":1786896000,"params":{"update":{"sessionUpdate":"turn_completed","usage":{"inputTokens":30,"modelCalls":2,"modelUsage":{"grok-a":{"inputTokens":10,"modelCalls":1},"grok-b":{"inputTokens":10,"modelCalls":1}}}}}}"#;
        let entries = parse_turn_line(line, tz(), &ctx())
            .expect("valid turn")
            .expect("usage entries");

        assert_eq!(
            entries
                .iter()
                .map(|entry| entry.to_stats().count)
                .sum::<i64>(),
            2
        );
        assert_eq!(
            entries.iter().map(|entry| entry.input_tokens).sum::<i64>(),
            30
        );
    }

    #[test]
    fn accepts_snake_case_turn_discriminator() {
        let line = r#"{"timestamp":1786896000,"params":{"update":{"session_update":"turn_completed","usage":{"inputTokens":10}}}}"#;
        let entries = parse_turn_line(line, tz(), &ctx())
            .expect("valid turn")
            .expect("usage entries");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].input_tokens, 10);
    }
}
