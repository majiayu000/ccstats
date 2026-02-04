use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Deserialize)]
pub struct UsageEntry {
    pub timestamp: Option<String>,
    pub message: Option<Message>,
}

#[derive(Debug, Deserialize)]
pub struct Message {
    pub id: Option<String>,
    pub model: Option<String>,
    pub stop_reason: Option<String>,
    pub usage: Option<Usage>,
}

#[derive(Debug, Deserialize, Default, Clone)]
pub struct Usage {
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub cache_creation_input_tokens: Option<i64>,
    pub cache_read_input_tokens: Option<i64>,
}

#[derive(Debug, Default, Clone)]
pub struct Stats {
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_creation: i64,
    pub cache_read: i64,
    pub count: i64,
    pub skipped_chunks: i64,
}

impl Stats {
    pub fn add(&mut self, other: &Stats) {
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
        self.cache_creation += other.cache_creation;
        self.cache_read += other.cache_read;
        self.count += other.count;
        self.skipped_chunks += other.skipped_chunks;
    }

    pub fn total_tokens(&self) -> i64 {
        self.input_tokens + self.output_tokens + self.cache_creation + self.cache_read
    }
}

#[derive(Debug, Default)]
pub struct DayStats {
    pub stats: Stats,
    pub models: HashMap<String, Stats>,
}

/// Intermediate entry for grouping by message ID
#[derive(Debug, Clone)]
pub struct ParsedEntry {
    pub date_str: String,
    pub timestamp: String,
    pub model: String,
    pub usage: Usage,
    pub stop_reason: Option<String>,
}

/// Session statistics
#[derive(Debug, Default)]
pub struct SessionStats {
    pub session_id: String,
    pub project_path: String,
    pub first_timestamp: String,
    pub last_timestamp: String,
    pub stats: Stats,
    pub models: HashMap<String, Stats>,
}

/// Project statistics
#[derive(Debug, Default)]
pub struct ProjectStats {
    pub project_path: String,
    pub project_name: String,
    pub session_count: usize,
    pub stats: Stats,
    pub models: HashMap<String, Stats>,
}

/// 5-hour billing block statistics
#[derive(Debug, Default)]
pub struct BlockStats {
    pub block_start: String,
    pub block_end: String,
    pub stats: Stats,
    pub models: HashMap<String, Stats>,
}
