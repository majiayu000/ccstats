//! Xum cumulative session usage source.

use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::consts::{DATE_FORMAT, UNKNOWN};
use crate::core::{CostKind, Endpoint, RawEntry, source_wide_message_id};
use crate::source::{Capabilities, ParseOutput, Source};
use crate::utils::Timezone;

const XUM_ROOT_ENV: &str = "XUM_ROOT";

pub(crate) struct XumSource;

impl XumSource {
    pub(crate) const fn new() -> Self {
        Self
    }
}

impl Source for XumSource {
    fn name(&self) -> &'static str {
        "xum"
    }

    fn display_name(&self) -> &'static str {
        "Xum"
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
        find_usage_files()
    }

    fn parse_file(&self, path: &Path, timezone: Timezone, debug: bool) -> ParseOutput {
        parse_usage_file(path, timezone, debug)
    }
}

fn xum_root() -> Option<PathBuf> {
    env::var_os(XUM_ROOT_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".xum")))
}

fn find_usage_files() -> Vec<PathBuf> {
    let Some(root) = xum_root() else {
        return Vec::new();
    };
    let pattern = root.join("sessions/*/session-usage.json");
    let mut files = glob::glob(&pattern.to_string_lossy())
        .into_iter()
        .flatten()
        .flatten()
        .filter(|path| path.is_file())
        .collect::<Vec<_>>();
    files.sort();
    files.dedup();
    files
}

#[derive(Deserialize)]
struct UsageFile {
    version: u32,
    #[serde(rename = "byModel")]
    by_model: HashMap<String, ModelUsage>,
    #[serde(rename = "lastRequest")]
    last_request: Option<LastRequest>,
    #[serde(rename = "rolledUpFrom", default)]
    rolled_up_from: HashMap<String, serde_json::Value>,
}

#[derive(Deserialize)]
struct LastRequest {
    timestamp: i64,
}

#[derive(Deserialize)]
struct ModelUsage {
    input: Bucket,
    cached: Bucket,
    #[serde(rename = "cacheCreate")]
    cache_create: Bucket,
    output: Bucket,
    reasoning: Bucket,
    #[serde(rename = "costsIncluded", default)]
    costs_included: bool,
}

#[derive(Deserialize)]
struct Bucket {
    tokens: i64,
    cost_usd: Option<f64>,
}

fn read_usage(path: &Path) -> Result<UsageFile, ()> {
    let bytes = fs::read(path).map_err(|_| ())?;
    serde_json::from_slice(&bytes).map_err(|_| ())
}

fn valid_rollup_parent(path: &Path, usage: &UsageFile) -> bool {
    usage.version == 1
        && usage
            .last_request
            .as_ref()
            .map(|request| request.timestamp)
            .or_else(|| file_timestamp(path))
            .filter(|timestamp| *timestamp > 0)
            .and_then(DateTime::<Utc>::from_timestamp_millis)
            .is_some()
        && !usage.by_model.is_empty()
        && usage.by_model.iter().all(|(model_key, usage)| {
            normalized_model(model_key).is_some()
                && buckets(usage).iter().all(|bucket| bucket.tokens >= 0)
                && recorded_cost(usage).is_ok()
        })
        && usage
            .by_model
            .values()
            .flat_map(buckets)
            .any(|bucket| bucket.tokens > 0 || bucket.cost_usd.is_some_and(|cost| cost > 0.0))
}

fn workspace_id(path: &Path) -> Option<String> {
    path.parent()
        .and_then(Path::file_name)
        .and_then(|value| value.to_str())
        .map(str::to_string)
}

enum RollupDisposition {
    Keep,
    KeepWithError,
    Suppress,
    SuppressWithError,
}

fn rollup_disposition(path: &Path) -> RollupDisposition {
    let Some(current_id) = workspace_id(path) else {
        return RollupDisposition::Keep;
    };
    let Some(sessions_dir) = path.parent().and_then(Path::parent) else {
        return RollupDisposition::Keep;
    };
    let pattern = sessions_dir.join("*/session-usage.json");
    let ledgers = glob::glob(&pattern.to_string_lossy())
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|candidate| {
            let id = workspace_id(&candidate)?;
            let usage = read_usage(&candidate).ok()?;
            valid_rollup_parent(&candidate, &usage).then_some((id, usage))
        })
        .collect::<HashMap<_, _>>();

    let self_cycle = ledgers
        .get(&current_id)
        .is_some_and(|usage| usage.rolled_up_from.contains_key(&current_id));
    let mut cycle_members = ledgers
        .keys()
        .filter(|id| {
            *id != &current_id
                && can_reach(&ledgers, &current_id, id, &mut Vec::new())
                && can_reach(&ledgers, id, &current_id, &mut Vec::new())
        })
        .cloned()
        .collect::<Vec<_>>();
    if self_cycle || !cycle_members.is_empty() {
        cycle_members.push(current_id.clone());
        let canonical = cycle_members.iter().min() == Some(&current_id);
        let has_external_parent = ledgers.iter().any(|(parent_id, usage)| {
            !cycle_members.contains(parent_id)
                && cycle_members
                    .iter()
                    .any(|member| usage.rolled_up_from.contains_key(member))
        });
        return if canonical && has_external_parent {
            RollupDisposition::SuppressWithError
        } else if canonical {
            RollupDisposition::KeepWithError
        } else {
            RollupDisposition::Suppress
        };
    }
    if ledgers.iter().any(|(parent_id, usage)| {
        parent_id != &current_id && usage.rolled_up_from.contains_key(&current_id)
    }) {
        RollupDisposition::Suppress
    } else {
        RollupDisposition::Keep
    }
}

fn can_reach(
    ledgers: &HashMap<String, UsageFile>,
    from: &str,
    target: &str,
    visiting: &mut Vec<String>,
) -> bool {
    if from == target {
        return true;
    }
    if visiting.iter().any(|id| id == from) {
        return false;
    }
    visiting.push(from.to_string());
    let found = ledgers.get(from).is_some_and(|usage| {
        usage
            .rolled_up_from
            .keys()
            .any(|child| can_reach(ledgers, child, target, visiting))
    });
    visiting.pop();
    found
}

fn buckets(usage: &ModelUsage) -> [&Bucket; 5] {
    [
        &usage.input,
        &usage.cached,
        &usage.cache_create,
        &usage.output,
        &usage.reasoning,
    ]
}

fn recorded_cost(usage: &ModelUsage) -> Result<Option<f64>, &'static str> {
    let bucket_costs = buckets(usage).map(|bucket| bucket.cost_usd);
    if bucket_costs
        .iter()
        .flatten()
        .any(|cost| !cost.is_finite() || *cost < 0.0)
    {
        return Err("invalid bucket cost");
    }
    if bucket_costs.iter().all(Option::is_some) {
        return Ok(Some(bucket_costs.into_iter().flatten().sum()));
    }
    Ok(usage.costs_included.then_some(0.0))
}

fn normalized_model(model_key: &str) -> Option<&str> {
    let model = model_key
        .split_once(':')
        .map_or(model_key, |(_, model)| model)
        .trim();
    (!model.is_empty()).then_some(model)
}

fn file_timestamp(path: &Path) -> Option<i64> {
    fs::metadata(path)
        .ok()?
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
}

fn parse_usage_file(path: &Path, timezone: Timezone, debug: bool) -> ParseOutput {
    let rollup_errors = match rollup_disposition(path) {
        RollupDisposition::Suppress => return ParseOutput::default(),
        RollupDisposition::SuppressWithError => {
            return ParseOutput {
                entries: Vec::new(),
                errors: 1,
            };
        }
        RollupDisposition::KeepWithError => 1,
        RollupDisposition::Keep => 0,
    };
    let usage = match read_usage(path) {
        Ok(usage) if usage.version == 1 => usage,
        Ok(_) => {
            return ParseOutput {
                entries: Vec::new(),
                errors: 1,
            };
        }
        Err(()) => {
            if debug {
                eprintln!("Failed to parse Xum usage file {}", path.display());
            }
            return ParseOutput {
                entries: Vec::new(),
                errors: 1,
            };
        }
    };
    let timestamp_ms = usage
        .last_request
        .map(|request| request.timestamp)
        .or_else(|| file_timestamp(path));
    let Some(timestamp) = timestamp_ms
        .filter(|value| *value > 0)
        .and_then(DateTime::<Utc>::from_timestamp_millis)
    else {
        return ParseOutput {
            entries: Vec::new(),
            errors: 1,
        };
    };
    let workspace_id = path.parent().and_then(Path::file_name).map_or_else(
        || UNKNOWN.to_string(),
        |value| value.to_string_lossy().into_owned(),
    );
    let date_str = timezone
        .to_fixed_offset(timestamp)
        .date_naive()
        .format(DATE_FORMAT)
        .to_string();
    let mut output = ParseOutput {
        entries: Vec::new(),
        errors: rollup_errors,
    };

    for (model_key, model_usage) in usage.by_model {
        let tokens = buckets(&model_usage).map(|bucket| bucket.tokens);
        if tokens.into_iter().any(|value| value < 0) {
            output.errors += 1;
            continue;
        }
        let Ok(recorded_cost_usd) = recorded_cost(&model_usage) else {
            output.errors += 1;
            continue;
        };
        if tokens.into_iter().all(|value| value == 0) && recorded_cost_usd.is_none() {
            continue;
        }
        let Some(model) = normalized_model(&model_key) else {
            output.errors += 1;
            continue;
        };
        let identity = format!("{workspace_id}:{model_key}");
        output.entries.push(RawEntry {
            timestamp: timestamp.to_rfc3339(),
            timestamp_ms: timestamp.timestamp_millis(),
            date_str: date_str.clone(),
            message_id: Some(source_wide_message_id("xum", &identity)),
            session_key: format!("xum::{workspace_id}"),
            session_id: workspace_id.clone(),
            project_path: UNKNOWN.to_string(),
            model: model.to_string(),
            input_tokens: model_usage.input.tokens,
            output_tokens: model_usage.output.tokens,
            cache_creation: model_usage.cache_create.tokens,
            cache_creation_1h: 0,
            cache_read: model_usage.cached.tokens,
            reasoning_tokens: model_usage.reasoning.tokens,
            stop_reason: Some("aggregate".to_string()),
            cost_kind: CostKind::Real,
            endpoint: Endpoint::Unknown,
            call_count: 0,
            reported_total_tokens: None,
            recorded_cost_usd,
        });
    }
    output
}
