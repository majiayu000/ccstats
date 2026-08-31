//! Durable Grok inference usage from `logs/unified.jsonl`.
//!
//! Grok trims the head of its unified log in place. Before parsing, ccstats
//! merges every inference record into an atomic, source-root-scoped
//! ledger under the platform application-data directory.

use std::collections::{BTreeMap, HashMap, hash_map::DefaultHasher};
use std::env;
use std::fs::{self, File, OpenOptions};
use std::hash::{Hash, Hasher};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::consts::{DATE_FORMAT, UNKNOWN};
use crate::core::{CostKind, RawEntry};
use crate::source::ParseOutput;
use crate::utils::Timezone;

const APP_DATA_DIR: &str = "ccstats";
const GROK_CACHE_DIR: &str = "grok";
const GROK_HOME_ENV: &str = "GROK_HOME";
const DEFAULT_GROK_DIR: &str = ".grok";
const UNIFIED_LOG: &str = "unified.jsonl";
const LEDGER_FILE: &str = "inference-v1.jsonl";
const SYNC_ERROR_FILE: &str = "inference-v1.sync-error";
const INFERENCE_DONE: &str = "shell.turn.inference_done";
const MODEL_CHANGED: &str = "model changed";
const LONG_CONTEXT_THRESHOLD: i64 = 200_000;

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct UnifiedEnvelope {
    ts: Option<String>,
    sid: Option<String>,
    msg: Option<String>,
    ctx: Option<InferenceContext>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct InferenceContext {
    loop_index: Option<i64>,
    prompt_tokens: Option<i64>,
    cached_prompt_tokens: Option<i64>,
    completion_tokens: Option<i64>,
    reasoning_tokens: Option<i64>,
    model: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct SessionSummary {
    current_model_id: Option<String>,
    git_root_dir: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct SessionMetadata {
    session_key: String,
    project_path: String,
    model: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct InferenceRecord {
    event_key: String,
    timestamp: String,
    session_id: String,
    session_key: String,
    project_path: String,
    model: String,
    prompt_tokens: i64,
    cached_prompt_tokens: i64,
    completion_tokens: i64,
    reasoning_tokens: i64,
}

#[derive(Clone, Copy)]
struct TokenRates {
    input: f64,
    cache_read: f64,
    output: f64,
}

fn api_cost_usd(
    model: &str,
    prompt_tokens: i64,
    cached_prompt_tokens: i64,
    completion_tokens: i64,
) -> Option<f64> {
    let model = model.to_ascii_lowercase();
    let is_long = prompt_tokens.max(0) >= LONG_CONTEXT_THRESHOLD;
    let rates = if model.contains("grok-4.6") {
        if is_long {
            TokenRates {
                input: 4e-6,
                cache_read: 1e-6,
                output: 12e-6,
            }
        } else {
            TokenRates {
                input: 2e-6,
                cache_read: 0.5e-6,
                output: 6e-6,
            }
        }
    } else if model.contains("grok-4.5") {
        if is_long {
            TokenRates {
                input: 4e-6,
                cache_read: 0.6e-6,
                output: 12e-6,
            }
        } else {
            TokenRates {
                input: 2e-6,
                cache_read: 0.3e-6,
                output: 6e-6,
            }
        }
    } else {
        return None;
    };

    let prompt_tokens = prompt_tokens.max(0);
    let cached_prompt_tokens = cached_prompt_tokens.clamp(0, prompt_tokens);
    let uncached_prompt_tokens = prompt_tokens.saturating_sub(cached_prompt_tokens);
    let completion_tokens = completion_tokens.max(0);
    Some(
        uncached_prompt_tokens as f64 * rates.input
            + cached_prompt_tokens as f64 * rates.cache_read
            + completion_tokens as f64 * rates.output,
    )
}

pub(super) fn grok_home() -> Option<PathBuf> {
    env::var_os(GROK_HOME_ENV)
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(DEFAULT_GROK_DIR)))
}

fn source_root_hash(path: &Path) -> String {
    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn ledger_path(grok_home: &Path) -> Option<PathBuf> {
    dirs::data_local_dir().map(|data_dir| {
        data_dir
            .join(APP_DATA_DIR)
            .join(GROK_CACHE_DIR)
            .join(source_root_hash(grok_home))
            .join(LEDGER_FILE)
    })
}

pub(super) fn find_grok_files() -> Vec<PathBuf> {
    let Some(grok_home) = grok_home() else {
        return Vec::new();
    };
    let source_path = grok_home.join("logs").join(UNIFIED_LOG);
    let sessions_dir = grok_home.join("sessions");
    let ledger_path = ledger_path(&grok_home);
    let mut files = super::parser::find_grok_files();

    if source_path.is_file() {
        files.push(if let Some(path) = ledger_path.as_deref() {
            sync_or_select_grok_file(&source_path, path, &sessions_dir)
        } else {
            source_path
        });
    } else if let Some(path) = ledger_path
        && path.is_file()
    {
        files.push(path);
    }

    files.sort();
    files.dedup();
    files
}

fn sync_or_select_grok_file(
    source_path: &Path,
    ledger_path: &Path,
    sessions_dir: &Path,
) -> PathBuf {
    match sync_ledger_at(source_path, ledger_path, sessions_dir) {
        Ok(records) if records > 0 => ledger_path.to_path_buf(),
        Ok(_) => source_path.to_path_buf(),
        Err(error) => {
            eprintln!(
                "Error: failed to persist the latest Grok inference log; incomplete result omitted: {error}"
            );
            ledger_path.with_file_name(SYNC_ERROR_FILE)
        }
    }
}

pub(super) fn parse_grok_file_with_debug(
    path: &Path,
    timezone: Timezone,
    debug: bool,
) -> ParseOutput {
    match path.file_name().and_then(|name| name.to_str()) {
        Some(LEDGER_FILE) => parse_ledger_file(path, timezone, debug),
        Some(SYNC_ERROR_FILE) => ParseOutput {
            entries: Vec::new(),
            errors: 1,
        },
        Some(UNIFIED_LOG) => parse_live_unified_file(path, timezone, debug),
        _ => super::parser::parse_grok_usage_file_with_debug(path, timezone, debug),
    }
}

fn sync_ledger_at(
    source_path: &Path,
    ledger_path: &Path,
    sessions_dir: &Path,
) -> Result<usize, String> {
    let _lock = super::ledger_lock::acquire(ledger_path)?;
    let mut records = load_ledger(ledger_path)?;
    for record in read_inference_records(source_path, sessions_dir)? {
        records.insert(record.event_key.clone(), record);
    }
    write_ledger_atomic(ledger_path, &records)?;
    Ok(records.len())
}

fn load_ledger(path: &Path) -> Result<BTreeMap<String, InferenceRecord>, String> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(error) => return Err(format!("failed to open {}: {error}", path.display())),
    };
    let mut records = BTreeMap::new();
    for (line_index, line) in BufReader::new(file).lines().enumerate() {
        let line = line.map_err(|error| {
            format!(
                "failed to read {} line {}: {error}",
                path.display(),
                line_index + 1
            )
        })?;
        if line.trim().is_empty() {
            continue;
        }
        let record: InferenceRecord = serde_json::from_str(&line).map_err(|error| {
            format!(
                "malformed ledger {} line {}: {error}",
                path.display(),
                line_index + 1
            )
        })?;
        records.insert(record.event_key.clone(), record);
    }
    Ok(records)
}

fn read_inference_records(
    path: &Path,
    sessions_dir: &Path,
) -> Result<Vec<InferenceRecord>, String> {
    let file =
        File::open(path).map_err(|error| format!("failed to open {}: {error}", path.display()))?;
    let session_metadata = load_session_metadata(sessions_dir);
    let mut active_models = HashMap::new();
    let mut records = Vec::new();

    for (line_index, line) in BufReader::new(file).lines().enumerate() {
        let line = line.map_err(|error| {
            format!(
                "failed to read {} line {}: {error}",
                path.display(),
                line_index + 1
            )
        })?;
        if !line.contains(INFERENCE_DONE) && !line.contains(MODEL_CHANGED) {
            continue;
        }
        let envelope: UnifiedEnvelope = serde_json::from_str(&line).map_err(|error| {
            format!(
                "malformed inference record {} line {}: {error}",
                path.display(),
                line_index + 1
            )
        })?;
        match envelope.msg.as_deref() {
            Some(MODEL_CHANGED) => {
                let Some(session_id) = envelope.sid.as_deref().map(str::trim) else {
                    continue;
                };
                let Some(model) = envelope
                    .ctx
                    .as_ref()
                    .and_then(|ctx| ctx.model.as_deref())
                    .map(str::trim)
                    .filter(|model| !model.is_empty())
                else {
                    continue;
                };
                if !session_id.is_empty() {
                    active_models.insert(session_id.to_string(), model.to_string());
                }
            }
            Some(INFERENCE_DONE) => {
                let record = normalize_inference(envelope, &session_metadata, &active_models)
                    .map_err(|error| {
                        format!(
                            "invalid inference record {} line {}: {error}",
                            path.display(),
                            line_index + 1
                        )
                    })?;
                records.push(record);
            }
            _ => {}
        }
    }
    Ok(records)
}

fn load_session_metadata(sessions_root: &Path) -> HashMap<String, SessionMetadata> {
    let pattern = format!("{}/**/summary.json", sessions_root.display());
    let Ok(paths) = glob::glob(&pattern) else {
        return HashMap::new();
    };
    paths
        .flatten()
        .filter_map(|path| {
            let summary: SessionSummary = serde_json::from_reader(File::open(&path).ok()?).ok()?;
            let session_path = path.parent()?;
            let session_id = session_path.file_name()?.to_str()?.to_string();
            let project_path = summary.git_root_dir.unwrap_or_else(|| {
                session_path
                    .parent()
                    .and_then(Path::file_name)
                    .and_then(|name| name.to_str())
                    .unwrap_or_default()
                    .to_string()
            });
            let model = summary
                .current_model_id
                .filter(|model| !model.trim().is_empty())
                .unwrap_or_else(|| "grok".to_string());
            Some((
                session_id,
                SessionMetadata {
                    session_key: session_path.display().to_string(),
                    project_path,
                    model,
                },
            ))
        })
        .collect()
}

fn normalize_inference(
    envelope: UnifiedEnvelope,
    session_metadata: &HashMap<String, SessionMetadata>,
    active_models: &HashMap<String, String>,
) -> Result<InferenceRecord, &'static str> {
    let timestamp = envelope.ts.ok_or("missing ts")?.trim().to_string();
    DateTime::parse_from_rfc3339(&timestamp).map_err(|_| "invalid ts")?;
    let session_id = envelope.sid.ok_or("missing sid")?.trim().to_string();
    if session_id.is_empty() {
        return Err("empty sid");
    }
    let ctx = envelope.ctx.ok_or("missing ctx")?;
    let prompt_tokens = ctx.prompt_tokens.ok_or("missing prompt_tokens")?.max(0);
    let cached_prompt_tokens = ctx
        .cached_prompt_tokens
        .unwrap_or(0)
        .clamp(0, prompt_tokens);
    let completion_tokens = ctx.completion_tokens.unwrap_or(0).max(0);
    let reasoning_tokens = ctx
        .reasoning_tokens
        .unwrap_or(0)
        .clamp(0, completion_tokens);
    let loop_index = ctx.loop_index.unwrap_or(0).max(0);
    let mut metadata = session_metadata
        .get(&session_id)
        .cloned()
        .unwrap_or_else(|| SessionMetadata {
            session_key: session_id.clone(),
            project_path: String::new(),
            model: "grok".to_string(),
        });
    if let Some(model) = active_models.get(&session_id) {
        metadata.model.clone_from(model);
    }
    let event_key = format!("{session_id}:{timestamp}:{loop_index}");

    Ok(InferenceRecord {
        event_key,
        timestamp,
        session_id,
        session_key: metadata.session_key,
        project_path: metadata.project_path,
        model: metadata.model,
        prompt_tokens,
        cached_prompt_tokens,
        completion_tokens,
        reasoning_tokens,
    })
}

fn write_ledger_atomic(
    path: &Path,
    records: &BTreeMap<String, InferenceRecord>,
) -> Result<(), String> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)
        .map_err(|error| format!("failed to create {}: {error}", parent.display()))?;
    let (temp_path, file) = create_temp_file(parent)?;
    let write_result = (|| {
        let mut writer = BufWriter::new(file);
        for record in records.values() {
            serde_json::to_writer(&mut writer, record)
                .map_err(|error| format!("failed to serialize {}: {error}", path.display()))?;
            writer
                .write_all(b"\n")
                .map_err(|error| format!("failed to write {}: {error}", temp_path.display()))?;
        }
        writer
            .flush()
            .map_err(|error| format!("failed to flush {}: {error}", temp_path.display()))?;
        writer
            .get_ref()
            .sync_all()
            .map_err(|error| format!("failed to sync {}: {error}", temp_path.display()))
    })();
    if let Err(error) = write_result {
        let _ = fs::remove_file(&temp_path);
        return Err(error);
    }
    fs::rename(&temp_path, path).map_err(|error| {
        let _ = fs::remove_file(&temp_path);
        format!(
            "failed to replace {} with {}: {error}",
            path.display(),
            temp_path.display()
        )
    })
}

fn create_temp_file(parent: &Path) -> Result<(PathBuf, File), String> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_nanos();
    let process_id = std::process::id();
    for attempt in 0..32 {
        let path = parent.join(format!(".{LEDGER_FILE}.{process_id}.{nanos}.{attempt}.tmp"));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(format!("failed to create {}: {error}", path.display())),
        }
    }
    Err(format!(
        "failed to create a unique temporary ledger in {}",
        parent.display()
    ))
}

fn parse_live_unified_file(path: &Path, timezone: Timezone, debug: bool) -> ParseOutput {
    let sessions_dir = grok_home()
        .map(|home| home.join("sessions"))
        .unwrap_or_default();
    match read_inference_records(path, &sessions_dir) {
        Ok(records) => records_to_parse_output(records, timezone),
        Err(error) => {
            if debug {
                eprintln!("{error}");
            }
            ParseOutput {
                entries: Vec::new(),
                errors: 1,
            }
        }
    }
}

fn parse_ledger_file(path: &Path, timezone: Timezone, debug: bool) -> ParseOutput {
    match load_ledger(path) {
        Ok(records) => records_to_parse_output(records.into_values(), timezone),
        Err(error) => {
            if debug {
                eprintln!("{error}");
            }
            ParseOutput {
                entries: Vec::new(),
                errors: 1,
            }
        }
    }
}

fn records_to_parse_output(
    records: impl IntoIterator<Item = InferenceRecord>,
    timezone: Timezone,
) -> ParseOutput {
    let mut errors = 0;
    let entries = records
        .into_iter()
        .filter_map(|record| {
            let Ok(utc_dt) = record.timestamp.parse::<DateTime<Utc>>() else {
                errors += 1;
                return None;
            };
            let prompt_tokens = record.prompt_tokens.max(0);
            let cache_read = record.cached_prompt_tokens.clamp(0, prompt_tokens);
            let completion_tokens = record.completion_tokens.max(0);
            let recorded_cost_usd =
                api_cost_usd(&record.model, prompt_tokens, cache_read, completion_tokens);
            let date_str = timezone
                .to_fixed_offset(utc_dt)
                .date_naive()
                .format(DATE_FORMAT)
                .to_string();
            Some(RawEntry {
                timestamp: utc_dt.to_rfc3339(),
                timestamp_ms: utc_dt.timestamp_millis(),
                date_str,
                message_id: Some(record.event_key),
                session_key: record.session_key,
                session_id: if record.session_id.is_empty() {
                    UNKNOWN.to_string()
                } else {
                    record.session_id
                },
                project_path: record.project_path,
                model: record.model,
                // Complete usage comes from turn_completed records. These
                // inference entries contribute only their exact request-boundary
                // API-equivalent cost, otherwise captured tokens are counted twice.
                input_tokens: 0,
                output_tokens: 0,
                cache_creation: 0,
                cache_creation_1h: 0,
                cache_read: 0,
                reasoning_tokens: 0,
                stop_reason: Some("inference_done".to_string()),
                cost_kind: CostKind::Real,
                endpoint: crate::core::Endpoint::Unknown,
                call_count: 0,
                reported_total_tokens: None,
                recorded_cost_usd,
                api_equivalent_priced_tokens: if recorded_cost_usd.is_some() {
                    prompt_tokens.saturating_add(completion_tokens)
                } else {
                    0
                },
                api_equivalent_coverage_tokens: 0,
            })
        })
        .collect();
    ParseOutput { entries, errors }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    fn tz() -> Timezone {
        Timezone::parse(Some("UTC")).expect("UTC timezone")
    }

    fn write_session_summary(sessions_root: &Path, session_id: &str, model: &str) {
        let session_path = sessions_root.join("%2Ftmp%2Fgrok-project").join(session_id);
        fs::create_dir_all(&session_path).expect("create session dir");
        fs::write(
            session_path.join("summary.json"),
            format!(r#"{{"current_model_id":"{model}","git_root_dir":"/tmp/grok-project"}}"#),
        )
        .expect("write summary");
    }

    fn inference_line(
        timestamp: &str,
        session_id: &str,
        loop_index: i64,
        prompt: i64,
        cached: i64,
        completion: i64,
        reasoning: i64,
    ) -> String {
        format!(
            r#"{{"ts":"{timestamp}","sid":"{session_id}","msg":"shell.turn.inference_done","ctx":{{"loop_index":{loop_index},"prompt_tokens":{prompt},"cached_prompt_tokens":{cached},"completion_tokens":{completion},"reasoning_tokens":{reasoning}}}}}"#
        )
    }

    fn model_changed_line(timestamp: &str, session_id: &str, model: &str) -> String {
        format!(
            r#"{{"ts":"{timestamp}","sid":"{session_id}","msg":"model changed","ctx":{{"model":"{model}"}}}}"#
        )
    }

    #[test]
    fn durable_ledger_uses_platform_data_directory() {
        let grok_home = Path::new("/tmp/example-grok-home");
        let path = ledger_path(grok_home).expect("platform data directory");
        let data_dir = dirs::data_local_dir().expect("platform data directory");

        assert!(path.starts_with(data_dir));
        assert_eq!(
            path.file_name().and_then(|name| name.to_str()),
            Some(LEDGER_FILE)
        );
        assert_eq!(
            path.parent()
                .and_then(Path::parent)
                .and_then(Path::file_name)
                .and_then(|name| name.to_str()),
            Some(GROK_CACHE_DIR)
        );
        assert_eq!(
            path.parent()
                .and_then(Path::parent)
                .and_then(Path::parent)
                .and_then(Path::file_name)
                .and_then(|name| name.to_str()),
            Some(APP_DATA_DIR)
        );
    }

    #[test]
    fn prices_grok_46_short_and_long_context_requests() {
        let short = api_cost_usd("grok-4.6", 150_000, 140_000, 1_000).expect("known model");
        let long =
            api_cost_usd("grok-4.6-build", 250_000, 240_000, 1_000).expect("known model alias");

        assert!((short - 0.096).abs() < 1e-12);
        assert!((long - 0.292).abs() < 1e-12);
    }

    #[test]
    fn prices_grok_45_with_its_cache_rates() {
        let short = api_cost_usd("grok-4.5", 150_000, 140_000, 1_000).expect("known model");
        let long =
            api_cost_usd("grok-4.5-build", 250_000, 240_000, 1_000).expect("known model alias");

        assert!((short - 0.068).abs() < 1e-12);
        assert!((long - 0.196).abs() < 1e-12);
    }

    #[path = "behavior_tests.rs"]
    mod behavior_tests;

    #[path = "lock_tests.rs"]
    mod lock_tests;
}
