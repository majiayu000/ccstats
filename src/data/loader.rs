use chrono::{DateTime, Local, NaiveDate, Utc};
use rayon::prelude::*;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;

use super::types::{DayStats, ParsedEntry, SessionStats, Stats, Usage, UsageEntry};

pub fn normalize_model_name(model: &str) -> String {
    model
        .replace("claude-", "")
        .replace("-20250514", "")
        .replace("-20241022", "")
        .replace("-20240620", "")
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

pub fn process_file(
    path: &PathBuf,
    since: Option<NaiveDate>,
    until: Option<NaiveDate>,
) -> (HashMap<String, DayStats>, i64, i64) {
    let mut day_stats: HashMap<String, DayStats> = HashMap::new();
    let mut total_entries = 0i64;
    let mut valid_messages = 0i64;

    // First pass: collect all entries grouped by message ID
    let mut message_entries: HashMap<String, Vec<ParsedEntry>> = HashMap::new();
    let mut no_id_entries: Vec<ParsedEntry> = Vec::new();

    let file = match File::open(path) {
        Ok(f) => f,
        Err(_) => return (day_stats, 0, 0),
    };

    let reader = BufReader::new(file);

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
        let parsed = ParsedEntry {
            date_str,
            model,
            usage,
        };

        // Group by message ID - we'll take the last entry for each ID
        if let Some(id) = &msg.id {
            message_entries.entry(id.clone()).or_default().push(parsed);
        } else {
            no_id_entries.push(parsed);
        }
    }

    // Second pass: for each message ID, take the LAST entry (final state)
    for (_id, entries) in message_entries {
        if let Some(last) = entries.last() {
            valid_messages += 1;
            let stats = Stats {
                input_tokens: last.usage.input_tokens.unwrap_or(0),
                output_tokens: last.usage.output_tokens.unwrap_or(0),
                cache_creation: last.usage.cache_creation_input_tokens.unwrap_or(0),
                cache_read: last.usage.cache_read_input_tokens.unwrap_or(0),
                count: 1,
                skipped_chunks: 0,
            };

            let day = day_stats.entry(last.date_str.clone()).or_default();
            day.stats.add(&stats);
            day.models.entry(last.model.clone()).or_default().add(&stats);
        }
    }

    // Also process entries without message ID (shouldn't happen often)
    for entry in no_id_entries {
        valid_messages += 1;
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

    let skipped = total_entries - valid_messages;
    (day_stats, skipped, valid_messages)
}

pub fn merge_results(
    results: Vec<(HashMap<String, DayStats>, i64, i64)>,
) -> (HashMap<String, DayStats>, i64, i64) {
    let mut merged: HashMap<String, DayStats> = HashMap::new();
    let mut total_skipped = 0i64;
    let mut total_valid = 0i64;

    for (day_stats, skipped, valid) in results {
        total_skipped += skipped;
        total_valid += valid;

        for (date, stats) in day_stats {
            let day = merged.entry(date).or_default();

            for (model, model_stats) in stats.models {
                day.stats.add(&model_stats);
                day.models.entry(model).or_default().add(&model_stats);
            }
        }
    }

    (merged, total_skipped, total_valid)
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
        .map(|f| process_file(f, since, until))
        .collect();

    let (merged, skipped, valid) = merge_results(results);

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
    // Extract session ID from filename
    let session_id = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();

    // Extract project path from parent directory
    let project_path = path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();

    let file = File::open(path).ok()?;
    let reader = BufReader::new(file);

    let mut message_entries: HashMap<String, (String, String, Usage)> = HashMap::new();
    let mut first_ts: Option<String> = None;
    let mut last_ts: Option<String> = None;
    let mut has_data = false;

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

        // Track timestamps
        if first_ts.is_none() {
            first_ts = Some(ts.clone());
        }
        last_ts = Some(ts.clone());

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

        if model == "<synthetic>" || model.is_empty() {
            continue;
        }

        has_data = true;

        // Group by message ID, keep last entry
        if let Some(id) = &msg.id {
            message_entries.insert(id.clone(), (ts.clone(), model, usage));
        }
    }

    if !has_data {
        return None;
    }

    let mut session = SessionStats {
        session_id,
        project_path,
        first_timestamp: first_ts.unwrap_or_default(),
        last_timestamp: last_ts.unwrap_or_default(),
        stats: Stats::default(),
        models: HashMap::new(),
    };

    for (_id, (_ts, model, usage)) in message_entries {
        let stats = Stats {
            input_tokens: usage.input_tokens.unwrap_or(0),
            output_tokens: usage.output_tokens.unwrap_or(0),
            cache_creation: usage.cache_creation_input_tokens.unwrap_or(0),
            cache_read: usage.cache_read_input_tokens.unwrap_or(0),
            count: 1,
            skipped_chunks: 0,
        };

        session.stats.add(&stats);
        session.models.entry(model).or_default().add(&stats);
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
