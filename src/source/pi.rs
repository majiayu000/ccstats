//! Pi coding-agent JSONL usage source.

use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::consts::{DATE_FORMAT, UNKNOWN};
use crate::core::{CostKind, Endpoint, RawEntry, source_wide_message_id};
use crate::source::{Capabilities, ParseOutput, Source};
use crate::utils::Timezone;

const PI_AGENT_DIR_ENV: &str = "PI_CODING_AGENT_DIR";
const PI_SESSION_DIR_ENV: &str = "PI_CODING_AGENT_SESSION_DIR";

pub(crate) struct PiSource;

impl PiSource {
    pub(crate) const fn new() -> Self {
        Self
    }
}

impl Source for PiSource {
    fn name(&self) -> &'static str {
        "pi"
    }

    fn display_name(&self) -> &'static str {
        "Pi"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["pi-agent"]
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            has_projects: true,
            has_billing_blocks: false,
            // Pi documents reasoning as a subset of output.
            has_reasoning_tokens: false,
            has_cache_creation: true,
            has_cache_read: true,
            // Branching copies existing entry ids into a new session file.
            needs_dedup: true,
            has_tool_calls: false,
            has_endpoints: false,
        }
    }

    fn find_files(&self) -> Vec<PathBuf> {
        find_pi_files()
    }

    fn parse_file(&self, path: &Path, timezone: Timezone, debug: bool) -> ParseOutput {
        parse_pi_file(path, timezone, debug)
    }
}

fn non_empty_env(name: &str) -> Option<PathBuf> {
    env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn pi_sessions_dir() -> Option<PathBuf> {
    non_empty_env(PI_SESSION_DIR_ENV)
        .or_else(|| non_empty_env(PI_AGENT_DIR_ENV).map(|root| root.join("sessions")))
        .or_else(|| dirs::home_dir().map(|home| home.join(".pi/agent/sessions")))
}

fn find_pi_files() -> Vec<PathBuf> {
    let Some(root) = pi_sessions_dir() else {
        return Vec::new();
    };
    let patterns = [root.join("*.jsonl"), root.join("**").join("*.jsonl")];
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

#[derive(Debug, Deserialize)]
struct PiHeader {
    #[serde(rename = "type")]
    entry_type: String,
    id: String,
    cwd: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PiEntry {
    #[serde(rename = "type")]
    entry_type: String,
    id: String,
    parent_id: Option<String>,
    timestamp: String,
    message: Option<PiMessage>,
    model_id: Option<String>,
    usage: Option<PiUsage>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PiMessage {
    role: String,
    model: Option<String>,
    usage: Option<PiUsage>,
    stop_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PiUsage {
    input: i64,
    output: i64,
    cache_read: i64,
    cache_write: i64,
    cost: Option<PiCost>,
}

#[derive(Debug, Deserialize)]
struct PiCost {
    total: f64,
}

struct PiUsageContext<'a> {
    entry_id: &'a str,
    timestamp: &'a str,
    session_id: &'a str,
    project_path: &'a str,
    model: Option<&'a str>,
    stop_reason: Option<String>,
    timezone: Timezone,
}

fn usage_entry(
    usage: PiUsage,
    context: PiUsageContext<'_>,
) -> Result<Option<RawEntry>, &'static str> {
    if context.entry_id.trim().is_empty() {
        return Err("empty entry id");
    }
    if [
        usage.input,
        usage.output,
        usage.cache_read,
        usage.cache_write,
    ]
    .into_iter()
    .any(|value| value < 0)
    {
        return Err("negative token count");
    }
    let recorded_cost_usd = usage
        .cost
        .map(|cost| cost.total)
        .filter(|cost| cost.is_finite() && *cost > 0.0);
    if usage.input == 0
        && usage.output == 0
        && usage.cache_read == 0
        && usage.cache_write == 0
        && recorded_cost_usd.is_none()
    {
        return Ok(None);
    }

    let timestamp = DateTime::parse_from_rfc3339(context.timestamp)
        .map_err(|_| "invalid timestamp")?
        .with_timezone(&Utc);
    let model = context
        .model
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .unwrap_or(UNKNOWN)
        .to_string();

    Ok(Some(RawEntry {
        timestamp: timestamp.to_rfc3339(),
        timestamp_ms: timestamp.timestamp_millis(),
        date_str: context
            .timezone
            .to_fixed_offset(timestamp)
            .date_naive()
            .format(DATE_FORMAT)
            .to_string(),
        message_id: Some(source_wide_message_id("pi", context.entry_id)),
        session_key: format!("pi::{}", context.session_id),
        session_id: context.session_id.to_string(),
        project_path: context.project_path.to_string(),
        model,
        input_tokens: usage.input,
        // Pi explicitly documents reasoning as a subset of output.
        output_tokens: usage.output,
        cache_creation: usage.cache_write,
        cache_creation_1h: 0,
        cache_read: usage.cache_read,
        reasoning_tokens: 0,
        stop_reason: context.stop_reason,
        cost_kind: CostKind::Real,
        endpoint: Endpoint::Unknown,
        call_count: 1,
        recorded_cost_usd,
        api_equivalent_priced_tokens: 0,
        api_equivalent_coverage_tokens: 0,
    }))
}

fn parse_entry(
    entry: PiEntry,
    header: &PiHeader,
    lineage_models: &mut HashMap<String, String>,
    timezone: Timezone,
) -> Result<Option<RawEntry>, &'static str> {
    let inherited_model = entry
        .parent_id
        .as_deref()
        .and_then(|parent| lineage_models.get(parent))
        .cloned();
    if let Some(model) = inherited_model.as_ref() {
        lineage_models.insert(entry.id.clone(), model.clone());
    }
    match entry.entry_type.as_str() {
        "model_change" => {
            if let Some(model) = entry
                .model_id
                .map(|model| model.trim().to_string())
                .filter(|model| !model.is_empty())
                .or(inherited_model)
            {
                lineage_models.insert(entry.id, model);
            }
            Ok(None)
        }
        "message" => {
            let Some(message) = entry.message else {
                return Err("message record is missing message");
            };
            if message.role != "assistant" {
                return Ok(None);
            }
            let Some(usage) = message.usage else {
                return Ok(None);
            };
            let model = message
                .model
                .as_deref()
                .map(str::trim)
                .filter(|model| !model.is_empty())
                .map(str::to_string)
                .or(inherited_model);
            if let Some(model) = model.as_ref() {
                lineage_models.insert(entry.id.clone(), model.clone());
            }
            usage_entry(
                usage,
                PiUsageContext {
                    entry_id: &entry.id,
                    timestamp: &entry.timestamp,
                    session_id: &header.id,
                    project_path: &header.cwd,
                    model: model.as_deref(),
                    stop_reason: message
                        .stop_reason
                        .filter(|reason| !reason.trim().is_empty())
                        .or_else(|| Some("assistant".to_string())),
                    timezone,
                },
            )
        }
        "compaction" | "branch_summary" => {
            let Some(usage) = entry.usage else {
                return Ok(None);
            };
            usage_entry(
                usage,
                PiUsageContext {
                    entry_id: &entry.id,
                    timestamp: &entry.timestamp,
                    session_id: &header.id,
                    project_path: &header.cwd,
                    model: inherited_model.as_deref(),
                    stop_reason: Some(entry.entry_type),
                    timezone,
                },
            )
        }
        _ => Ok(None),
    }
}

fn parse_pi_file(path: &Path, timezone: Timezone, debug: bool) -> ParseOutput {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) => {
            if debug {
                eprintln!("Failed to open Pi session {}: {error}", path.display());
            }
            return ParseOutput {
                entries: Vec::new(),
                errors: 1,
            };
        }
    };
    let mut lines = BufReader::new(file).lines().enumerate();
    let header = loop {
        let Some((line_index, line)) = lines.next() else {
            return ParseOutput::default();
        };
        let line = match line {
            Ok(line) => line,
            Err(error) => {
                if debug {
                    eprintln!(
                        "Failed to read {} line {}: {error}",
                        path.display(),
                        line_index + 1
                    );
                }
                return ParseOutput {
                    entries: Vec::new(),
                    errors: 1,
                };
            }
        };
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<PiHeader>(&line) {
            Ok(header)
                if header.entry_type == "session"
                    && !header.id.trim().is_empty()
                    && !header.cwd.trim().is_empty() =>
            {
                break header;
            }
            Ok(_) | Err(_) => {
                if debug {
                    eprintln!("Invalid Pi session header in {}", path.display());
                }
                return ParseOutput {
                    entries: Vec::new(),
                    errors: 1,
                };
            }
        }
    };

    parse_pi_entries(lines, &header, path, timezone, debug)
}

fn parse_pi_entries(
    lines: impl Iterator<Item = (usize, std::io::Result<String>)>,
    header: &PiHeader,
    path: &Path,
    timezone: Timezone,
    debug: bool,
) -> ParseOutput {
    let mut output = ParseOutput::default();
    let mut lineage_models = HashMap::new();
    for (line_index, line) in lines {
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
        let entry = match serde_json::from_str::<PiEntry>(&line) {
            Ok(entry) => entry,
            Err(error) => {
                output.errors += 1;
                if debug {
                    eprintln!(
                        "Invalid Pi JSON in {} line {}: {error}",
                        path.display(),
                        line_index + 1
                    );
                }
                continue;
            }
        };
        match parse_entry(entry, header, &mut lineage_models, timezone) {
            Ok(Some(entry)) => output.entries.push(entry),
            Ok(None) => {}
            Err(error) => {
                output.errors += 1;
                if debug {
                    eprintln!(
                        "Invalid Pi usage in {} line {}: {error}",
                        path.display(),
                        line_index + 1
                    );
                }
            }
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    use tempfile::tempdir;

    #[test]
    fn reasoning_stays_inside_output_and_reported_cost_is_preserved() {
        let usage = PiUsage {
            input: 10,
            output: 7,
            cache_read: 3,
            cache_write: 2,
            cost: Some(PiCost { total: 0.125 }),
        };

        let entry = usage_entry(
            usage,
            PiUsageContext {
                entry_id: "entry-1",
                timestamp: "2026-08-31T03:00:00Z",
                session_id: "session-1",
                project_path: "/tmp/project",
                model: Some("gpt-5"),
                stop_reason: Some("stop".to_string()),
                timezone: Timezone::Named(chrono_tz::UTC),
            },
        )
        .unwrap()
        .unwrap();

        assert_eq!(entry.output_tokens, 7);
        assert_eq!(entry.reasoning_tokens, 0);
        assert_eq!(entry.recorded_cost_usd, Some(0.125));
    }

    #[test]
    fn negative_usage_is_an_explicit_error() {
        let usage = PiUsage {
            input: -1,
            output: 0,
            cache_read: 0,
            cache_write: 0,
            cost: None,
        };

        assert!(matches!(
            usage_entry(
                usage,
                PiUsageContext {
                    entry_id: "entry-1",
                    timestamp: "2026-08-31T03:00:00Z",
                    session_id: "session-1",
                    project_path: "/tmp/project",
                    model: Some("gpt-5"),
                    stop_reason: Some("stop".to_string()),
                    timezone: Timezone::Named(chrono_tz::UTC),
                },
            ),
            Err("negative token count")
        ));
    }

    #[test]
    fn empty_usage_entry_id_is_an_explicit_error() {
        let usage = PiUsage {
            input: 1,
            output: 0,
            cache_read: 0,
            cache_write: 0,
            cost: None,
        };

        assert!(matches!(
            usage_entry(
                usage,
                PiUsageContext {
                    entry_id: "",
                    timestamp: "2026-08-31T03:00:00Z",
                    session_id: "session-1",
                    project_path: "/tmp/project",
                    model: Some("gpt-5"),
                    stop_reason: Some("stop".to_string()),
                    timezone: Timezone::Named(chrono_tz::UTC),
                },
            ),
            Err("empty entry id")
        ));
    }

    #[test]
    fn summary_model_follows_its_parent_lineage() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("session.jsonl");
        fs::write(
            &path,
            r#"{"type":"session","version":3,"id":"session-1","timestamp":"2026-08-31T03:00:00Z","cwd":"/tmp/project"}
{"type":"model_change","id":"model-a","parentId":null,"timestamp":"2026-08-31T03:00:01Z","provider":"a","modelId":"model-a"}
{"type":"message","id":"assistant-a","parentId":"model-a","timestamp":"2026-08-31T03:00:02Z","message":{"role":"assistant","provider":"a","model":"model-a","usage":{"input":1,"output":1,"cacheRead":0,"cacheWrite":0}}}
{"type":"model_change","id":"model-b","parentId":"assistant-a","timestamp":"2026-08-31T03:00:03Z","provider":"b","modelId":"model-b"}
{"type":"message","id":"assistant-b","parentId":"model-b","timestamp":"2026-08-31T03:00:04Z","message":{"role":"assistant","provider":"b","model":"model-b","usage":{"input":1,"output":1,"cacheRead":0,"cacheWrite":0}}}
{"type":"branch_summary","id":"summary-a","parentId":"assistant-a","timestamp":"2026-08-31T03:00:05Z","usage":{"input":1,"output":1,"cacheRead":0,"cacheWrite":0}}
"#,
        )
        .unwrap();

        let parsed = parse_pi_file(&path, Timezone::Named(chrono_tz::UTC), false);

        assert_eq!(parsed.errors, 0);
        let summary = parsed
            .entries
            .iter()
            .find(|entry| entry.stop_reason.as_deref() == Some("branch_summary"))
            .unwrap();
        assert_eq!(summary.model, "model-a");
    }
}
