//! Vercel Fx profile usage ledger and bounded recovery registry.
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::Value;

use crate::consts::DATE_FORMAT;
use crate::core::{CostKind, Endpoint, RawEntry, source_wide_message_id};
use crate::utils::Timezone;

use super::{Capabilities, ParseOutput, Source, fx_recovery};

pub(crate) struct FxSource;

impl FxSource {
    pub(crate) const fn new() -> Self {
        Self
    }
}

impl Source for FxSource {
    fn name(&self) -> &'static str {
        "fx"
    }

    fn display_name(&self) -> &'static str {
        "Vercel Fx"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            has_projects: false,
            has_billing_blocks: false,
            has_reasoning_tokens: true,
            has_cache_creation: true,
            has_cache_read: true,
            needs_dedup: false,
            has_tool_calls: false,
            has_endpoints: false,
        }
    }

    fn find_files(&self) -> Vec<PathBuf> {
        dirs::home_dir()
            .map(|home| home.join(".fx"))
            .filter(|root| {
                root.join("usage.jsonl").exists() || root.join("usage-recovery").exists()
            })
            .into_iter()
            .collect()
    }

    fn parse_file(&self, path: &Path, timezone: Timezone, debug: bool) -> ParseOutput {
        parse_root(path, timezone, debug)
    }
}

#[derive(Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(super) struct Fact {
    pub(super) id: String,
    created_at_ms: i64,
    model: String,
    input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
    cache_write_tokens: i64,
    reasoning_tokens: Option<i64>,
    #[serde(default)]
    billable_web_search_calls: i64,
    total_cost: f64,
}

#[derive(Clone, Default)]
pub(super) struct Ledger {
    output: ParseOutput,
    coverage: Option<i64>,
    facts: HashMap<String, Fact>,
    pending: HashMap<String, i64>,
    settled: HashSet<String>,
}

impl Ledger {
    pub(super) fn add_error(&mut self) {
        self.output.errors += 1;
    }

    pub(super) fn add_fact(&mut self, fact: Fact, timezone: Timezone) -> Result<(), &'static str> {
        let parsed_entry = entry(&fact, timezone)?;
        self.settled.insert(fact.id.clone());
        match self.facts.get(&fact.id) {
            Some(existing) if existing == &fact => Ok(()),
            Some(_) => Err("conflicting generation fact"),
            None => {
                self.facts.insert(fact.id.clone(), fact);
                self.output.entries.push(parsed_entry);
                Ok(())
            }
        }
    }

    pub(super) fn add_pending(&mut self, id: &str, timestamp: i64) -> Result<(), &'static str> {
        if self
            .pending
            .get(id)
            .is_some_and(|existing| *existing != timestamp)
        {
            return Err("conflicting pending marker");
        }
        self.pending.insert(id.to_string(), timestamp);
        Ok(())
    }

    pub(super) fn add_recovery_batch(
        &mut self,
        facts: Vec<Fact>,
        pending: Vec<(String, i64)>,
        timezone: Timezone,
    ) -> Result<(), &'static str> {
        let mut batch_facts = HashMap::new();
        for fact in &facts {
            drop(entry(fact, timezone)?);
            if self
                .facts
                .get(&fact.id)
                .is_some_and(|existing| existing != fact)
                || batch_facts
                    .insert(fact.id.as_str(), fact)
                    .is_some_and(|existing| existing != fact)
            {
                return Err("conflicting generation fact");
            }
        }
        let mut batch_pending = HashMap::new();
        for (id, timestamp) in &pending {
            if *timestamp < 0
                || self
                    .pending
                    .get(id)
                    .is_some_and(|existing| existing != timestamp)
                || batch_pending
                    .insert(id.as_str(), *timestamp)
                    .is_some_and(|existing| existing != *timestamp)
            {
                return Err("conflicting pending marker");
            }
        }
        for fact in facts {
            self.add_fact(fact, timezone)?;
        }
        for (id, timestamp) in pending {
            self.add_pending(&id, timestamp)?;
        }
        Ok(())
    }

    fn finish(mut self) -> ParseOutput {
        self.output.errors += self
            .pending
            .keys()
            .filter(|id| !self.settled.contains(*id))
            .count();
        self.output
    }
}

pub(super) fn valid_id(id: &str) -> bool {
    id.len() == 30
        && id.starts_with("gen_")
        && id[4..].bytes().all(|byte| {
            matches!(byte, b'0'..=b'9' | b'A'..=b'H' | b'J'..=b'K' | b'M'..=b'N' | b'P'..=b'T' | b'V'..=b'Z')
        })
}

fn entry(fact: &Fact, timezone: Timezone) -> Result<RawEntry, &'static str> {
    if !valid_id(&fact.id)
        || fact.created_at_ms < 0
        || fact.model.is_empty()
        || fact.model.len() > 1_024
        || !fact.model.bytes().all(|byte| (0x21..=0x7e).contains(&byte))
    {
        return Err("invalid generation identity");
    }
    let reasoning = fact.reasoning_tokens.unwrap_or(0);
    if [
        fact.input_tokens,
        fact.output_tokens,
        fact.cache_read_tokens,
        fact.cache_write_tokens,
        reasoning,
        fact.billable_web_search_calls,
    ]
    .into_iter()
    .any(|value| value < 0)
    {
        return Err("negative usage value");
    }
    let cached = fact
        .cache_read_tokens
        .checked_add(fact.cache_write_tokens)
        .ok_or("cache token overflow")?;
    if cached > fact.input_tokens {
        return Err("cache tokens exceed input tokens");
    }
    if reasoning > fact.output_tokens {
        return Err("reasoning tokens exceed output tokens");
    }
    if !fact.total_cost.is_finite() || fact.total_cost < 0.0 {
        return Err("invalid total cost");
    }
    let timestamp = DateTime::<Utc>::from_timestamp_millis(fact.created_at_ms)
        .ok_or("timestamp outside supported range")?;
    Ok(RawEntry {
        timestamp: timestamp.to_rfc3339(),
        timestamp_ms: fact.created_at_ms,
        date_str: timezone
            .to_fixed_offset(timestamp)
            .date_naive()
            .format(DATE_FORMAT)
            .to_string(),
        message_id: Some(source_wide_message_id("fx", &fact.id)),
        session_key: "fx::profile".to_string(),
        session_id: "unknown-session".to_string(),
        project_path: String::new(),
        model: fact.model.clone(),
        input_tokens: fact.input_tokens - cached,
        output_tokens: fact.output_tokens - reasoning,
        cache_creation: fact.cache_write_tokens,
        cache_creation_1h: 0,
        cache_read: fact.cache_read_tokens,
        reasoning_tokens: reasoning,
        stop_reason: Some("completed".to_string()),
        cost_kind: CostKind::Real,
        endpoint: Endpoint::Unknown,
        call_count: 1,
        reported_total_tokens: None,
        recorded_cost_usd: Some(fact.total_cost),
    })
}

fn non_negative_i64(object: &serde_json::Map<String, Value>, key: &str) -> Option<i64> {
    object.get(key).and_then(Value::as_i64).filter(|v| *v >= 0)
}

pub(super) fn deserialize_fact(value: Value) -> Result<Fact, &'static str> {
    let object = value
        .as_object()
        .ok_or("generation fact is not an object")?;
    if !matches!(object.len(), 9 | 10) || !object.contains_key("reasoning_tokens") {
        return Err("invalid generation fact shape");
    }
    serde_json::from_value(value).map_err(|_| "invalid generation fact")
}

fn parse_root(root: &Path, timezone: Timezone, debug: bool) -> ParseOutput {
    let mut ledger = Ledger::default();
    let profile = root.join("usage.jsonl");
    if profile.is_file() {
        parse_profile(&profile, timezone, debug, &mut ledger);
    } else if profile.exists() {
        ledger.output.errors += 1;
    }
    fx_recovery::parse_recovery_dir(root, timezone, debug, &mut ledger);
    ledger.finish()
}

#[allow(clippy::too_many_lines)]
fn parse_profile(path: &Path, timezone: Timezone, debug: bool, ledger: &mut Ledger) {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) => {
            if debug {
                eprintln!("Failed to read Fx ledger {}: {error}", path.display());
            }
            ledger.output.errors += 1;
            return;
        }
    };
    let Ok(text) = std::str::from_utf8(&bytes) else {
        ledger.output.errors += 1;
        return;
    };
    let complete = text.ends_with('\n') || text.is_empty();
    let line_count = text.lines().count();

    for (line_index, line) in text.lines().enumerate() {
        if !complete && line_index + 1 == line_count {
            ledger.output.errors += 1;
            break;
        }
        if line.trim().is_empty() {
            continue;
        }
        let result = (|| -> Result<(), &'static str> {
            let value: Value = serde_json::from_str(line).map_err(|_| "invalid JSON")?;
            let object = value.as_object().ok_or("record is not an object")?;
            if object.get("schema_version").and_then(Value::as_u64) != Some(1) {
                return Err("unsupported schema version");
            }
            let kind = object
                .get("kind")
                .and_then(Value::as_str)
                .ok_or("missing record kind")?;
            match kind {
                "coverage" => {
                    if object.len() != 3 {
                        return Err("invalid coverage record");
                    }
                    let timestamp = non_negative_i64(object, "started_at_ms")
                        .ok_or("invalid coverage timestamp")?;
                    if ledger
                        .coverage
                        .is_some_and(|existing| existing != timestamp)
                    {
                        return Err("conflicting coverage record");
                    }
                    ledger.coverage = Some(timestamp);
                }
                "generation" => {
                    if object.len() != 3 || ledger.coverage.is_none() {
                        return Err("generation record precedes coverage");
                    }
                    let fact = deserialize_fact(
                        object
                            .get("fact")
                            .cloned()
                            .ok_or("missing generation fact")?,
                    )?;
                    ledger.add_fact(fact, timezone)?;
                }
                "pending" => {
                    if object.len() != 4 || ledger.coverage.is_none() {
                        return Err("invalid pending record");
                    }
                    let id = object
                        .get("id")
                        .and_then(Value::as_str)
                        .filter(|id| valid_id(id))
                        .ok_or("invalid pending id")?;
                    let timestamp = non_negative_i64(object, "observed_at_ms")
                        .ok_or("invalid pending timestamp")?;
                    ledger.add_pending(id, timestamp)?;
                }
                "incident" => {
                    if object.len() != 4
                        || ledger.coverage.is_none()
                        || non_negative_i64(object, "occurred_at_ms").is_none()
                        || !object
                            .get("completeness")
                            .and_then(Value::as_str)
                            .is_some_and(|value| matches!(value, "pending" | "incomplete"))
                    {
                        return Err("invalid incident record");
                    }
                    ledger.output.errors += 1;
                }
                _ => return Err("unknown record kind"),
            }
            Ok(())
        })();
        if let Err(error) = result {
            ledger.output.errors += 1;
            if debug {
                eprintln!(
                    "Invalid Fx usage in {} line {}: {error}",
                    path.display(),
                    line_index + 1
                );
            }
        }
    }
}
