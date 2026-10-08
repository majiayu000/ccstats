//! Local Claude usage evidence, never a reconstruction of subscription billing.
use std::collections::{BTreeMap, BTreeSet};

use chrono::{Duration, Utc};
use serde::Serialize;

use crate::app::{CommandContext, print_json};
use crate::cli::Commands;
use crate::core::{DateFilter, Endpoint, RawEntry};
use crate::source::{ClaudeSource, load_entries};

const FIVE_HOURS: i64 = 5 * 60 * 60 * 1000;
const DAY: i64 = 24 * 60 * 60 * 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum DiagnoseWindow {
    #[value(name = "5h")]
    FiveHours,
    Today,
    #[value(name = "7d")]
    SevenDays,
}

#[derive(Debug, Default, Clone, Serialize)]
struct Usage {
    messages: usize,
    input_tokens: i64,
    output_tokens: i64,
    cache_write_tokens: i64,
    cache_read_tokens: i64,
    cache_write_1h_tokens: i64,
    total_tokens: i64,
    subagent_tokens: i64,
    cache_write_missing_messages: usize,
    turns: usize,
    compactions: usize,
}

impl Usage {
    fn add(&mut self, e: &RawEntry) {
        self.messages += 1;
        self.input_tokens += e.input_tokens;
        self.output_tokens += e.output_tokens;
        self.cache_write_tokens += e.cache_creation;
        self.cache_read_tokens += e.cache_read;
        self.cache_write_1h_tokens += e.cache_creation_1h;
        let total = e.to_stats().total_tokens();
        self.total_tokens += total;
        if let Some(d) = &e.claude_diagnostics {
            self.subagent_tokens += if d.is_subagent { total } else { 0 };
            self.cache_write_missing_messages += usize::from(!d.cache_write_reported);
        }
    }
}

#[derive(Debug, Serialize)]
struct Finding {
    kind: &'static str,
    message: String,
    messages: usize,
    baseline_windows: usize,
}

#[derive(Debug, Serialize)]
struct Dimension {
    model: String,
    endpoint: &'static str,
    usage: Usage,
    baseline_windows: usize,
    baseline_messages: usize,
    cache_share_baseline_windows: usize,
    cache_share_baseline_messages: usize,
    baseline_median_tokens: Option<f64>,
    baseline_median_cache_write_share: Option<f64>,
    token_ratio: Option<f64>,
}

#[derive(Debug, Serialize)]
struct SessionEvidence {
    session_id: String,
    parent_session_id: Option<String>,
    is_subagent: bool,
    usage: Usage,
    first_usage_at: String,
    last_usage_at: String,
    compaction_times: Vec<String>,
}

#[derive(Debug, Serialize)]
struct VersionRow {
    model: String,
    endpoint: &'static str,
    version: String,
    complete_turns: usize,
    messages: usize,
    median_cache_write_per_turn: f64,
    previous_version: Option<String>,
    change_pct: Option<f64>,
    assessment: &'static str,
}

#[derive(Debug, Serialize)]
pub(crate) struct Report {
    schema_version: u32,
    source: &'static str,
    metric: &'static str,
    start: String,
    end: String,
    baseline_start: String,
    baseline_windows: usize,
    baseline_messages: usize,
    baseline_median_tokens: Option<f64>,
    token_ratio: Option<f64>,
    usage: Usage,
    dimensions: Vec<Dimension>,
    sessions: Vec<SessionEvidence>,
    versions: Vec<VersionRow>,
    findings: Vec<Finding>,
    official_snapshot: Option<serde_json::Value>,
    parse_errors: usize,
    duplicates_skipped: i64,
    notes: Vec<String>,
}

fn timestamp(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms)
        .map(|at| at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
        .unwrap_or_default()
}

fn median(mut values: Vec<f64>) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    let mid = values.len() / 2;
    Some(if values.len().is_multiple_of(2) {
        f64::midpoint(values[mid - 1], values[mid])
    } else {
        values[mid]
    })
}

fn ratio(a: f64, b: Option<f64>) -> Option<f64> {
    b.filter(|b| *b > 0.0).map(|b| a / b)
}

fn share(part: i64, total: i64) -> f64 {
    if total > 0 {
        part as f64 / total as f64
    } else {
        0.0
    }
}

fn cohort(e: &RawEntry) -> (String, &'static str) {
    (
        e.claude_diagnostics
            .as_ref()
            .map_or_else(|| e.model.clone(), |d| d.model_id.clone()),
        e.endpoint.as_str(),
    )
}

fn summarize(entries: &[&RawEntry], start: i64, end: i64) -> Usage {
    let mut usage = Usage::default();
    let mut turns = BTreeSet::new();
    let mut compactions = BTreeSet::new();
    for e in entries {
        usage.add(e);
        if let Some(d) = &e.claude_diagnostics {
            if let Some(turn) = d.turn {
                turns.insert((&e.session_key, turn));
            }
            for at in &d.compactions {
                if *at >= start && *at < end {
                    compactions.insert((&e.session_key, *at));
                }
            }
        }
    }
    usage.turns = turns.len();
    usage.compactions = compactions.len();
    usage
}

#[allow(clippy::type_complexity)] // Exact model/endpoint/version maps to first time, messages and turn samples.
fn version_rows(entries: &[&RawEntry], start: i64, end: i64) -> Vec<VersionRow> {
    let mut turns: BTreeMap<(&str, u64), Vec<&RawEntry>> = BTreeMap::new();
    for e in entries {
        if let Some(turn) = e.claude_diagnostics.as_ref().and_then(|d| d.turn) {
            turns.entry((&e.session_key, turn)).or_default().push(e);
        }
    }
    // A turn crossing versions, models, endpoints or the report range is not comparable.
    let mut groups: BTreeMap<(String, &'static str, String), (i64, usize, Vec<f64>)> =
        BTreeMap::new();
    for turn in turns.values() {
        let first = turn
            .iter()
            .min_by_key(|e| e.timestamp_ms)
            .expect("nonempty turn");
        let Some(version) = &first.agent_version else {
            continue;
        };
        let key = cohort(first);
        if first.endpoint == Endpoint::Unknown
            || key.0.is_empty()
            || turn.iter().any(|e| {
                e.timestamp_ms < start
                    || e.timestamp_ms >= end
                    || e.agent_version.as_ref() != Some(version)
                    || cohort(e) != key
                    || !e.claude_diagnostics.as_ref().is_some_and(|d| {
                        d.cache_write_reported && d.turn_start_ms.is_some_and(|at| at >= start)
                    })
            })
            || !turn.iter().any(|e| {
                matches!(
                    e.stop_reason.as_deref(),
                    Some("end_turn" | "stop_sequence" | "max_tokens")
                )
            })
        {
            continue;
        }
        let group = groups.entry((key.0, key.1, version.clone())).or_insert((
            first.timestamp_ms,
            0,
            Vec::new(),
        ));
        group.0 = group.0.min(first.timestamp_ms);
        group.1 += turn.len();
        group
            .2
            .push(turn.iter().map(|e| e.cache_creation).sum::<i64>() as f64);
    }
    let mut groups: Vec<_> = groups.into_iter().collect();
    groups.sort_by(|a, b| {
        (a.0.0.as_str(), a.0.1, a.1.0, a.0.2.as_str()).cmp(&(
            b.0.0.as_str(),
            b.0.1,
            b.1.0,
            b.0.2.as_str(),
        ))
    });
    let mut rows: Vec<VersionRow> = Vec::new();
    for ((model, endpoint, version), (_, messages, samples)) in groups {
        let value = median(samples.clone()).unwrap_or(0.0);
        let previous = rows
            .last()
            .filter(|r| r.model == model && r.endpoint == endpoint);
        let change_pct = previous
            .and_then(|r| ratio(value, Some(r.median_cache_write_per_turn)))
            .map(|r| (r - 1.0) * 100.0);
        let assessment = match previous {
            None => "no_comparable_previous_version",
            Some(r) if r.complete_turns < 100 || samples.len() < 100 => "insufficient_samples",
            Some(_) if change_pct.is_none() => "zero_baseline_no_ratio",
            Some(_) if change_pct.is_some_and(|c| c.abs() >= 25.0) => {
                "observed_change_not_causation"
            }
            Some(_) => "below_25_percent_threshold",
        };
        rows.push(VersionRow {
            model,
            endpoint,
            version,
            complete_turns: samples.len(),
            messages,
            median_cache_write_per_turn: value,
            previous_version: previous.map(|r| r.version.clone()),
            change_pct,
            assessment,
        });
    }
    rows
}

#[allow(clippy::too_many_lines)] // Keep the evidence and its conclusions together.
fn analyze(
    entries: &[RawEntry],
    start: i64,
    end: i64,
    baseline_ranges: &[(i64, i64)],
    session: Option<&str>,
) -> Report {
    let selected: Vec<_> = entries
        .iter()
        .filter(|e| {
            session.is_none_or(|id| {
                e.session_id == id
                    || e.claude_diagnostics
                        .as_ref()
                        .and_then(|d| d.parent_session_id.as_deref())
                        == Some(id)
            })
        })
        .collect();
    let current: Vec<_> = selected
        .iter()
        .copied()
        .filter(|e| e.timestamp_ms >= start && e.timestamp_ms < end)
        .collect();
    let baseline_start = baseline_ranges.first().map_or(start, |r| r.0);
    let baseline: Vec<Vec<_>> = baseline_ranges
        .iter()
        .map(|(a, b)| {
            selected
                .iter()
                .copied()
                .filter(|e| e.timestamp_ms >= *a && e.timestamp_ms < *b)
                .collect()
        })
        .filter(|v: &Vec<_>| !v.is_empty())
        .collect();
    let baseline_totals: Vec<_> = baseline
        .iter()
        .map(|v| v.iter().map(|e| e.to_stats().total_tokens()).sum::<i64>() as f64)
        .collect();
    let baseline_median_tokens = median(baseline_totals);
    let usage = summarize(&current, start, end);
    let token_ratio = ratio(usage.total_tokens as f64, baseline_median_tokens);
    let mut grouped: BTreeMap<_, Vec<_>> = BTreeMap::new();
    for e in &current {
        grouped.entry(cohort(e)).or_default().push(*e);
    }
    let mut dimensions: Vec<_> = grouped
        .into_iter()
        .map(|((model, endpoint), v)| {
            let same: Vec<Vec<_>> = baseline
                .iter()
                .map(|w| {
                    w.iter()
                        .copied()
                        .filter(|e| cohort(e) == (model.clone(), endpoint))
                        .collect()
                })
                .filter(|w: &Vec<_>| !w.is_empty())
                .collect();
            let base_tokens = median(
                same.iter()
                    .map(|w| w.iter().map(|e| e.to_stats().total_tokens()).sum::<i64>() as f64)
                    .collect(),
            );
            let share_windows: Vec<_> = same
                .iter()
                .filter(|w| {
                    w.iter().map(|e| e.to_stats().total_tokens()).sum::<i64>() > 0
                        && w.iter().all(|e| {
                            e.claude_diagnostics
                                .as_ref()
                                .is_some_and(|d| d.cache_write_reported)
                        })
                })
                .collect();
            let base_share = median(
                share_windows
                    .iter()
                    .map(|w| {
                        share(
                            w.iter().map(|e| e.cache_creation).sum(),
                            w.iter().map(|e| e.to_stats().total_tokens()).sum(),
                        )
                    })
                    .collect(),
            );
            let usage = summarize(&v, start, end);
            Dimension {
                model,
                endpoint,
                baseline_windows: same.len(),
                baseline_messages: same.iter().map(Vec::len).sum(),
                cache_share_baseline_windows: share_windows.len(),
                cache_share_baseline_messages: share_windows.iter().map(|w| w.len()).sum(),
                token_ratio: ratio(usage.total_tokens as f64, base_tokens),
                usage,
                baseline_median_tokens: base_tokens,
                baseline_median_cache_write_share: base_share,
            }
        })
        .collect();
    dimensions.sort_by_key(|d| std::cmp::Reverse(d.usage.total_tokens));
    let mut grouped: BTreeMap<&str, Vec<_>> = BTreeMap::new();
    for e in &current {
        grouped.entry(&e.session_key).or_default().push(*e);
    }
    let mut sessions: Vec<_> = grouped
        .values()
        .map(|v| {
            let first = v
                .iter()
                .min_by_key(|e| e.timestamp_ms)
                .expect("nonempty session");
            let last = v
                .iter()
                .max_by_key(|e| e.timestamp_ms)
                .expect("nonempty session");
            let d = first.claude_diagnostics.as_ref();
            let times: BTreeSet<_> = v
                .iter()
                .filter_map(|e| e.claude_diagnostics.as_ref())
                .flat_map(|d| d.compactions.iter())
                .filter(|at| **at >= start && **at < end)
                .map(|at| timestamp(*at))
                .collect();
            SessionEvidence {
                session_id: first.session_id.clone(),
                parent_session_id: d.and_then(|d| d.parent_session_id.clone()),
                is_subagent: d.is_some_and(|d| d.is_subagent),
                usage: summarize(v, start, end),
                first_usage_at: timestamp(first.timestamp_ms),
                last_usage_at: timestamp(last.timestamp_ms),
                compaction_times: times.into_iter().collect(),
            }
        })
        .collect();
    sessions.sort_by_key(|s| std::cmp::Reverse(s.usage.total_tokens));
    let mut findings = Vec::new();
    let mut add = |kind, message, messages, baseline_windows| {
        findings.push(Finding {
            kind,
            message,
            messages,
            baseline_windows,
        });
    };
    if baseline.len() >= 2 && token_ratio.is_some_and(|r| r >= 2.0) {
        add(
            "window_above_baseline",
            format!(
                "Observed token volume is {:.2}x the median of {} active baseline windows; endpoint/model mix may differ.",
                token_ratio.unwrap_or_default(),
                baseline.len()
            ),
            usage.messages,
            baseline.len(),
        );
    }
    for dimension in &dimensions {
        if dimension.endpoint != "unknown"
            && dimension.cache_share_baseline_windows >= 2
            && dimension.usage.cache_write_missing_messages == 0
            && dimension
                .baseline_median_cache_write_share
                .is_some_and(|base| {
                    share(
                        dimension.usage.cache_write_tokens,
                        dimension.usage.total_tokens,
                    ) - base
                        >= 0.20
                })
        {
            add(
                "cache_write_share_increase",
                format!(
                    "{} / {}: cache-write share rose at least 20 percentage points within this model/endpoint cohort ({} messages). Inspect session times and recorded compactions; causation is unknown.",
                    dimension.model, dimension.endpoint, dimension.usage.messages
                ),
                dimension.usage.messages,
                dimension.cache_share_baseline_windows,
            );
        }
    }
    if usage.messages > 0 && share(usage.subagent_tokens, usage.total_tokens) >= 0.25 {
        add(
            "subagent_share",
            format!(
                "Subagent files account for {:.1}% of observed tokens; see their session rows.",
                100.0 * share(usage.subagent_tokens, usage.total_tokens)
            ),
            usage.messages,
            baseline.len(),
        );
    }
    if usage.cache_write_1h_tokens > 0
        && share(usage.cache_write_1h_tokens, usage.cache_write_tokens) >= 0.25
    {
        add(
            "one_hour_cache_writes",
            format!(
                "{} tokens use a 1-hour cache TTL ({:.1}% of writes). Published API cache-write rates are 2x base input versus 1.25x for 5 minutes (1.6x); subscription quota weighting is unknown.",
                usage.cache_write_1h_tokens,
                100.0 * share(usage.cache_write_1h_tokens, usage.cache_write_tokens)
            ),
            usage.messages,
            baseline.len(),
        );
    }
    let endpoints: BTreeSet<_> = current.iter().map(|e| e.endpoint.as_str()).collect();
    if endpoints.contains("native") && endpoints.contains("proxy") {
        add("mixed_endpoints","Native and proxy classifications coexist. Compare within each endpoint/model; missing cache fields are unknown, and a reported zero is not evidence of a version regression.".into(), usage.messages, baseline.len());
    }
    if usage.cache_write_missing_messages > 0 {
        add(
            "missing_cache_fields",
            format!(
                "{} of {} messages omit cache-write fields. Those messages cannot establish a zero-write baseline.",
                usage.cache_write_missing_messages, usage.messages
            ),
            usage.messages,
            baseline.len(),
        );
    }
    let versions = version_rows(&selected, baseline_start, end);
    let mut notes = vec!["Local observed token volume, not Anthropic billing or subscription quota consumption. Cache reads count as tokens here; these totals have no quota weighting.".into(),"Endpoint classification is an inference_geo heuristic, not verified routing. Unknown endpoints never enter version comparisons.".into(),"Baseline: previous 14 days, non-overlapping active windows only; absent windows are excluded. Version samples require a user boundary, a completion stop reason, fully reported cache writes, and one exact model/endpoint/version per turn.".into(),"Compactions are explicit native boundaries observed before usage; temporal association does not establish why cache was rewritten. Prompt changes are not inferred.".into()];
    if baseline.len() < 2 {
        notes.push("Fewer than two active baseline windows: no window anomaly conclusion.".into());
    }
    if versions.is_empty() {
        notes.push("No complete comparable version samples; missing metadata or partial turns are not filled in.".into());
    }
    if current.is_empty() {
        notes
            .push("No Claude usage in this window. Widen --window or check ccstats doctor.".into());
    }
    Report {
        schema_version: 1,
        source: "claude",
        metric: "observed_tokens",
        start: timestamp(start),
        end: timestamp(end),
        baseline_start: timestamp(baseline_start),
        baseline_windows: baseline.len(),
        baseline_messages: baseline.iter().map(Vec::len).sum(),
        baseline_median_tokens,
        token_ratio,
        usage,
        dimensions,
        sessions,
        versions,
        findings,
        official_snapshot: None,
        parse_errors: 0,
        duplicates_skipped: 0,
        notes,
    }
}

pub(crate) fn report(
    ctx: &CommandContext<'_>,
    window: DiagnoseWindow,
    session: Option<&str>,
) -> Result<Report, String> {
    let now = Utc::now();
    let end = now.timestamp_millis();
    let today = ctx.timezone.to_fixed_offset(now).date_naive();
    let (start, span) = match window {
        DiagnoseWindow::FiveHours => (end - FIVE_HOURS, FIVE_HOURS),
        DiagnoseWindow::Today => (
            ctx.timezone
                .date_start_utc_millis(today)
                .ok_or("Cannot resolve local midnight")?,
            DAY,
        ),
        DiagnoseWindow::SevenDays => (end - 7 * DAY, 7 * DAY),
    };
    let baseline_start = start - 14 * DAY;
    let mut ranges = Vec::new();
    if window == DiagnoseWindow::Today {
        // Compare elapsed local day with the same local time on each prior day,
        // including timezone offset changes rather than assuming every day is 24h.
        let local_now = ctx.timezone.to_fixed_offset(now);
        for days in (1..=14).rev() {
            let date = today - Duration::days(days);
            let a = ctx
                .timezone
                .date_start_utc_millis(date)
                .ok_or("Cannot resolve baseline midnight")?;
            let b = ctx
                .timezone
                .local_datetime_utc_millis(date.and_time(local_now.time()))
                .ok_or("Cannot resolve baseline local time")?;
            ranges.push((a, b));
        }
    } else {
        let mut b = start;
        while b - span >= baseline_start {
            ranges.push((b - span, b));
            b -= span;
        }
        ranges.reverse();
    }
    let filter = DateFilter::new(
        Some(
            ctx.timezone
                .to_fixed_offset(now - Duration::days(22))
                .date_naive(),
        ),
        Some(today),
    );
    let (entries, duplicates, errors) = load_entries(
        &ClaudeSource::with_accounting_diagnostics(),
        &filter,
        ctx.timezone,
    );
    if let Some(id) = session {
        if id.is_empty() {
            return Err("session ID must not be empty".into());
        }
        if !entries.iter().any(|e| e.session_id == id) {
            return Err(format!(
                "No Claude session '{id}' found in the last 22 days"
            ));
        }
    }
    let mut result = analyze(&entries, start, end, &ranges, session);
    result.parse_errors = errors;
    result.duplicates_skipped = duplicates;
    if errors > 0 {
        result.notes.push(format!(
            "{errors} parse errors: report is partial; inspect the source logs."
        ));
    }
    let state = crate::quota::load_claude_quota_state();
    result.official_snapshot = state.latest().map(|s| serde_json::json!({"source":"statusline","captured_at":s.captured_at,"stale":state.is_stale(now),"five_hour":s.five_hour,"seven_day":s.seven_day,"scope":"account-wide snapshot; not attributed to this report window or session"}));
    if result.official_snapshot.is_none() {
        result
            .notes
            .push("No official Claude snapshot collected by statusline.".into());
    }
    Ok(result)
}

#[allow(clippy::too_many_lines)]
fn text(report: &Report, versions_only: bool) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let _ = writeln!(
        out,
        "Claude quota diagnosis  {} — {}",
        report.start, report.end
    );
    if !versions_only {
        let _ = writeln!(
            out,
            "Observed: {} tokens / {} messages / {} turns / {} compactions",
            report.usage.total_tokens,
            report.usage.messages,
            report.usage.turns,
            report.usage.compactions
        );
        let _ = writeln!(
            out,
            "Input {} · output {} · cache write {} · cache read {} · 1h writes {}",
            report.usage.input_tokens,
            report.usage.output_tokens,
            report.usage.cache_write_tokens,
            report.usage.cache_read_tokens,
            report.usage.cache_write_1h_tokens
        );
        let _ = writeln!(
            out,
            "Baseline: {} active windows / {} messages; median {} tokens",
            report.baseline_windows,
            report.baseline_messages,
            report
                .baseline_median_tokens
                .map_or_else(|| "unavailable".into(), |v| format!("{v:.0}"))
        );
        if let Some(snapshot) = &report.official_snapshot {
            let _ = writeln!(out, "Official account snapshot: {snapshot}");
        }
        for f in &report.findings {
            let _ = writeln!(
                out,
                "- {} [messages {}; baseline windows {}]",
                f.message, f.messages, f.baseline_windows
            );
        }
        let _ = writeln!(
            out,
            "Model / endpoint                            tokens  messages  baseline median"
        );
        for d in &report.dimensions {
            let _ = writeln!(
                out,
                "{} / {}  {}  {}  {}",
                d.model,
                d.endpoint,
                d.usage.total_tokens,
                d.usage.messages,
                d.baseline_median_tokens
                    .map_or_else(|| "unavailable".into(), |v| format!("{v:.0}"))
            );
        }
        let _ = writeln!(out, "Largest sessions (plus every subagent):");
        for (i, s) in report
            .sessions
            .iter()
            .enumerate()
            .filter(|(i, s)| *i < 3 || s.is_subagent)
        {
            let _ = writeln!(
                out,
                "{}. {}{}: {} tokens / {} messages, {} — {}; compactions {}{}",
                i + 1,
                s.session_id,
                if s.is_subagent { " [subagent]" } else { "" },
                s.usage.total_tokens,
                s.usage.messages,
                s.first_usage_at,
                s.last_usage_at,
                s.usage.compactions,
                if s.compaction_times.is_empty() {
                    String::new()
                } else {
                    format!(" at {}", s.compaction_times.join(", "))
                }
            );
        }
    }
    let _ = writeln!(
        out,
        "Version comparison (same exact model/endpoint; completed-turn median):"
    );
    for v in &report.versions {
        let _ = writeln!(
            out,
            "{} / {} / {}: {:.0} cache-write tokens/turn · {} turns / {} messages · {}{}",
            v.model,
            v.endpoint,
            v.version,
            v.median_cache_write_per_turn,
            v.complete_turns,
            v.messages,
            v.assessment,
            v.change_pct
                .map_or_else(String::new, |pct| format!(" ({pct:+.1}%)"))
        );
    }
    for note in &report.notes {
        let _ = writeln!(out, "Note: {note}");
    }
    out
}

pub(crate) fn handle(ctx: &CommandContext<'_>) {
    let Some(Commands::Diagnose {
        window,
        session,
        versions,
    }) = &ctx.cli.command
    else {
        return;
    };
    if ctx.cli.csv || ctx.cli.since.is_some() || ctx.cli.until.is_some() || ctx.devices.is_some() {
        eprintln!(
            "Error: diagnose supports text/JSON local windows; use --window/--session, without --csv, --since/--until or --devices"
        );
        std::process::exit(1);
    }
    match report(ctx, *window, session.as_deref()) {
        Ok(report) => {
            if ctx.cli.json {
                match serde_json::to_string_pretty(&report) {
                    Ok(json) => print_json(&json, ctx.jq_filter),
                    Err(e) => {
                        eprintln!("Error: {e}");
                        std::process::exit(1);
                    }
                }
            } else {
                print!("{}", text(&report, *versions));
            }
        }
        Err(error) => {
            eprintln!("Error: {error}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
#[path = "diagnose_tests.rs"]
mod tests;
