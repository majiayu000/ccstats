//! Grok cost semantics derived from complete turns and request telemetry.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

use crate::consts::DATE_FORMAT;
use crate::core::{
    CostKind, CostTokens, DateFilter, DedupAccumulator, LoadResult, RawEntry, aggregate_daily,
};
use crate::source::Source;
use crate::utils::Timezone;

use super::unified::{
    InferenceRecord, LEDGER_FILE, SYNC_ERROR_FILE, UNIFIED_LOG, api_rates, find_grok_files,
    grok_home, load_ledger, read_inference_records, records_to_parse_output,
};
use super::{GrokSource, canonical_model_name};

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct GrokCostReport {
    pub(crate) total_tokens: i64,
    pub(crate) priced_tokens: i64,
    pub(crate) observed_api_cost_usd: f64,
    pub(crate) api_cost_lower_bound_usd: Option<f64>,
    pub(crate) api_cost_upper_bound_usd: Option<f64>,
    pub(crate) provider_reported_cost_usd: Option<f64>,
    pub(crate) provider_priced_tokens: i64,
    pub(crate) excluded_request_tokens: i64,
    pub(crate) coverage_mismatch: bool,
}

impl GrokCostReport {
    pub(crate) fn estimated_api_cost_usd(self) -> Option<f64> {
        let (Some(minimum), Some(maximum)) =
            (self.api_cost_lower_bound_usd, self.api_cost_upper_bound_usd)
        else {
            return None;
        };
        if self.coverage_mismatch || self.priced_tokens <= 0 {
            return None;
        }
        let scaled =
            self.observed_api_cost_usd * self.total_tokens as f64 / self.priced_tokens as f64;
        Some(scaled.clamp(minimum, maximum))
    }

    pub(crate) fn coverage_percent(self) -> f64 {
        percent(self.priced_tokens, self.total_tokens)
    }

    pub(crate) fn provider_percent(self) -> f64 {
        percent(self.provider_priced_tokens, self.total_tokens)
    }

    pub(crate) fn status(self) -> &'static str {
        if self.coverage_mismatch {
            "mismatch"
        } else if self.priced_tokens == self.total_tokens {
            "complete"
        } else {
            "partial"
        }
    }

    pub(crate) fn merge(self, other: Self) -> Self {
        Self {
            total_tokens: self.total_tokens.saturating_add(other.total_tokens),
            priced_tokens: self.priced_tokens.saturating_add(other.priced_tokens),
            observed_api_cost_usd: self.observed_api_cost_usd + other.observed_api_cost_usd,
            api_cost_lower_bound_usd: sum_options(
                self.api_cost_lower_bound_usd,
                other.api_cost_lower_bound_usd,
            ),
            api_cost_upper_bound_usd: sum_options(
                self.api_cost_upper_bound_usd,
                other.api_cost_upper_bound_usd,
            ),
            provider_reported_cost_usd: sum_optional_metrics(
                self.provider_reported_cost_usd,
                other.provider_reported_cost_usd,
            ),
            provider_priced_tokens: self
                .provider_priced_tokens
                .saturating_add(other.provider_priced_tokens),
            excluded_request_tokens: self
                .excluded_request_tokens
                .saturating_add(other.excluded_request_tokens),
            coverage_mismatch: self.coverage_mismatch || other.coverage_mismatch,
        }
    }
}

fn percent(part: i64, total: i64) -> f64 {
    if total <= 0 {
        0.0
    } else {
        part.max(0).min(total) as f64 / total as f64 * 100.0
    }
}

fn sum_options(left: Option<f64>, right: Option<f64>) -> Option<f64> {
    Some(left? + right?)
}

fn sum_optional_metrics(left: Option<f64>, right: Option<f64>) -> Option<f64> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left + right),
        (Some(value), None) | (None, Some(value)) => Some(value),
        (None, None) => None,
    }
}

#[derive(Default)]
struct DailyBuilder {
    total_by_model: HashMap<String, CostTokens>,
    priced_by_model: HashMap<String, CostTokens>,
    observed_api_cost_usd: f64,
    provider_reported_cost_usd: Option<f64>,
    provider_priced_tokens: i64,
    excluded_request_tokens: i64,
    mismatch: bool,
}

impl DailyBuilder {
    fn finish(self) -> GrokCostReport {
        let total_tokens = self.total_by_model.values().fold(0_i64, |sum, tokens| {
            sum.saturating_add(tokens.total_tokens())
        });
        let priced_tokens = self.priced_by_model.values().fold(0_i64, |sum, tokens| {
            sum.saturating_add(tokens.total_tokens())
        });
        let mut mismatch = self.mismatch || priced_tokens > total_tokens;
        for (model, priced) in &self.priced_by_model {
            match self.total_by_model.get(model) {
                Some(total) if !token_components_exceed(*priced, *total) => {}
                _ if priced.has_entries() => mismatch = true,
                _ => {}
            }
        }
        let bounds = (!mismatch)
            .then(|| {
                api_cost_bounds(
                    &self.total_by_model,
                    &self.priced_by_model,
                    self.observed_api_cost_usd,
                )
            })
            .flatten();
        GrokCostReport {
            total_tokens,
            priced_tokens,
            observed_api_cost_usd: self.observed_api_cost_usd,
            api_cost_lower_bound_usd: bounds.map(|(minimum, _)| minimum),
            api_cost_upper_bound_usd: bounds.map(|(_, maximum)| maximum),
            provider_reported_cost_usd: self.provider_reported_cost_usd,
            provider_priced_tokens: self.provider_priced_tokens,
            excluded_request_tokens: self.excluded_request_tokens,
            coverage_mismatch: mismatch,
        }
    }
}

struct InferenceObservation {
    entry: RawEntry,
    tokens: CostTokens,
}

struct SnapshotInputs {
    usage_entries: Vec<RawEntry>,
    inference_observations: Vec<InferenceObservation>,
    skipped: i64,
    parse_errors: usize,
    mismatch: bool,
}

pub(crate) fn load_daily_with_cost_reports(
    filter: &DateFilter,
    timezone: Timezone,
    quiet: bool,
    debug: bool,
) -> (LoadResult, HashMap<String, GrokCostReport>) {
    load_daily_with_cost_reports_from_files_and_options(
        &find_grok_files(),
        filter,
        timezone,
        quiet,
        debug,
    )
}

#[cfg(test)]
fn load_daily_with_cost_reports_from_files(
    files: &[PathBuf],
    filter: &DateFilter,
    timezone: Timezone,
) -> (LoadResult, HashMap<String, GrokCostReport>) {
    load_daily_with_cost_reports_from_files_and_options(files, filter, timezone, true, false)
}

fn load_daily_with_cost_reports_from_files_and_options(
    files: &[PathBuf],
    filter: &DateFilter,
    timezone: Timezone,
    quiet: bool,
    debug: bool,
) -> (LoadResult, HashMap<String, GrokCostReport>) {
    let load_start = Instant::now();
    if !quiet && !files.is_empty() {
        eprintln!("Scanning {} Grok files...", files.len());
    }

    let SnapshotInputs {
        mut usage_entries,
        inference_observations,
        skipped,
        parse_errors,
        mismatch,
    } = read_snapshot_files(files, filter, timezone, debug);
    let mut builders: HashMap<String, DailyBuilder> = HashMap::new();
    for entry in &usage_entries {
        if entry.cost_kind == CostKind::EstimatedProxy {
            continue;
        }
        let builder = builders.entry(entry.date_str.clone()).or_default();
        let tokens = entry_cost_tokens(entry);
        builder
            .total_by_model
            .entry(canonical_model_name(&entry.model))
            .or_default()
            .add(&tokens);
        if let Some(cost) = entry.recorded_cost_usd {
            builder.provider_reported_cost_usd =
                Some(builder.provider_reported_cost_usd.unwrap_or_default() + cost.max(0.0));
            builder.provider_priced_tokens = builder
                .provider_priced_tokens
                .saturating_add(entry.to_stats().total_tokens());
        }
    }

    let inference_entries = reconcile_inference_observations(inference_observations, &mut builders);
    let reports: HashMap<String, GrokCostReport> = builders
        .into_iter()
        .filter_map(|(date, mut builder)| {
            builder.mismatch |= mismatch;
            let report = builder.finish();
            (report.total_tokens > 0).then_some((date, report))
        })
        .collect();

    // The same parsed turn entries carry the provider metric into the report.
    // Clear it before normal aggregation so the table cost remains exclusively
    // the API-equivalent value from request-boundary inference entries.
    for entry in &mut usage_entries {
        if entry.cost_kind != CostKind::EstimatedProxy {
            entry.recorded_cost_usd = Some(0.0);
        }
    }
    usage_entries.extend(inference_entries);
    if reports.is_empty() {
        usage_entries = GrokSource::new().finalize_entries(usage_entries);
    }
    let valid = usage_entries.len() as i64;
    let day_stats = aggregate_daily(usage_entries);
    let elapsed_ms = load_start.elapsed().as_secs_f64() * 1000.0;

    if !quiet && !files.is_empty() {
        eprintln!(
            "Parsed {} files in one Grok snapshot ({elapsed_ms:.2}ms)",
            files.len()
        );
        if skipped > 0 {
            eprintln!("Deduplicated {skipped} entries");
        }
        if parse_errors > 0 {
            eprintln!("Warning: ignored {parse_errors} malformed records");
        }
    }

    (
        LoadResult {
            day_stats,
            skipped,
            valid,
            parse_errors,
            elapsed_ms,
        },
        reports,
    )
}

fn read_snapshot_files(
    files: &[PathBuf],
    filter: &DateFilter,
    timezone: Timezone,
    debug: bool,
) -> SnapshotInputs {
    let mut usage = DedupAccumulator::new();
    let mut inference_observations = Vec::new();
    let mut mismatch = false;
    let mut parse_errors = 0;

    for path in files {
        match path.file_name().and_then(|name| name.to_str()) {
            Some(LEDGER_FILE) => match load_ledger(path) {
                Ok(records) => add_inference_snapshot(
                    records.into_values().collect(),
                    filter,
                    timezone,
                    &mut inference_observations,
                    &mut mismatch,
                    &mut parse_errors,
                ),
                Err(error) => {
                    mismatch = true;
                    parse_errors += 1;
                    if debug {
                        eprintln!("{error}");
                    }
                }
            },
            Some(UNIFIED_LOG) => {
                let sessions_dir = grok_home()
                    .map(|home| home.join("sessions"))
                    .unwrap_or_default();
                match read_inference_records(path, &sessions_dir) {
                    Ok(records) => add_inference_snapshot(
                        records,
                        filter,
                        timezone,
                        &mut inference_observations,
                        &mut mismatch,
                        &mut parse_errors,
                    ),
                    Err(error) => {
                        mismatch = true;
                        parse_errors += 1;
                        if debug {
                            eprintln!("{error}");
                        }
                    }
                }
            }
            Some(SYNC_ERROR_FILE) => {
                mismatch = true;
                parse_errors += 1;
            }
            _ => {
                let parsed =
                    super::parser::parse_grok_session_file_for_provider(path, timezone, debug);
                parse_errors += parsed.errors;
                usage.extend(
                    parsed
                        .entries
                        .into_iter()
                        .filter(|entry| entry_in_filter(entry, filter)),
                );
            }
        }
    }

    let (usage_entries, skipped) = usage.finalize();
    SnapshotInputs {
        usage_entries,
        inference_observations,
        skipped,
        parse_errors,
        mismatch,
    }
}

fn reconcile_inference_observations(
    mut inference_observations: Vec<InferenceObservation>,
    builders: &mut HashMap<String, DailyBuilder>,
) -> Vec<RawEntry> {
    let mut inference_entries = Vec::new();
    inference_observations.sort_by_key(|observation| observation.entry.timestamp_ms);
    for observation in inference_observations {
        let builder = builders
            .entry(observation.entry.date_str.clone())
            .or_default();
        if observation.entry.recorded_cost_usd.is_none() {
            if builder.total_by_model.is_empty() {
                inference_entries.push(observation.entry);
            }
            continue;
        }
        let model = canonical_model_name(&observation.entry.model);
        let mut candidate = builder
            .priced_by_model
            .get(&model)
            .copied()
            .unwrap_or_default();
        candidate.add(&observation.tokens);
        let matches_completed_usage = builder
            .total_by_model
            .get(&model)
            .is_some_and(|total| !token_components_exceed(candidate, *total));
        if matches_completed_usage {
            builder.priced_by_model.insert(model, candidate);
            builder.observed_api_cost_usd +=
                observation.entry.recorded_cost_usd.unwrap_or_default();
            inference_entries.push(observation.entry);
        } else if builder.total_by_model.is_empty() {
            // Preserve exact inference-only fallback rows for logs that have no
            // completed turns at all. They remain outside completed-turn cost
            // coverage, so they cannot poison or inflate a period estimate.
            inference_entries.push(observation.entry);
        } else {
            builder.excluded_request_tokens = builder
                .excluded_request_tokens
                .saturating_add(observation.tokens.total_tokens());
        }
    }
    inference_entries
}

fn entry_in_filter(entry: &RawEntry, filter: &DateFilter) -> bool {
    if filter.has_timestamp_range() {
        return filter.contains_entry_timestamp(&entry.timestamp, entry.timestamp_ms);
    }
    chrono::NaiveDate::parse_from_str(&entry.date_str, DATE_FORMAT)
        .is_ok_and(|date| filter.contains(date))
}

fn add_inference_snapshot(
    records: Vec<InferenceRecord>,
    filter: &DateFilter,
    timezone: Timezone,
    observations: &mut Vec<InferenceObservation>,
    global_mismatch: &mut bool,
    parse_errors: &mut usize,
) {
    let mut tokens_by_event: HashMap<String, CostTokens> = records
        .iter()
        .map(|record| {
            let prompt = record.prompt_tokens.max(0);
            let cached = record.cached_prompt_tokens.clamp(0, prompt);
            (
                record.event_key.clone(),
                CostTokens {
                    input_tokens: prompt.saturating_sub(cached),
                    output_tokens: record.completion_tokens.max(0),
                    cache_read: cached,
                    ..CostTokens::default()
                },
            )
        })
        .collect();
    let parsed = records_to_parse_output(records, timezone);
    *parse_errors += parsed.errors;
    *global_mismatch |= parsed.errors > 0;
    observations.extend(parsed.entries.into_iter().filter_map(|entry| {
        if !entry_in_filter(&entry, filter) {
            return None;
        }
        let event_key = entry.message_id.as_deref()?;
        let tokens = tokens_by_event.remove(event_key)?;
        Some(InferenceObservation { entry, tokens })
    }));
}

fn entry_cost_tokens(entry: &RawEntry) -> CostTokens {
    CostTokens {
        input_tokens: entry.input_tokens,
        output_tokens: entry.output_tokens.saturating_add(entry.reasoning_tokens),
        cache_creation: entry.cache_creation,
        cache_creation_1h: entry.cache_creation_1h,
        cache_read: entry.cache_read,
        reasoning_tokens: 0,
        count: 0,
    }
}

fn token_components_exceed(left: CostTokens, right: CostTokens) -> bool {
    left.input_tokens > right.input_tokens
        || left.output_tokens > right.output_tokens
        || left.cache_creation > right.cache_creation
        || left.cache_read > right.cache_read
}

fn api_cost_bounds(
    total_by_model: &HashMap<String, CostTokens>,
    priced_by_model: &HashMap<String, CostTokens>,
    observed_cost: f64,
) -> Option<(f64, f64)> {
    let mut minimum = observed_cost;
    let mut maximum = observed_cost;
    for (model, total) in total_by_model {
        let priced = priced_by_model.get(model).copied().unwrap_or_default();
        let missing = total.saturating_sub(&priced);
        minimum += cost_tokens_at_tier(model, missing, false)?;
        maximum += cost_tokens_at_tier(model, missing, true)?;
    }
    Some((minimum, maximum))
}

fn cost_tokens_at_tier(model: &str, tokens: CostTokens, is_long: bool) -> Option<f64> {
    let rates = api_rates(model, is_long)?;
    let input = tokens.input_tokens.saturating_add(tokens.cache_creation);
    let output = tokens.output_tokens.saturating_add(tokens.reasoning_tokens);
    Some(
        input as f64 * rates.input
            + tokens.cache_read as f64 * rates.cache_read
            + output as f64 * rates.output,
    )
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    fn tz() -> Timezone {
        Timezone::parse(Some("UTC")).expect("UTC timezone")
    }

    #[test]
    fn reports_api_estimate_range_and_provider_metric_separately() {
        let root = tempdir().expect("temp dir");
        let session_dir = root.path().join("session-1");
        fs::create_dir_all(&session_dir).expect("create session dir");
        fs::write(
            session_dir.join("summary.json"),
            r#"{"current_model_id":"grok-4.6","git_root_dir":"/tmp/grok-project"}"#,
        )
        .expect("write summary");
        let updates_path = session_dir.join("updates.jsonl");
        fs::write(
            &updates_path,
            r#"{"timestamp":1786896000,"params":{"sessionId":"session-1","update":{"sessionUpdate":"turn_completed","usage":{"inputTokens":300000,"outputTokens":10000,"cachedReadTokens":200000,"reasoningTokens":2000,"modelCalls":2,"costUsdTicks":10000000000,"modelUsage":{"grok-4.6-build":{"inputTokens":300000,"outputTokens":10000,"cachedReadTokens":200000,"reasoningTokens":2000,"modelCalls":2,"costUsdTicks":10000000000}}}},"_meta":{"eventId":"turn-1"}}}
"#,
        )
        .expect("write updates");
        let ledger_path = root.path().join(LEDGER_FILE);
        fs::write(
            &ledger_path,
            r#"{"event_key":"inference-1","timestamp":"2026-08-16T00:00:00Z","session_id":"session-1","session_key":"session-1","project_path":"/tmp/grok-project","model":"grok-4.6","prompt_tokens":150000,"cached_prompt_tokens":100000,"completion_tokens":5000,"reasoning_tokens":1000}
{"event_key":"inference-2","timestamp":"2026-08-16T00:01:00Z","session_id":"session-1","session_key":"session-1","project_path":"/tmp/grok-project","model":"grok-4.6","prompt_tokens":300000,"cached_prompt_tokens":200000,"completion_tokens":10000,"reasoning_tokens":2000}
"#,
        )
        .expect("write ledger");

        let date = chrono::NaiveDate::from_ymd_opt(2026, 8, 16).unwrap();
        let (result, reports) = load_daily_with_cost_reports_from_files(
            &[updates_path, ledger_path],
            &DateFilter::new(Some(date), Some(date)),
            tz(),
        );
        let report = reports.get("2026-08-16").expect("daily report");
        let day = result.day_stats.get("2026-08-16").expect("daily table row");

        assert_eq!(day.stats.total_tokens(), report.total_tokens);
        assert_eq!(day.stats.api_equivalent_priced_tokens, report.priced_tokens);
        assert!((day.stats.recorded_cost_usd - report.observed_api_cost_usd).abs() < 1e-12);
        assert_eq!(report.total_tokens, 310_000);
        assert_eq!(report.priced_tokens, 155_000);
        assert!((report.observed_api_cost_usd - 0.18).abs() < 1e-12);
        assert!((report.estimated_api_cost_usd().unwrap() - 0.36).abs() < 1e-12);
        assert!((report.api_cost_lower_bound_usd.unwrap() - 0.36).abs() < 1e-12);
        assert!((report.api_cost_upper_bound_usd.unwrap() - 0.54).abs() < 1e-12);
        assert_eq!(report.provider_reported_cost_usd, Some(1.0));
        assert_eq!(report.provider_priced_tokens, 310_000);
        assert_eq!(report.excluded_request_tokens, 310_000);
    }

    #[test]
    fn mismatch_hides_estimate_and_range() {
        let mut builder = DailyBuilder::default();
        builder.total_by_model.insert(
            "grok-4.6".to_string(),
            CostTokens {
                input_tokens: 10,
                ..CostTokens::default()
            },
        );
        builder.priced_by_model.insert(
            "grok-4.6".to_string(),
            CostTokens {
                input_tokens: 20,
                ..CostTokens::default()
            },
        );

        let report = builder.finish();

        assert_eq!(report.status(), "mismatch");
        assert_eq!(report.estimated_api_cost_usd(), None);
        assert_eq!(report.api_cost_lower_bound_usd, None);
        assert_eq!(report.api_cost_upper_bound_usd, None);
    }
}
