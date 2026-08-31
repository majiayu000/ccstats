//! Usage sources whose transcripts descend from Pi but have independent accounting rules.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use crate::consts::{DATE_FORMAT, UNKNOWN};
use crate::core::{CostKind, Endpoint, RawEntry, source_wide_message_id};
use crate::source::{Capabilities, ParseOutput, Source};
use crate::utils::Timezone;
use chrono::{DateTime, Utc};

use super::pi_fork_paths::{
    find_gjc_files, find_omp_files, find_prime_files, inherited_project_path,
    linked_child_transcript, parent_transcript,
};
use super::pi_fork_schema::{
    ForkUsage, PrimeAttribution, SessionEntry, SessionHeader, SessionRecord,
    subtract_prime_children,
};

pub(crate) struct GjcSource;
pub(crate) struct PrimeSource;
pub(crate) struct OmpSource;

impl GjcSource {
    pub(crate) const fn new() -> Self {
        Self
    }
}

impl PrimeSource {
    pub(crate) const fn new() -> Self {
        Self
    }
}

impl OmpSource {
    pub(crate) const fn new() -> Self {
        Self
    }
}

#[derive(Clone, Copy)]
struct ForkProfile {
    source: &'static str,
    display_name: &'static str,
    has_reasoning: bool,
    session_version: u32,
    record_zero_cost: bool,
}

impl ForkProfile {
    const fn gjc() -> Self {
        Self {
            source: "gjc",
            display_name: "Gajae Code",
            has_reasoning: true,
            session_version: 5,
            record_zero_cost: true,
        }
    }

    const fn prime() -> Self {
        Self {
            source: "prime",
            display_name: "Prime Agent",
            has_reasoning: false,
            session_version: 3,
            record_zero_cost: false,
        }
    }

    const fn omp() -> Self {
        Self {
            source: "omp",
            display_name: "Oh My Pi",
            has_reasoning: true,
            session_version: 3,
            record_zero_cost: false,
        }
    }
}

fn capabilities(has_reasoning: bool) -> Capabilities {
    Capabilities {
        has_projects: true,
        has_billing_blocks: false,
        has_reasoning_tokens: has_reasoning,
        has_cache_creation: true,
        has_cache_read: true,
        needs_dedup: true,
        has_tool_calls: false,
        has_endpoints: false,
    }
}

macro_rules! source_impl {
    ($source:ty, $name:literal, $display:literal, $aliases:expr, $profile:expr, $find:ident) => {
        impl Source for $source {
            fn name(&self) -> &'static str {
                $name
            }

            fn display_name(&self) -> &'static str {
                $display
            }

            fn aliases(&self) -> &'static [&'static str] {
                $aliases
            }

            fn capabilities(&self) -> Capabilities {
                capabilities($profile.has_reasoning)
            }

            fn find_files(&self) -> Vec<PathBuf> {
                $find()
            }

            fn parse_file(&self, path: &Path, timezone: Timezone, debug: bool) -> ParseOutput {
                parse_file(path, timezone, debug, $profile)
            }
        }
    };
}

source_impl!(
    GjcSource,
    "gjc",
    "Gajae Code",
    &["gajae-code"],
    ForkProfile::gjc(),
    find_gjc_files
);
source_impl!(
    PrimeSource,
    "prime",
    "Prime Agent",
    &["prime-agent"],
    ForkProfile::prime(),
    find_prime_files
);
source_impl!(
    OmpSource,
    "omp",
    "Oh My Pi",
    &["oh-my-pi"],
    ForkProfile::omp(),
    find_omp_files
);

fn parse_timestamp(
    message_timestamp: Option<i64>,
    entry_timestamp: &str,
) -> Result<DateTime<Utc>, &'static str> {
    if let Some(timestamp_ms) = message_timestamp {
        return DateTime::from_timestamp_millis(timestamp_ms).ok_or("invalid message timestamp");
    }
    DateTime::parse_from_rfc3339(entry_timestamp)
        .map(|timestamp| timestamp.with_timezone(&Utc))
        .map_err(|_| "invalid entry timestamp")
}

struct UsageContext<'a> {
    entry_id: &'a str,
    entry_timestamp: &'a str,
    message_timestamp: Option<i64>,
    provider: Option<&'a str>,
    model: Option<&'a str>,
    stop_reason: Option<String>,
    call_count: i64,
}

fn usage_entry(
    mut usage: ForkUsage,
    context: UsageContext<'_>,
    header: &SessionHeader,
    timezone: Timezone,
    profile: ForkProfile,
) -> Result<Option<RawEntry>, &'static str> {
    let normalized = usage.normalize(
        profile.has_reasoning,
        profile.source == "omp",
        profile.record_zero_cost,
    )?;
    if context.call_count == 0
        && usage.input == 0
        && usage.output == 0
        && usage.cache_read == 0
        && usage.cache_write == 0
    {
        return Ok(None);
    }

    let timestamp = parse_timestamp(context.message_timestamp, context.entry_timestamp)?;
    if context.call_count > 0 && context.provider.map(str::trim).is_none_or(str::is_empty) {
        return Err("assistant record is missing provider");
    }
    let model = context
        .model
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .or_else(|| (context.call_count == 0).then_some(UNKNOWN))
        .ok_or("assistant record is missing model")?
        .to_string();
    let identity = format!(
        "{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{:?}",
        context.entry_id,
        timestamp.timestamp_millis(),
        context.provider.unwrap_or_default(),
        context.model.unwrap_or_default(),
        usage.input,
        usage.output,
        usage.cache_read,
        usage.cache_write,
        normalized.reasoning,
        normalized.cache_creation_1h,
        normalized.recorded_cost_usd.map(f64::to_bits)
    );

    Ok(Some(RawEntry {
        timestamp: timestamp.to_rfc3339(),
        timestamp_ms: timestamp.timestamp_millis(),
        date_str: timezone
            .to_fixed_offset(timestamp)
            .date_naive()
            .format(DATE_FORMAT)
            .to_string(),
        message_id: Some(source_wide_message_id(profile.source, &identity)),
        session_key: format!("{}::{}", profile.source, header.id),
        session_id: header.id.clone(),
        project_path: header.cwd.clone(),
        model,
        input_tokens: usage.input,
        output_tokens: usage.output - normalized.reasoning,
        cache_creation: usage.cache_write,
        cache_creation_1h: normalized.cache_creation_1h,
        cache_read: usage.cache_read,
        reasoning_tokens: normalized.reasoning,
        stop_reason: context.stop_reason,
        cost_kind: CostKind::Real,
        endpoint: Endpoint::Unknown,
        call_count: context.call_count,
        reported_total_tokens: None,
        recorded_cost_usd: normalized.recorded_cost_usd,
    }))
}

fn assistant_entry(
    entry: &SessionEntry,
    usage: ForkUsage,
    header: &SessionHeader,
    timezone: Timezone,
    profile: ForkProfile,
) -> Result<Option<RawEntry>, &'static str> {
    usage_entry(
        usage,
        UsageContext {
            entry_id: &entry.id,
            entry_timestamp: &entry.timestamp,
            message_timestamp: entry.message.timestamp,
            provider: entry.message.provider.as_deref(),
            model: entry.message.model.as_deref(),
            stop_reason: entry
                .message
                .stop_reason
                .clone()
                .filter(|reason| !reason.trim().is_empty())
                .or_else(|| Some("assistant".to_string())),
            call_count: 1,
        },
        header,
        timezone,
        profile,
    )
}

#[derive(Default)]
struct UsageTotals {
    input: i64,
    output: i64,
    cache_read: i64,
    cache_write: i64,
    cost: f64,
    all_costs_recorded: bool,
}

impl UsageTotals {
    fn add_usage(&mut self, mut usage: ForkUsage) -> Result<(), &'static str> {
        let normalized = usage.normalize(true, false, true)?;
        self.input = self
            .input
            .checked_add(usage.input)
            .ok_or("child usage overflow")?;
        self.output = self
            .output
            .checked_add(usage.output)
            .ok_or("child usage overflow")?;
        self.cache_read = self
            .cache_read
            .checked_add(usage.cache_read)
            .ok_or("child usage overflow")?;
        self.cache_write = self
            .cache_write
            .checked_add(usage.cache_write)
            .ok_or("child usage overflow")?;
        self.cost += normalized.recorded_cost_usd.unwrap_or_default();
        if !self.cost.is_finite() {
            return Err("child cost overflow");
        }
        Ok(())
    }

    fn add_entry(&mut self, entry: &RawEntry) -> Result<(), &'static str> {
        self.input = self
            .input
            .checked_add(entry.input_tokens)
            .ok_or("child usage overflow")?;
        self.output = self
            .output
            .checked_add(entry.output_tokens)
            .and_then(|value| value.checked_add(entry.reasoning_tokens))
            .ok_or("child usage overflow")?;
        self.cache_read = self
            .cache_read
            .checked_add(entry.cache_read)
            .ok_or("child usage overflow")?;
        self.cache_write = self
            .cache_write
            .checked_add(entry.cache_creation)
            .ok_or("child usage overflow")?;
        self.cost += entry.recorded_cost_usd.unwrap_or_default();
        self.all_costs_recorded &= entry.recorded_cost_usd.is_some();
        if !self.cost.is_finite() {
            return Err("child cost overflow");
        }
        Ok(())
    }
}

fn safe_child_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn task_residual_entry(
    path: &Path,
    entry: &SessionEntry,
    header: &SessionHeader,
    timezone: Timezone,
) -> Result<Option<RawEntry>, &'static str> {
    if entry.message.role != "toolResult" || entry.message.tool_name.as_deref() != Some("task") {
        return Ok(None);
    }
    let Some(details) = entry.message.details.as_ref() else {
        return Ok(None);
    };
    let Some(mut residual) = details.usage.clone() else {
        return Ok(None);
    };
    let mut totals = UsageTotals {
        all_costs_recorded: true,
        ..UsageTotals::default()
    };
    let mut seen_results = HashSet::new();
    for result in &details.results {
        if !safe_child_id(&result.id) {
            return Err("invalid task child id");
        }
        if !seen_results.insert(result.id.clone()) {
            continue;
        }
        let Some(child_path) =
            linked_child_transcript(path, header.parent_session.as_deref(), &result.id)
        else {
            continue;
        };
        let child_output = parse_file(&child_path, timezone, false, ForkProfile::gjc());
        let child_has_errors = child_output.errors > 0;
        let child_entries = child_output
            .entries
            .into_iter()
            .filter(|entry| entry.call_count > 0)
            .collect::<Vec<_>>();
        if child_entries.is_empty() {
            continue;
        }
        if !child_has_errors && let Some(usage) = result.usage.clone() {
            totals.add_usage(usage)?;
        } else {
            for child_entry in child_entries {
                totals.add_entry(&child_entry)?;
            }
        }
    }
    if totals.input > residual.input
        || totals.output > residual.output
        || totals.cache_read > residual.cache_read
        || totals.cache_write > residual.cache_write
    {
        return Err("task child usage exceeds parent rollup");
    }
    residual.input -= totals.input;
    residual.output -= totals.output;
    residual.cache_read -= totals.cache_read;
    residual.cache_write -= totals.cache_write;
    if !totals.all_costs_recorded {
        residual.cost = None;
    } else if let Some(cost) = residual.cost.as_mut() {
        if totals.cost > cost.total {
            return Err("task child cost exceeds parent rollup");
        }
        cost.total -= totals.cost;
    }
    residual.reasoning_tokens = 0;
    residual.cttl = None;
    let identity = format!("task-residual:{}", entry.id);
    usage_entry(
        residual,
        UsageContext {
            entry_id: &identity,
            entry_timestamp: &entry.timestamp,
            message_timestamp: entry.message.timestamp,
            provider: None,
            model: None,
            stop_reason: Some("task_residual".to_string()),
            call_count: 0,
        },
        header,
        timezone,
        ForkProfile::gjc(),
    )
}

fn prime_ancestor_usage(
    path: &Path,
    parent_session: &str,
    entry_id: &str,
) -> Result<Option<ForkUsage>, &'static str> {
    let Some(mut ancestor) = parent_transcript(path, parent_session) else {
        return Ok(None);
    };
    for _ in 0..32 {
        let content = fs::read_to_string(&ancestor).map_err(|_| "failed to read parent session")?;
        let mut lines = content.lines().filter(|line| !line.trim().is_empty());
        let header =
            serde_json::from_str::<SessionHeader>(lines.next().ok_or("parent session is empty")?)
                .map_err(|_| "invalid parent session header")?;
        if header.entry_type != "session" || header.version != Some(3) {
            return Err("invalid parent session header");
        }
        let mut usage = None;
        let mut attribution = PrimeAttribution::default();
        for line in lines {
            let record = serde_json::from_str::<SessionRecord>(line)
                .map_err(|_| "invalid parent session record")?;
            match record {
                SessionRecord::Message { id, message, .. } if id == entry_id => {
                    usage = message.usage;
                }
                SessionRecord::ChildUsageAttributed {
                    target_id,
                    child_usage,
                    aggregate_usage,
                } if target_id == entry_id => {
                    attribution.aggregate = Some(aggregate_usage);
                    attribution.children.push(child_usage);
                }
                _ => {}
            }
        }
        if let Some(aggregate) = attribution.aggregate {
            return subtract_prime_children(aggregate, &attribution.children).map(Some);
        }
        let Some(parent) = header.parent_session else {
            return Ok(usage);
        };
        let Some(parent_path) = parent_transcript(&ancestor, &parent) else {
            return Ok(usage);
        };
        ancestor = parent_path;
    }
    Err("parent session lineage is too deep")
}

fn validate_omp_task_children(
    path: &Path,
    entry: &SessionEntry,
    header: &SessionHeader,
    timezone: Timezone,
) -> Result<(), &'static str> {
    if entry.message.role != "toolResult" || entry.message.tool_name.as_deref() != Some("task") {
        return Ok(());
    }
    let Some(details) = entry.message.details.as_ref() else {
        return Ok(());
    };
    if details.results.is_empty() && details.usage.as_ref().is_some_and(ForkUsage::has_usage) {
        return Err("task rollup has no child transcript references");
    }
    for result in &details.results {
        if !safe_child_id(&result.id) {
            return Err("invalid task child id");
        }
        if !result.usage.as_ref().is_some_and(ForkUsage::has_usage) {
            continue;
        }
        let child = linked_child_transcript(path, header.parent_session.as_deref(), &result.id)
            .ok_or("task child transcript is missing")?;
        if parse_file(&child, timezone, false, ForkProfile::omp())
            .entries
            .is_empty()
        {
            return Err("task child transcript has no accountable usage");
        }
    }
    Ok(())
}

struct ReplayedSession {
    entries: Vec<SessionEntry>,
    prime_attributions: HashMap<String, PrimeAttribution>,
    output: ParseOutput,
}

fn replay_records(
    lines: impl Iterator<Item = (usize, std::io::Result<String>)>,
    path: &Path,
    debug: bool,
    profile: ForkProfile,
    header: &mut SessionHeader,
) -> ReplayedSession {
    let mut output = ParseOutput::default();
    let mut entries = Vec::<SessionEntry>::new();
    let mut entry_indices = HashMap::<String, usize>::new();
    let mut prime_attributions = HashMap::<String, PrimeAttribution>::new();
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
        let record = match serde_json::from_str::<SessionRecord>(&line) {
            Ok(record) => record,
            Err(error) => {
                output.errors += 1;
                if debug {
                    eprintln!(
                        "Invalid {} JSON in {} line {}: {error}",
                        profile.display_name,
                        path.display(),
                        line_index + 1
                    );
                }
                continue;
            }
        };
        match record {
            SessionRecord::Message {
                id,
                timestamp,
                message,
            } => {
                let index = entries.len();
                entry_indices.insert(id.clone(), index);
                entries.push(SessionEntry {
                    id,
                    timestamp,
                    message,
                });
            }
            SessionRecord::HeaderPatch { patch } if profile.source == "gjc" => {
                if let Some(cwd) = patch.cwd.filter(|cwd| !cwd.trim().is_empty()) {
                    header.cwd = cwd;
                }
            }
            SessionRecord::EntryPatch { entry_id, patch } if profile.source == "gjc" => {
                if let (Some(index), Some(message)) = (entry_indices.get(&entry_id), patch.message)
                {
                    entries[*index].message = message;
                }
            }
            SessionRecord::ChildUsageAttributed {
                target_id,
                child_usage,
                aggregate_usage,
            } if profile.source == "prime" => {
                let attribution = prime_attributions.entry(target_id).or_default();
                attribution.aggregate = Some(aggregate_usage);
                attribution.children.push(child_usage);
            }
            SessionRecord::HeaderPatch { .. }
            | SessionRecord::EntryPatch { .. }
            | SessionRecord::ChildUsageAttributed { .. }
            | SessionRecord::Other => {}
        }
    }
    ReplayedSession {
        entries,
        prime_attributions,
        output,
    }
}

fn emit_entries(
    path: &Path,
    replayed: ReplayedSession,
    header: &SessionHeader,
    timezone: Timezone,
    debug: bool,
    profile: ForkProfile,
) -> ParseOutput {
    let mut output = replayed.output;
    for entry in replayed.entries {
        let result = if entry.message.role == "assistant" {
            let usage = if profile.source == "prime" {
                if let Some(attribution) = replayed.prime_attributions.get(&entry.id)
                    && let Some(aggregate) = attribution.aggregate.clone()
                {
                    subtract_prime_children(aggregate, &attribution.children).map(Some)
                } else if let Some(parent) = header.parent_session.as_deref() {
                    match prime_ancestor_usage(path, parent, &entry.id) {
                        Ok(usage) => Ok(usage.or_else(|| entry.message.usage.clone())),
                        Err(error) => {
                            output.errors += 1;
                            if debug {
                                eprintln!(
                                    "Invalid {} ancestor usage in {}: {error}",
                                    profile.display_name,
                                    path.display()
                                );
                            }
                            Ok(entry.message.usage.clone())
                        }
                    }
                } else {
                    Ok(entry.message.usage.clone())
                }
            } else {
                Ok(entry.message.usage.clone())
            };
            usage.and_then(|usage| {
                usage.map_or(Ok(None), |usage| {
                    assistant_entry(&entry, usage, header, timezone, profile)
                })
            })
        } else if profile.source == "gjc" {
            task_residual_entry(path, &entry, header, timezone)
        } else if profile.source == "omp" {
            validate_omp_task_children(path, &entry, header, timezone).map(|()| None)
        } else {
            Ok(None)
        };
        match result {
            Ok(Some(raw)) => output.entries.push(raw),
            Ok(None) => {}
            Err(error) => {
                output.errors += 1;
                if debug {
                    eprintln!(
                        "Invalid {} usage in {}: {error}",
                        profile.display_name,
                        path.display()
                    );
                }
            }
        }
    }
    output
}

fn parse_file(path: &Path, timezone: Timezone, debug: bool, profile: ForkProfile) -> ParseOutput {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) => {
            if debug {
                eprintln!(
                    "Failed to open {} session {}: {error}",
                    profile.display_name,
                    path.display()
                );
            }
            return ParseOutput {
                entries: Vec::new(),
                errors: 1,
            };
        }
    };
    let mut lines = BufReader::new(file).lines().enumerate();
    let mut header = loop {
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
        match serde_json::from_str::<SessionHeader>(&line) {
            Ok(header)
                if header.entry_type == "session"
                    && header.version == Some(profile.session_version)
                    && !header.id.trim().is_empty()
                    && !header.cwd.trim().is_empty() =>
            {
                break header;
            }
            Ok(_) | Err(_) => {
                if debug {
                    eprintln!(
                        "Invalid {} session header in {}",
                        profile.display_name,
                        path.display()
                    );
                }
                return ParseOutput {
                    entries: Vec::new(),
                    errors: 1,
                };
            }
        }
    };

    let replayed = replay_records(lines, path, debug, profile, &mut header);
    if profile.source == "gjc"
        && let Some(project) = inherited_project_path(path, header.parent_session.as_deref())
    {
        header.cwd = project;
    }
    emit_entries(path, replayed, &header, timezone, debug, profile)
}

#[cfg(test)]
#[path = "pi_forks_tests.rs"]
mod tests;
