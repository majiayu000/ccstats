use chrono::{DateTime, Duration, FixedOffset, NaiveDate, TimeZone, Timelike, Utc};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::UNIX_EPOCH;

use crate::utils::Timezone;

use super::types::{BlockStats, DayStats, ParsedEntry, ProjectStats, SessionStats, Stats, Usage, UsageEntry};

#[derive(Debug)]
struct ParsedEntryWithLocal {
    message_id: Option<String>,
    parsed: ParsedEntry,
    local_dt: DateTime<FixedOffset>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RawEntry {
    timestamp: String,
    model: String,
    usage: Usage,
    stop_reason: Option<String>,
    message_id: Option<String>,
    session_id: String,
    project_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedFile {
    mtime: i64,
    size: u64,
    entries: Vec<RawEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct EntriesCache {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    files: HashMap<String, CachedFile>,
}

struct FileEntries {
    key: String,
    entries: Vec<RawEntry>,
    mtime: Option<i64>,
    size: Option<u64>,
    from_cache: bool,
}

trait EntryMeta {
    fn timestamp_ms(&self) -> i64;
    fn has_stop_reason(&self) -> bool;
}

impl EntryMeta for ParsedEntry {
    fn timestamp_ms(&self) -> i64 {
        self.timestamp_ms
    }

    fn has_stop_reason(&self) -> bool {
        self.stop_reason.is_some()
    }
}

#[derive(Debug, Clone)]
struct CandidateState<T: EntryMeta + Clone> {
    completed: Option<T>,
    latest: T,
}

impl<T: EntryMeta + Clone> CandidateState<T> {
    fn new(entry: T) -> Self {
        let completed = if entry.has_stop_reason() {
            Some(entry.clone())
        } else {
            None
        };
        Self {
            completed,
            latest: entry,
        }
    }

    fn update(&mut self, entry: T) {
        if entry.has_stop_reason() {
            match &self.completed {
                Some(existing) => {
                    if entry.timestamp_ms() > existing.timestamp_ms() {
                        self.completed = Some(entry.clone());
                    }
                }
                None => self.completed = Some(entry.clone()),
            }
        }

        if entry.timestamp_ms() > self.latest.timestamp_ms() {
            self.latest = entry;
        }
    }

    fn merge(&mut self, other: CandidateState<T>) {
        if let Some(other_completed) = other.completed {
            match &self.completed {
                Some(existing) => {
                    if other_completed.timestamp_ms() > existing.timestamp_ms() {
                        self.completed = Some(other_completed);
                    }
                }
                None => self.completed = Some(other_completed),
            }
        }

        if other.latest.timestamp_ms() > self.latest.timestamp_ms() {
            self.latest = other.latest;
        }
    }

    fn finalize(self) -> T {
        self.completed.unwrap_or(self.latest)
    }
}

fn stats_from_usage(usage: &Usage) -> Stats {
    Stats {
        input_tokens: usage.input_tokens.unwrap_or(0),
        output_tokens: usage.output_tokens.unwrap_or(0),
        cache_creation: usage.cache_creation_input_tokens.unwrap_or(0),
        cache_read: usage.cache_read_input_tokens.unwrap_or(0),
        count: 1,
        skipped_chunks: 0,
    }
}

fn add_entry_to_day_stats(day_stats: &mut HashMap<String, DayStats>, entry: &ParsedEntry) {
    let stats = stats_from_usage(&entry.usage);
    let day = day_stats.entry(entry.date_str.clone()).or_default();
    day.stats.add(&stats);
    day.models
        .entry(entry.model.clone())
        .or_default()
        .add(&stats);
}

fn merge_day_stats(target: &mut HashMap<String, DayStats>, source: HashMap<String, DayStats>) {
    for (date, stats) in source {
        let day = target.entry(date).or_default();
        day.stats.add(&stats.stats);
        for (model, model_stats) in stats.models {
            day.models.entry(model).or_default().add(&model_stats);
        }
    }
}

fn parse_raw_line(line: &str, session_id: &str, project_path: &str) -> Option<RawEntry> {
    if line.trim().is_empty() {
        return None;
    }

    let entry: UsageEntry = serde_json::from_str(line).ok()?;
    let ts = entry.timestamp?;

    let msg = entry.message?;
    let usage = msg.usage?;

    let model = msg
        .model
        .as_deref()
        .map(normalize_model_name)
        .unwrap_or_else(|| "unknown".to_string());

    if model == "<synthetic>" || model.is_empty() {
        return None;
    }

    Some(RawEntry {
        timestamp: ts,
        model,
        usage,
        stop_reason: msg.stop_reason,
        message_id: msg.id,
        session_id: session_id.to_string(),
        project_path: project_path.to_string(),
    })
}

fn raw_to_parsed(
    entry: &RawEntry,
    since: Option<NaiveDate>,
    until: Option<NaiveDate>,
    timezone: &Timezone,
) -> Option<ParsedEntryWithLocal> {
    let utc_dt = entry.timestamp.parse::<DateTime<Utc>>().ok()?;
    let local_dt = timezone.to_fixed_offset(utc_dt);
    let date = local_dt.date_naive();

    if let Some(s) = since {
        if date < s {
            return None;
        }
    }
    if let Some(u) = until {
        if date > u {
            return None;
        }
    }

    let date_str = date.format("%Y-%m-%d").to_string();
    let parsed = ParsedEntry {
        date_str,
        timestamp: entry.timestamp.clone(),
        timestamp_ms: utc_dt.timestamp_millis(),
        model: entry.model.clone(),
        usage: entry.usage.clone(),
        stop_reason: entry.stop_reason.clone(),
    };

    Some(ParsedEntryWithLocal {
        message_id: entry.message_id.clone(),
        parsed,
        local_dt,
    })
}

pub fn normalize_model_name(model: &str) -> String {
    // Remove "anthropic." and "claude-" prefixes
    let name = model
        .strip_prefix("anthropic.")
        .unwrap_or(model)
        .to_string();
    let name = name.strip_prefix("claude-").unwrap_or(&name).to_string();

    // Remove date suffix like -20251101, -20250929, etc.
    // Pattern: -YYYYMMDD at the end
    if let Some(pos) = name.rfind('-') {
        let suffix = &name[pos + 1..];
        if suffix.len() == 8 && suffix.chars().all(|c| c.is_ascii_digit()) {
            return name[..pos].to_string();
        }
    }

    name
}

pub fn find_jsonl_files() -> Vec<PathBuf> {
    let Some(home) = dirs::home_dir() else {
        eprintln!("Warning: Cannot find home directory. No data will be loaded.");
        return Vec::new();
    };
    let claude_path = home.join(".claude").join("projects");

    let mut files = Vec::new();
    if let Ok(entries) = glob::glob(&format!("{}/**/*.jsonl", claude_path.display())) {
        for entry in entries.flatten() {
            files.push(entry);
        }
    }
    files
}

fn get_entries_cache_path() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    Some(home.join(".cache").join("ccstats").join("entries.json"))
}

fn load_entries_cache() -> EntriesCache {
    let Some(path) = get_entries_cache_path() else {
        return EntriesCache::default();
    };
    let file = match File::open(&path) {
        Ok(file) => file,
        Err(_) => return EntriesCache::default(),
    };
    match serde_json::from_reader(file) {
        Ok(cache) => cache,
        Err(_) => EntriesCache::default(),
    }
}

fn file_meta(path: &PathBuf) -> Option<(i64, u64)> {
    let meta = std::fs::metadata(path).ok()?;
    let mtime = meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_secs() as i64;
    Some((mtime, meta.len()))
}

fn parse_file_to_raw_entries(path: &PathBuf) -> Vec<RawEntry> {
    let session_id = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();

    let project_path = path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();

    let file = match File::open(path) {
        Ok(f) => f,
        Err(_) => return Vec::new(),
    };
    let reader = BufReader::new(file);

    let mut entries = Vec::new();
    for line in reader.lines().flatten() {
        if let Some(entry) = parse_raw_line(&line, &session_id, &project_path) {
            entries.push(entry);
        }
    }
    entries
}

fn load_file_entries(
    files: &[PathBuf],
    quiet: bool,
    debug: bool,
) -> (Vec<FileEntries>, Option<PathBuf>, usize, usize) {
    let cache = load_entries_cache();
    let cached_files = Arc::new(cache.files);

    let entries: Vec<FileEntries> = files
        .par_iter()
        .map(|path| {
            let key = path.to_string_lossy().to_string();
            let meta = file_meta(path);
            if let Some((mtime, size)) = meta {
                if let Some(cached) = cached_files.get(&key) {
                    if cached.mtime == mtime && cached.size == size {
                        return FileEntries {
                            key,
                            entries: cached.entries.clone(),
                            mtime: Some(mtime),
                            size: Some(size),
                            from_cache: true,
                        };
                    }
                }
            }

            let parsed_entries = parse_file_to_raw_entries(path);
            FileEntries {
                key,
                entries: parsed_entries,
                mtime: meta.map(|m| m.0),
                size: meta.map(|m| m.1),
                from_cache: false,
            }
        })
        .collect();

    let cache_hits = entries.iter().filter(|e| e.from_cache).count();
    let cache_misses = entries.len().saturating_sub(cache_hits);

    if debug && !quiet {
        eprintln!(
            "[DEBUG] Cache: {} hits, {} misses",
            cache_hits, cache_misses
        );
    }

    let cache_path = get_entries_cache_path();
    (entries, cache_path, cache_hits, cache_misses)
}

fn save_file_entries_cache(entries: Vec<FileEntries>, cache_path: Option<PathBuf>) {
    let Some(path) = cache_path else {
        return;
    };

    let mut files = HashMap::new();
    for entry in entries {
        if let (Some(mtime), Some(size)) = (entry.mtime, entry.size) {
            files.insert(
                entry.key,
                CachedFile {
                    mtime,
                    size,
                    entries: entry.entries,
                },
            );
        }
    }

    let cache = EntriesCache { version: 1, files };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut file) = File::create(&path) {
        let _ = serde_json::to_writer(&mut file, &cache);
    }
}

#[derive(Default)]
struct UsageAggregate {
    total_candidates: i64,
    total_with_id: i64,
    no_id_valid: i64,
    message_map: HashMap<String, CandidateState<ParsedEntry>>,
    no_id_day_stats: HashMap<String, DayStats>,
}

impl UsageAggregate {
    fn merge(mut self, other: UsageAggregate) -> UsageAggregate {
        self.total_candidates += other.total_candidates;
        self.total_with_id += other.total_with_id;
        self.no_id_valid += other.no_id_valid;

        for (id, state) in other.message_map {
            match self.message_map.get_mut(&id) {
                Some(existing) => existing.merge(state),
                None => {
                    self.message_map.insert(id, state);
                }
            }
        }

        merge_day_stats(&mut self.no_id_day_stats, other.no_id_day_stats);

        self
    }
}

fn process_entries_for_usage(
    entries: &[RawEntry],
    since: Option<NaiveDate>,
    until: Option<NaiveDate>,
    timezone: &Timezone,
) -> UsageAggregate {
    let mut aggregate = UsageAggregate::default();

    for entry in entries {
        let parsed = match raw_to_parsed(entry, since, until, timezone) {
            Some(p) => p,
            None => continue,
        };

        aggregate.total_candidates += 1;

        if let Some(id) = parsed.message_id {
            aggregate.total_with_id += 1;
            let entry = parsed.parsed;
            match aggregate.message_map.get_mut(&id) {
                Some(state) => state.update(entry),
                None => {
                    aggregate
                        .message_map
                        .insert(id, CandidateState::new(entry));
                }
            }
        } else if parsed.parsed.stop_reason.is_some() {
            aggregate.no_id_valid += 1;
            add_entry_to_day_stats(&mut aggregate.no_id_day_stats, &parsed.parsed);
        }
    }

    aggregate
}

pub fn load_usage_data_with_debug(
    since: Option<NaiveDate>,
    until: Option<NaiveDate>,
    debug: bool,
    timezone: &Timezone,
) -> (HashMap<String, DayStats>, i64, i64) {
    load_usage_data_internal(since, until, debug, false, timezone)
}

pub fn load_usage_data_quiet(
    since: Option<NaiveDate>,
    until: Option<NaiveDate>,
    timezone: &Timezone,
) -> (HashMap<String, DayStats>, i64, i64) {
    load_usage_data_internal(since, until, false, true, timezone)
}

fn load_usage_data_internal(
    since: Option<NaiveDate>,
    until: Option<NaiveDate>,
    debug: bool,
    quiet: bool,
    timezone: &Timezone,
) -> (HashMap<String, DayStats>, i64, i64) {
    if !quiet {
        if debug {
            eprintln!("[DEBUG] Scanning JSONL files...");
        } else {
            eprintln!("Scanning JSONL files...");
        }
    }

    let files = find_jsonl_files();

    if !quiet {
        if debug {
            eprintln!("[DEBUG] Found {} files", files.len());
            eprintln!("[DEBUG] Date filter: since={:?}, until={:?}", since, until);
        } else {
            eprintln!("Found {} files", files.len());
        }
    }

    if !quiet {
        if debug {
            eprintln!("[DEBUG] Processing files in parallel...");
        } else {
            eprintln!("Processing...");
        }
    }

    let (file_entries, cache_path, _cache_hits, _cache_misses) =
        load_file_entries(&files, quiet, debug);

    let aggregate = file_entries
        .par_iter()
        .map(|f| process_entries_for_usage(&f.entries, since, until, timezone))
        .reduce(UsageAggregate::default, |a, b| a.merge(b));

    let unique_messages = aggregate.message_map.len() as i64;
    let mut day_stats = aggregate.no_id_day_stats;
    for (_id, state) in aggregate.message_map {
        let entry = state.finalize();
        add_entry_to_day_stats(&mut day_stats, &entry);
    }
    let valid = unique_messages + aggregate.no_id_valid;
    let skipped = (aggregate.total_with_id - unique_messages).max(0);

    if debug && !quiet {
        eprintln!("[DEBUG] Processing complete:");
        eprintln!("[DEBUG]   - Total candidate entries: {}", aggregate.total_candidates);
        eprintln!("[DEBUG]   - Unique message IDs: {}", unique_messages);
        eprintln!("[DEBUG]   - No-id completed entries: {}", aggregate.no_id_valid);
        eprintln!("[DEBUG]   - Streaming entries deduplicated: {}", skipped);
        eprintln!("[DEBUG]   - Days with data: {}", day_stats.len());

        let mut model_counts: HashMap<String, i64> = HashMap::new();
        for (_date, stats) in &day_stats {
            for (model, stats) in &stats.models {
                *model_counts.entry(model.clone()).or_default() += stats.count;
            }
        }
        eprintln!("[DEBUG]   - Models used:");
        let mut models: Vec<_> = model_counts.iter().collect();
        models.sort_by(|a, b| b.1.cmp(a.1));
        for (model, count) in models {
            eprintln!("[DEBUG]       {} ({} calls)", model, count);
        }
    }

    save_file_entries_cache(file_entries, cache_path);

    (day_stats, skipped, valid)
}

#[derive(Debug, Clone)]
struct SessionEntry {
    session_id: String,
    project_path: String,
    parsed: ParsedEntry,
}

impl EntryMeta for SessionEntry {
    fn timestamp_ms(&self) -> i64 {
        self.parsed.timestamp_ms
    }

    fn has_stop_reason(&self) -> bool {
        self.parsed.stop_reason.is_some()
    }
}

#[derive(Debug, Default)]
struct SessionAccumulator {
    session_id: String,
    project_path: String,
    first_timestamp: String,
    last_timestamp: String,
    first_timestamp_ms: i64,
    last_timestamp_ms: i64,
    stats: Stats,
    models: HashMap<String, Stats>,
}

impl SessionAccumulator {
    fn new(session_id: String, project_path: String, timestamp: &str, timestamp_ms: i64) -> Self {
        SessionAccumulator {
            session_id,
            project_path,
            first_timestamp: timestamp.to_string(),
            last_timestamp: timestamp.to_string(),
            first_timestamp_ms: timestamp_ms,
            last_timestamp_ms: timestamp_ms,
            stats: Stats::default(),
            models: HashMap::new(),
        }
    }

    fn add_entry(&mut self, entry: &ParsedEntry) {
        let stats = stats_from_usage(&entry.usage);
        self.stats.add(&stats);
        self.models
            .entry(entry.model.clone())
            .or_default()
            .add(&stats);
        self.update_timestamps(&entry.timestamp, entry.timestamp_ms);
    }

    fn update_timestamps(&mut self, timestamp: &str, timestamp_ms: i64) {
        if self.first_timestamp.is_empty() || timestamp_ms < self.first_timestamp_ms {
            self.first_timestamp = timestamp.to_string();
            self.first_timestamp_ms = timestamp_ms;
        }
        if self.last_timestamp.is_empty() || timestamp_ms > self.last_timestamp_ms {
            self.last_timestamp = timestamp.to_string();
            self.last_timestamp_ms = timestamp_ms;
        }
    }

    fn merge(&mut self, other: SessionAccumulator) {
        self.stats.add(&other.stats);
        for (model, model_stats) in other.models {
            self.models.entry(model).or_default().add(&model_stats);
        }
        self.update_timestamps(&other.first_timestamp, other.first_timestamp_ms);
        self.update_timestamps(&other.last_timestamp, other.last_timestamp_ms);
    }
}

impl From<SessionAccumulator> for SessionStats {
    fn from(value: SessionAccumulator) -> Self {
        SessionStats {
            session_id: value.session_id,
            project_path: value.project_path,
            first_timestamp: value.first_timestamp,
            last_timestamp: value.last_timestamp,
            stats: value.stats,
            models: value.models,
        }
    }
}

#[derive(Default)]
struct SessionAggregate {
    total_with_id: i64,
    message_map: HashMap<String, CandidateState<SessionEntry>>,
    no_id_sessions: HashMap<String, SessionAccumulator>,
}

impl SessionAggregate {
    fn merge(mut self, other: SessionAggregate) -> SessionAggregate {
        self.total_with_id += other.total_with_id;
        for (id, state) in other.message_map {
            match self.message_map.get_mut(&id) {
                Some(existing) => existing.merge(state),
                None => {
                    self.message_map.insert(id, state);
                }
            }
        }

        for (session_id, session) in other.no_id_sessions {
            match self.no_id_sessions.get_mut(&session_id) {
                Some(existing) => existing.merge(session),
                None => {
                    self.no_id_sessions.insert(session_id, session);
                }
            }
        }

        self
    }
}

fn process_entries_for_session(
    entries: &[RawEntry],
    since: Option<NaiveDate>,
    until: Option<NaiveDate>,
    timezone: &Timezone,
) -> SessionAggregate {
    let mut aggregate = SessionAggregate::default();

    for entry in entries {
        let parsed = match raw_to_parsed(entry, since, until, timezone) {
            Some(p) => p,
            None => continue,
        };

        if let Some(id) = parsed.message_id {
            aggregate.total_with_id += 1;
            let entry = SessionEntry {
                session_id: entry.session_id.clone(),
                project_path: entry.project_path.clone(),
                parsed: parsed.parsed,
            };
            match aggregate.message_map.get_mut(&id) {
                Some(state) => state.update(entry),
                None => {
                    aggregate
                        .message_map
                        .insert(id, CandidateState::new(entry));
                }
            }
        } else if parsed.parsed.stop_reason.is_some() {
            let session = aggregate
                .no_id_sessions
                .entry(entry.session_id.clone())
                .or_insert_with(|| {
                    SessionAccumulator::new(
                        entry.session_id.clone(),
                        entry.project_path.clone(),
                        &parsed.parsed.timestamp,
                        parsed.parsed.timestamp_ms,
                    )
                });
            session.add_entry(&parsed.parsed);
        }
    }

    aggregate
}

pub fn load_session_data(
    since: Option<NaiveDate>,
    until: Option<NaiveDate>,
    quiet: bool,
    timezone: &Timezone,
) -> Vec<SessionStats> {
    if !quiet {
        eprintln!("Scanning JSONL files...");
    }

    let files = find_jsonl_files();

    if !quiet {
        eprintln!("Found {} files", files.len());
        eprintln!("Processing sessions...");
    }

    let (file_entries, cache_path, _cache_hits, _cache_misses) =
        load_file_entries(&files, quiet, false);

    let aggregate = file_entries
        .par_iter()
        .map(|f| process_entries_for_session(&f.entries, since, until, timezone))
        .reduce(SessionAggregate::default, |a, b| a.merge(b));

    let mut sessions = aggregate.no_id_sessions;

    for (_id, state) in aggregate.message_map {
        let entry = state.finalize();
        let session = sessions
            .entry(entry.session_id.clone())
            .or_insert_with(|| {
                SessionAccumulator::new(
                    entry.session_id.clone(),
                    entry.project_path.clone(),
                    &entry.parsed.timestamp,
                    entry.parsed.timestamp_ms,
                )
            });
        session.add_entry(&entry.parsed);
    }

    let result: Vec<SessionStats> = sessions.into_values().map(SessionStats::from).collect();

    if !quiet {
        eprintln!("Found {} sessions with data", result.len());
    }

    save_file_entries_cache(file_entries, cache_path);

    result
}

/// Extract readable project name from path
pub fn format_project_name(path: &str) -> String {
    path.split('-').last().unwrap_or(path).to_string()
}

pub fn load_project_data(
    since: Option<NaiveDate>,
    until: Option<NaiveDate>,
    quiet: bool,
    timezone: &Timezone,
) -> Vec<ProjectStats> {
    let sessions = load_session_data(since, until, quiet, timezone);

    let mut project_map: HashMap<String, ProjectStats> = HashMap::new();

    for session in sessions {
        let project = project_map.entry(session.project_path.clone()).or_insert_with(|| {
            ProjectStats {
                project_path: session.project_path.clone(),
                project_name: format_project_name(&session.project_path),
                session_count: 0,
                stats: Stats::default(),
                models: HashMap::new(),
            }
        });

        project.session_count += 1;
        project.stats.add(&session.stats);

        for (model, model_stats) in session.models {
            project.models.entry(model).or_default().add(&model_stats);
        }
    }

    let mut projects: Vec<ProjectStats> = project_map.into_values().collect();
    projects.sort_by(|a, b| b.stats.total_tokens().cmp(&a.stats.total_tokens()));

    if !quiet {
        eprintln!("Aggregated into {} projects", projects.len());
    }

    projects
}

#[derive(Debug, Clone)]
struct BlockEntry {
    local_dt: DateTime<FixedOffset>,
    parsed: ParsedEntry,
}

impl EntryMeta for BlockEntry {
    fn timestamp_ms(&self) -> i64 {
        self.parsed.timestamp_ms
    }

    fn has_stop_reason(&self) -> bool {
        self.parsed.stop_reason.is_some()
    }
}

#[derive(Default)]
struct BlockAggregate {
    total_with_id: i64,
    message_map: HashMap<String, CandidateState<BlockEntry>>,
    block_map: HashMap<DateTime<FixedOffset>, BlockStats>,
}

impl BlockAggregate {
    fn merge(mut self, other: BlockAggregate) -> BlockAggregate {
        self.total_with_id += other.total_with_id;
        for (id, state) in other.message_map {
            match self.message_map.get_mut(&id) {
                Some(existing) => existing.merge(state),
                None => {
                    self.message_map.insert(id, state);
                }
            }
        }

        for (block_start, stats) in other.block_map {
            let block = self.block_map.entry(block_start).or_default();
            block.stats.add(&stats.stats);
            for (model, model_stats) in stats.models {
                block.models.entry(model).or_default().add(&model_stats);
            }
            if block.block_start.is_empty() {
                block.block_start = stats.block_start;
                block.block_end = stats.block_end;
            }
        }

        self
    }
}

/// Calculate the 5-hour block start time for a given timestamp
fn get_block_start(dt: DateTime<FixedOffset>) -> DateTime<FixedOffset> {
    let hour = dt.hour() as i64;
    let block_hour = (hour / 5) * 5;
    let offset = *dt.offset();
    let naive = dt
        .date_naive()
        .and_hms_opt(block_hour as u32, 0, 0)
        .unwrap_or_else(|| dt.naive_utc());
    offset
        .from_local_datetime(&naive)
        .single()
        .unwrap_or_else(|| offset.from_utc_datetime(&naive))
}

fn add_entry_to_block_map(
    block_map: &mut HashMap<DateTime<FixedOffset>, BlockStats>,
    local_dt: DateTime<FixedOffset>,
    entry: &ParsedEntry,
) {
    let stats = stats_from_usage(&entry.usage);
    let block_start = get_block_start(local_dt);
    let block_end = block_start + Duration::hours(5);

    let block = block_map.entry(block_start).or_insert_with(|| BlockStats {
        block_start: block_start.format("%Y-%m-%d %H:%M").to_string(),
        block_end: block_end.format("%H:%M").to_string(),
        stats: Stats::default(),
        models: HashMap::new(),
    });

    block.stats.add(&stats);
    block
        .models
        .entry(entry.model.clone())
        .or_default()
        .add(&stats);
}

fn process_entries_for_blocks(
    entries: &[RawEntry],
    since: Option<NaiveDate>,
    until: Option<NaiveDate>,
    timezone: &Timezone,
) -> BlockAggregate {
    let mut aggregate = BlockAggregate::default();

    for entry in entries {
        let parsed = match raw_to_parsed(entry, since, until, timezone) {
            Some(p) => p,
            None => continue,
        };

        if let Some(id) = parsed.message_id {
            aggregate.total_with_id += 1;
            let entry = BlockEntry {
                local_dt: parsed.local_dt,
                parsed: parsed.parsed,
            };
            match aggregate.message_map.get_mut(&id) {
                Some(state) => state.update(entry),
                None => {
                    aggregate
                        .message_map
                        .insert(id, CandidateState::new(entry));
                }
            }
        } else if parsed.parsed.stop_reason.is_some() {
            add_entry_to_block_map(&mut aggregate.block_map, parsed.local_dt, &parsed.parsed);
        }
    }

    aggregate
}

pub fn load_block_data(
    since: Option<NaiveDate>,
    until: Option<NaiveDate>,
    quiet: bool,
    timezone: &Timezone,
) -> Vec<BlockStats> {
    if !quiet {
        eprintln!("Scanning JSONL files...");
    }

    let files = find_jsonl_files();

    if !quiet {
        eprintln!("Found {} files", files.len());
        eprintln!("Processing for 5-hour blocks...");
    }

    let (file_entries, cache_path, _cache_hits, _cache_misses) =
        load_file_entries(&files, quiet, false);

    let aggregate = file_entries
        .par_iter()
        .map(|f| process_entries_for_blocks(&f.entries, since, until, timezone))
        .reduce(BlockAggregate::default, |a, b| a.merge(b));

    let mut block_map = aggregate.block_map;

    for (_id, state) in aggregate.message_map {
        let entry = state.finalize();
        add_entry_to_block_map(&mut block_map, entry.local_dt, &entry.parsed);
    }

    let mut blocks: Vec<BlockStats> = block_map.into_values().collect();
    blocks.sort_by(|a, b| a.block_start.cmp(&b.block_start));

    if !quiet {
        eprintln!("Found {} billing blocks", blocks.len());
    }

    save_file_entries_cache(file_entries, cache_path);

    blocks
}
