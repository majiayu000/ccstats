use chrono::{DateTime, Duration, Local, NaiveDate, Timelike, Utc};
use rayon::prelude::*;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;

use super::types::{BlockStats, DayStats, ParsedEntry, ProjectStats, SessionStats, Stats, UsageEntry};

#[derive(Debug)]
struct RawEntry {
    message_id: Option<String>,
    parsed: ParsedEntry,
}

fn collect_entries_from_file(
    path: &PathBuf,
    since: Option<NaiveDate>,
    until: Option<NaiveDate>,
) -> (Vec<RawEntry>, i64) {
    let file = match File::open(path) {
        Ok(f) => f,
        Err(_) => return (Vec::new(), 0),
    };
    let reader = BufReader::new(file);

    let mut total_entries = 0i64;
    let mut entries: Vec<RawEntry> = Vec::new();

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => continue,
        };

        if line.trim().is_empty() {
            continue;
        }

        let entry: UsageEntry = match serde_json::from_str(&line) {
            Ok(e) => e,
            Err(_) => continue,
        };

        // Get timestamp and convert to local date
        let ts = match &entry.timestamp {
            Some(t) => t,
            None => continue,
        };

        let utc_dt = match ts.parse::<DateTime<Utc>>() {
            Ok(dt) => dt,
            Err(_) => continue,
        };

        let local_dt: DateTime<Local> = utc_dt.into();
        let date = local_dt.date_naive();

        // Apply date filters
        if let Some(s) = since {
            if date < s {
                continue;
            }
        }
        if let Some(u) = until {
            if date > u {
                continue;
            }
        }

        total_entries += 1;

        let msg = match &entry.message {
            Some(m) => m,
            None => continue,
        };

        let usage = match &msg.usage {
            Some(u) => u.clone(),
            None => continue,
        };

        let model = msg
            .model
            .as_ref()
            .map(|m| normalize_model_name(m))
            .unwrap_or_else(|| "unknown".to_string());

        // Skip synthetic/empty entries
        if model == "<synthetic>" || model.is_empty() {
            continue;
        }

        let date_str = date.format("%Y-%m-%d").to_string();
        let stop_reason = msg.stop_reason.clone();
        let parsed = ParsedEntry {
            date_str,
            timestamp: ts.clone(),
            model,
            usage,
            stop_reason,
        };

        entries.push(RawEntry {
            message_id: msg.id.clone(),
            parsed,
        });
    }

    (entries, total_entries)
}

/// Deduplicate entries by message ID.
/// For entries with the same message ID, prefer the one with stop_reason (earliest).
/// If none has stop_reason, use the latest by timestamp.
/// Entries without message ID are only kept if they have stop_reason.
pub fn deduplicate_by_message_id(
    message_entries: HashMap<String, Vec<ParsedEntry>>,
    no_id_entries: Vec<ParsedEntry>,
) -> Vec<ParsedEntry> {
    let mut result = Vec::new();

    for (_id, entries) in message_entries {
        let completed = entries
            .iter()
            .filter(|e| e.stop_reason.is_some())
            .min_by(|a, b| a.timestamp.cmp(&b.timestamp));
        let last = entries.iter().max_by(|a, b| a.timestamp.cmp(&b.timestamp));
        if let Some(entry) = completed.or(last) {
            result.push(entry.clone());
        }
    }

    for entry in no_id_entries {
        if entry.stop_reason.is_some() {
            result.push(entry);
        }
    }

    result
}

fn split_raw_entries(raw_entries: Vec<RawEntry>) -> (HashMap<String, Vec<ParsedEntry>>, Vec<ParsedEntry>) {
    let mut message_entries: HashMap<String, Vec<ParsedEntry>> = HashMap::new();
    let mut no_id_entries: Vec<ParsedEntry> = Vec::new();

    for raw in raw_entries {
        if let Some(id) = raw.message_id {
            message_entries.entry(id).or_default().push(raw.parsed);
        } else {
            no_id_entries.push(raw.parsed);
        }
    }

    (message_entries, no_id_entries)
}

pub fn normalize_model_name(model: &str) -> String {
    // Remove "claude-" prefix
    let name = model.replace("claude-", "");

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
    let home = dirs::home_dir().expect("Cannot find home directory");
    let claude_path = home.join(".claude").join("projects");

    let mut files = Vec::new();
    if let Ok(entries) = glob::glob(&format!("{}/**/*.jsonl", claude_path.display())) {
        for entry in entries.flatten() {
            files.push(entry);
        }
    }
    files
}

pub fn load_usage_data_with_debug(
    since: Option<NaiveDate>,
    until: Option<NaiveDate>,
    debug: bool,
) -> (HashMap<String, DayStats>, i64, i64) {
    load_usage_data_internal(since, until, debug, false)
}

pub fn load_usage_data_quiet(
    since: Option<NaiveDate>,
    until: Option<NaiveDate>,
) -> (HashMap<String, DayStats>, i64, i64) {
    load_usage_data_internal(since, until, false, true)
}

fn load_usage_data_internal(
    since: Option<NaiveDate>,
    until: Option<NaiveDate>,
    debug: bool,
    quiet: bool,
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

    let results: Vec<_> = files
        .par_iter()
        .map(|f| collect_entries_from_file(f, since, until))
        .collect();

    let mut total_entries = 0i64;
    let mut all_raw_entries = Vec::new();

    for (entries, file_total) in results {
        total_entries += file_total;
        all_raw_entries.extend(entries);
    }

    let (message_entries, no_id_entries) = split_raw_entries(all_raw_entries);
    let deduped = deduplicate_by_message_id(message_entries, no_id_entries);

    let mut day_stats: HashMap<String, DayStats> = HashMap::new();

    for entry in &deduped {
        let stats = Stats {
            input_tokens: entry.usage.input_tokens.unwrap_or(0),
            output_tokens: entry.usage.output_tokens.unwrap_or(0),
            cache_creation: entry.usage.cache_creation_input_tokens.unwrap_or(0),
            cache_read: entry.usage.cache_read_input_tokens.unwrap_or(0),
            count: 1,
            skipped_chunks: 0,
        };

        let day = day_stats.entry(entry.date_str.clone()).or_default();
        day.stats.add(&stats);
        day.models.entry(entry.model.clone()).or_default().add(&stats);
    }

    let valid = deduped.len() as i64;
    let skipped = total_entries - valid;
    let merged = day_stats;

    if debug && !quiet {
        eprintln!("[DEBUG] Processing complete:");
        eprintln!("[DEBUG]   - Total unique API calls: {}", valid);
        eprintln!("[DEBUG]   - Streaming entries deduplicated: {}", skipped);
        eprintln!("[DEBUG]   - Days with data: {}", merged.len());

        // Show model breakdown
        let mut model_counts: HashMap<String, i64> = HashMap::new();
        for (_date, day_stats) in &merged {
            for (model, stats) in &day_stats.models {
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

    (merged, skipped, valid)
}

/// Process a single file and return session stats
fn process_file_for_session(
    path: &PathBuf,
    since: Option<NaiveDate>,
    until: Option<NaiveDate>,
) -> Option<SessionStats> {
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

    let (raw_entries, _) = collect_entries_from_file(path, since, until);
    if raw_entries.is_empty() {
        return None;
    }

    let first_ts = raw_entries.iter().map(|e| &e.parsed.timestamp).min().cloned().unwrap_or_default();
    let last_ts = raw_entries.iter().map(|e| &e.parsed.timestamp).max().cloned().unwrap_or_default();

    let (message_entries, no_id_entries) = split_raw_entries(raw_entries);
    let deduped = deduplicate_by_message_id(message_entries, no_id_entries);

    if deduped.is_empty() {
        return None;
    }

    let mut session = SessionStats {
        session_id,
        project_path,
        first_timestamp: first_ts,
        last_timestamp: last_ts,
        stats: Stats::default(),
        models: HashMap::new(),
    };

    for entry in &deduped {
        let stats = Stats {
            input_tokens: entry.usage.input_tokens.unwrap_or(0),
            output_tokens: entry.usage.output_tokens.unwrap_or(0),
            cache_creation: entry.usage.cache_creation_input_tokens.unwrap_or(0),
            cache_read: entry.usage.cache_read_input_tokens.unwrap_or(0),
            count: 1,
            skipped_chunks: 0,
        };

        session.stats.add(&stats);
        session.models.entry(entry.model.clone()).or_default().add(&stats);
    }

    Some(session)
}

pub fn load_session_data(
    since: Option<NaiveDate>,
    until: Option<NaiveDate>,
    quiet: bool,
) -> Vec<SessionStats> {
    if !quiet {
        eprintln!("Scanning JSONL files...");
    }

    let files = find_jsonl_files();

    if !quiet {
        eprintln!("Found {} files", files.len());
        eprintln!("Processing sessions...");
    }

    let sessions: Vec<SessionStats> = files
        .par_iter()
        .filter_map(|f| process_file_for_session(f, since, until))
        .collect();

    if !quiet {
        eprintln!("Found {} sessions with data", sessions.len());
    }

    sessions
}

/// Extract readable project name from path
pub fn format_project_name(path: &str) -> String {
    path.split('-').last().unwrap_or(path).to_string()
}

pub fn load_project_data(
    since: Option<NaiveDate>,
    until: Option<NaiveDate>,
    quiet: bool,
) -> Vec<ProjectStats> {
    // First load all sessions
    let sessions = load_session_data(since, until, quiet);

    // Aggregate by project
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

/// Calculate the 5-hour block start time for a given timestamp
fn get_block_start(dt: DateTime<Local>) -> DateTime<Local> {
    let hour = dt.hour() as i64;
    let block_hour = (hour / 5) * 5;
    dt.date_naive()
        .and_hms_opt(block_hour as u32, 0, 0)
        .unwrap()
        .and_local_timezone(Local)
        .unwrap()
}

/// Process a single file and return entries with timestamps for block grouping
fn process_file_for_blocks(
    path: &PathBuf,
    since: Option<NaiveDate>,
    until: Option<NaiveDate>,
) -> Vec<(DateTime<Local>, String, Stats)> {
    let (raw_entries, _) = collect_entries_from_file(path, since, until);
    let (message_entries, no_id_entries) = split_raw_entries(raw_entries);
    let deduped = deduplicate_by_message_id(message_entries, no_id_entries);

    deduped
        .into_iter()
        .filter_map(|entry| {
            let utc_dt = entry.timestamp.parse::<DateTime<Utc>>().ok()?;
            let local_dt: DateTime<Local> = utc_dt.into();
            let stats = Stats {
                input_tokens: entry.usage.input_tokens.unwrap_or(0),
                output_tokens: entry.usage.output_tokens.unwrap_or(0),
                cache_creation: entry.usage.cache_creation_input_tokens.unwrap_or(0),
                cache_read: entry.usage.cache_read_input_tokens.unwrap_or(0),
                count: 1,
                skipped_chunks: 0,
            };
            Some((local_dt, entry.model, stats))
        })
        .collect()
}

pub fn load_block_data(
    since: Option<NaiveDate>,
    until: Option<NaiveDate>,
    quiet: bool,
) -> Vec<BlockStats> {
    if !quiet {
        eprintln!("Scanning JSONL files...");
    }

    let files = find_jsonl_files();

    if !quiet {
        eprintln!("Found {} files", files.len());
        eprintln!("Processing for 5-hour blocks...");
    }

    // Collect all entries with timestamps
    let all_entries: Vec<(DateTime<Local>, String, Stats)> = files
        .par_iter()
        .flat_map(|f| process_file_for_blocks(f, since, until))
        .collect();

    // Group by 5-hour blocks
    let mut block_map: HashMap<DateTime<Local>, BlockStats> = HashMap::new();

    for (dt, model, stats) in all_entries {
        let block_start = get_block_start(dt);
        let block_end = block_start + Duration::hours(5);

        let block = block_map.entry(block_start).or_insert_with(|| BlockStats {
            block_start: block_start.format("%Y-%m-%d %H:%M").to_string(),
            block_end: block_end.format("%H:%M").to_string(),
            stats: Stats::default(),
            models: HashMap::new(),
        });

        block.stats.add(&stats);
        block.models.entry(model).or_default().add(&stats);
    }

    let mut blocks: Vec<BlockStats> = block_map.into_values().collect();
    blocks.sort_by(|a, b| a.block_start.cmp(&b.block_start));

    if !quiet {
        eprintln!("Found {} billing blocks", blocks.len());
    }

    blocks
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::types::{ParsedEntry, Usage};

    #[test]
    fn normalize_model_name_removes_claude_prefix() {
        assert_eq!(normalize_model_name("claude-sonnet-4-20250514"), "sonnet-4");
        assert_eq!(normalize_model_name("claude-opus-4-5-20251101"), "opus-4-5");
    }

    #[test]
    fn normalize_model_name_removes_date_suffix() {
        assert_eq!(normalize_model_name("sonnet-4-20250514"), "sonnet-4");
    }

    #[test]
    fn normalize_model_name_preserves_name_without_date() {
        assert_eq!(normalize_model_name("sonnet-4"), "sonnet-4");
        assert_eq!(normalize_model_name("unknown"), "unknown");
    }

    #[test]
    fn normalize_model_name_short_suffix_not_stripped() {
        assert_eq!(normalize_model_name("model-123"), "model-123");
    }

    fn make_entry(id: Option<&str>, model: &str, stop_reason: Option<&str>, ts: &str, input: i64) -> (Option<String>, ParsedEntry) {
        (
            id.map(|s| s.to_string()),
            ParsedEntry {
                date_str: "2025-01-01".to_string(),
                timestamp: ts.to_string(),
                model: model.to_string(),
                usage: Usage {
                    input_tokens: Some(input),
                    output_tokens: Some(0),
                    cache_creation_input_tokens: None,
                    cache_read_input_tokens: None,
                },
                stop_reason: stop_reason.map(|s| s.to_string()),
            },
        )
    }

    #[test]
    fn dedup_prefers_stop_reason_entry() {
        let mut message_entries: HashMap<String, Vec<ParsedEntry>> = HashMap::new();
        let (_, e1) = make_entry(Some("msg1"), "sonnet", None, "2025-01-01T00:00:00Z", 100);
        let (_, e2) = make_entry(Some("msg1"), "sonnet", Some("end_turn"), "2025-01-01T00:00:01Z", 200);
        let (_, e3) = make_entry(Some("msg1"), "sonnet", None, "2025-01-01T00:00:02Z", 300);
        message_entries.insert("msg1".to_string(), vec![e1, e2, e3]);

        let result = deduplicate_by_message_id(message_entries, vec![]);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].usage.input_tokens, Some(200));
    }

    #[test]
    fn dedup_falls_back_to_latest_without_stop_reason() {
        let mut message_entries: HashMap<String, Vec<ParsedEntry>> = HashMap::new();
        let (_, e1) = make_entry(Some("msg1"), "sonnet", None, "2025-01-01T00:00:00Z", 100);
        let (_, e2) = make_entry(Some("msg1"), "sonnet", None, "2025-01-01T00:00:02Z", 300);
        message_entries.insert("msg1".to_string(), vec![e1, e2]);

        let result = deduplicate_by_message_id(message_entries, vec![]);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].usage.input_tokens, Some(300));
    }

    #[test]
    fn dedup_no_id_entries_only_keeps_completed() {
        let (_, e1) = make_entry(None, "sonnet", None, "2025-01-01T00:00:00Z", 100);
        let (_, e2) = make_entry(None, "sonnet", Some("end_turn"), "2025-01-01T00:00:01Z", 200);

        let result = deduplicate_by_message_id(HashMap::new(), vec![e1, e2]);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].usage.input_tokens, Some(200));
    }

    #[test]
    fn dedup_empty_input() {
        let result = deduplicate_by_message_id(HashMap::new(), vec![]);
        assert!(result.is_empty());
    }

    #[test]
    fn format_project_name_extracts_last_segment() {
        assert_eq!(format_project_name("-Users-apple-Desktop-code-ccstats"), "ccstats");
        assert_eq!(format_project_name("simple"), "simple");
    }
}
