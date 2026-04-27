//! Cursor `SQLite` parser
//!
//! Cursor's schema is not a public API. This parser intentionally only trusts
//! explicit token count fields and skips records that would require estimation.

use std::collections::HashMap;
use std::env;
use std::path::{Path, PathBuf};

use chrono::{DateTime, TimeZone, Utc};
use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;

use crate::consts::{DATE_FORMAT, UNKNOWN};
use crate::core::RawEntry;
use crate::source::ParseOutput;
use crate::utils::Timezone;

const CURSOR_HOME_ENV: &str = "CURSOR_HOME";
const CURSOR_MODEL: &str = "cursor";

#[derive(Debug, Clone, Default)]
struct ComposerMeta {
    model: Option<String>,
    project_path: Option<String>,
}

fn cursor_user_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(path) = env::var(CURSOR_HOME_ENV) {
        return vec![PathBuf::from(path)];
    }

    if let Some(home) = dirs::home_dir() {
        dirs.push(
            home.join("Library")
                .join("Application Support")
                .join("Cursor")
                .join("User"),
        );
        dirs.push(home.join(".config").join("Cursor").join("User"));
    }

    if let Some(data_dir) = dirs::data_dir() {
        dirs.push(data_dir.join("Cursor").join("User"));
    }

    dirs.sort();
    dirs.dedup();
    dirs
}

pub(super) fn find_cursor_files() -> Vec<PathBuf> {
    let mut files = Vec::new();
    for user_dir in cursor_user_dirs() {
        let global = user_dir.join("globalStorage").join("state.vscdb");
        if global.is_file() {
            files.push(global);
        }

        let workspace_glob = user_dir
            .join("workspaceStorage")
            .join("*")
            .join("state.vscdb");
        if let Ok(entries) = glob::glob(&workspace_glob.display().to_string()) {
            files.extend(entries.flatten().filter(|path| path.is_file()));
        }
    }
    files.sort();
    files.dedup();
    files
}

fn open_readonly(path: &Path) -> rusqlite::Result<Connection> {
    Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
}

fn table_exists(conn: &Connection, table: &str) -> bool {
    conn.query_row(
        "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1 LIMIT 1",
        [table],
        |_| Ok(()),
    )
    .is_ok()
}

fn value_from_blob(blob: &[u8]) -> Option<Value> {
    serde_json::from_slice(blob).ok().or_else(|| {
        std::str::from_utf8(blob)
            .ok()
            .and_then(|text| serde_json::from_str(text).ok())
    })
}

fn row_value_bytes(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<Vec<u8>> {
    match row.get_ref(index)? {
        ValueRef::Blob(bytes) | ValueRef::Text(bytes) => Ok(bytes.to_vec()),
        _ => Ok(Vec::new()),
    }
}

fn string_at<'a>(value: &'a Value, path: &[&str]) -> Option<&'a str> {
    let mut current = value;
    for part in path {
        current = current.get(*part)?;
    }
    current.as_str()
}

fn integer_at(value: &Value, path: &[&str]) -> Option<i64> {
    let mut current = value;
    for part in path {
        current = current.get(*part)?;
    }
    current
        .as_i64()
        .or_else(|| current.as_u64().and_then(|n| i64::try_from(n).ok()))
}

fn first_string(value: &Value, paths: &[&[&str]]) -> Option<String> {
    paths
        .iter()
        .find_map(|path| string_at(value, path))
        .filter(|s| !s.trim().is_empty())
        .map(std::string::ToString::to_string)
}

fn timestamp_from_value(value: &Value) -> Option<DateTime<Utc>> {
    if let Some(timestamp) = string_at(value, &["createdAt"])
        .or_else(|| string_at(value, &["timestamp"]))
        .or_else(|| string_at(value, &["time"]))
    {
        return timestamp.parse::<DateTime<Utc>>().ok();
    }

    let millis = integer_at(value, &["createdAt"])
        .or_else(|| integer_at(value, &["unixMs"]))
        .or_else(|| integer_at(value, &["timestampMs"]))?;
    Utc.timestamp_millis_opt(millis).single()
}

fn token_counts(value: &Value) -> Option<(i64, i64)> {
    let input = integer_at(value, &["tokenCount", "inputTokens"])
        .or_else(|| integer_at(value, &["tokenCount", "input_tokens"]))
        .or_else(|| integer_at(value, &["usage", "inputTokens"]))
        .or_else(|| integer_at(value, &["inputTokens"]))
        .unwrap_or(0);
    let output = integer_at(value, &["tokenCount", "outputTokens"])
        .or_else(|| integer_at(value, &["tokenCount", "output_tokens"]))
        .or_else(|| integer_at(value, &["usage", "outputTokens"]))
        .or_else(|| integer_at(value, &["outputTokens"]))
        .unwrap_or(0);

    (input > 0 || output > 0).then_some((input, output))
}

fn composer_id_from_key(key: &str) -> Option<&str> {
    key.strip_prefix("composerData:")
}

fn bubble_ids_from_key(key: &str) -> Option<(&str, &str)> {
    let rest = key.strip_prefix("bubbleId:")?;
    rest.split_once(':')
}

fn composer_meta_from_value(value: &Value) -> ComposerMeta {
    ComposerMeta {
        model: first_string(
            value,
            &[
                &["modelConfig", "modelName"],
                &["modelInfo", "modelName"],
                &["model"],
            ],
        ),
        project_path: first_string(
            value,
            &[
                &["workspaceIdentifier", "uri", "fsPath"],
                &["workspaceIdentifier", "uri", "path"],
                &["workspaceIdentifier", "path"],
            ],
        ),
    }
}

fn entry_from_bubble(
    key: &str,
    value: &Value,
    composers: &HashMap<String, ComposerMeta>,
    timezone: Timezone,
) -> Option<RawEntry> {
    let (composer_id, bubble_id) = bubble_ids_from_key(key)?;
    let (input_tokens, output_tokens) = token_counts(value)?;
    let utc_dt = timestamp_from_value(value)?;
    let local_dt = timezone.to_fixed_offset(utc_dt);
    let meta = composers.get(composer_id);
    let model = first_string(
        value,
        &[
            &["modelInfo", "modelName"],
            &["modelConfig", "modelName"],
            &["model"],
        ],
    )
    .or_else(|| meta.and_then(|m| m.model.clone()))
    .unwrap_or_else(|| CURSOR_MODEL.to_string());

    Some(RawEntry {
        timestamp: utc_dt.to_rfc3339(),
        timestamp_ms: utc_dt.timestamp_millis(),
        date_str: local_dt.date_naive().format(DATE_FORMAT).to_string(),
        message_id: Some(bubble_id.to_string()),
        session_id: composer_id.to_string(),
        project_path: meta
            .and_then(|m| m.project_path.clone())
            .unwrap_or_default(),
        model,
        input_tokens,
        output_tokens,
        cache_creation: 0,
        cache_read: 0,
        reasoning_tokens: 0,
        stop_reason: Some("complete".to_string()),
    })
}

fn entry_from_generation(generation: &Value, path: &Path, timezone: Timezone) -> Option<RawEntry> {
    let (input_tokens, output_tokens) = token_counts(generation)?;
    let utc_dt = timestamp_from_value(generation)?;
    let local_dt = timezone.to_fixed_offset(utc_dt);
    let session_id = first_string(
        generation,
        &[
            &["generationUUID"],
            &["generationId"],
            &["id"],
            &["sessionId"],
        ],
    )
    .unwrap_or_else(|| {
        path.file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or(UNKNOWN)
            .to_string()
    });
    let model = first_string(
        generation,
        &[
            &["model"],
            &["modelName"],
            &["modelInfo", "modelName"],
            &["modelConfig", "modelName"],
        ],
    )
    .unwrap_or_else(|| CURSOR_MODEL.to_string());

    Some(RawEntry {
        timestamp: utc_dt.to_rfc3339(),
        timestamp_ms: utc_dt.timestamp_millis(),
        date_str: local_dt.date_naive().format(DATE_FORMAT).to_string(),
        message_id: Some(session_id.clone()),
        session_id,
        project_path: String::new(),
        model,
        input_tokens,
        output_tokens,
        cache_creation: 0,
        cache_read: 0,
        reasoning_tokens: 0,
        stop_reason: Some("complete".to_string()),
    })
}

fn parse_cursor_disk_kv(
    conn: &Connection,
    timezone: Timezone,
) -> rusqlite::Result<(Vec<RawEntry>, usize)> {
    let mut entries = Vec::new();
    let mut errors = 0usize;
    let mut composers = HashMap::new();

    let mut stmt = conn.prepare(
        "SELECT key, value FROM cursorDiskKV \
         WHERE key LIKE 'composerData:%' OR key LIKE 'bubbleId:%'",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row_value_bytes(row, 1)?))
    })?;

    let mut bubbles = Vec::new();
    for row in rows {
        let (key, blob) = row?;
        let Some(value) = value_from_blob(&blob) else {
            errors += 1;
            continue;
        };
        if let Some(composer_id) = composer_id_from_key(&key) {
            composers.insert(composer_id.to_string(), composer_meta_from_value(&value));
        } else {
            bubbles.push((key, value));
        }
    }

    for (key, value) in bubbles {
        if let Some(entry) = entry_from_bubble(&key, &value, &composers, timezone) {
            entries.push(entry);
        }
    }

    Ok((entries, errors))
}

fn parse_item_table(
    conn: &Connection,
    path: &Path,
    timezone: Timezone,
) -> rusqlite::Result<(Vec<RawEntry>, usize)> {
    let mut entries = Vec::new();
    let mut errors = 0usize;
    let mut stmt = conn.prepare(
        "SELECT value FROM ItemTable \
         WHERE key IN ('aiService.generations', 'workbench.panel.aichat.view.aichat.chatdata')",
    )?;
    let rows = stmt.query_map([], |row| row_value_bytes(row, 0))?;

    for row in rows {
        let blob = row?;
        let Some(value) = value_from_blob(&blob) else {
            errors += 1;
            continue;
        };
        if let Some(generations) = value.as_array() {
            for generation in generations {
                if let Some(entry) = entry_from_generation(generation, path, timezone) {
                    entries.push(entry);
                }
            }
        }
    }

    Ok((entries, errors))
}

pub(super) fn parse_cursor_db_with_debug(
    path: &Path,
    timezone: Timezone,
    debug: bool,
) -> ParseOutput {
    let conn = match open_readonly(path) {
        Ok(conn) => conn,
        Err(err) => {
            if debug {
                eprintln!("Failed to open Cursor database {}: {}", path.display(), err);
            }
            return ParseOutput {
                entries: Vec::new(),
                errors: 1,
            };
        }
    };

    let mut entries = Vec::new();
    let mut errors = 0usize;

    if table_exists(&conn, "cursorDiskKV") {
        match parse_cursor_disk_kv(&conn, timezone) {
            Ok((mut parsed, parse_errors)) => {
                entries.append(&mut parsed);
                errors += parse_errors;
            }
            Err(err) => {
                if debug {
                    eprintln!(
                        "Failed to parse cursorDiskKV in {}: {}",
                        path.display(),
                        err
                    );
                }
                errors += 1;
            }
        }
    }

    if table_exists(&conn, "ItemTable") {
        match parse_item_table(&conn, path, timezone) {
            Ok((mut parsed, parse_errors)) => {
                entries.append(&mut parsed);
                errors += parse_errors;
            }
            Err(err) => {
                if debug {
                    eprintln!("Failed to parse ItemTable in {}: {}", path.display(), err);
                }
                errors += 1;
            }
        }
    }

    ParseOutput { entries, errors }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tz() -> Timezone {
        Timezone::parse(Some("UTC")).unwrap()
    }

    #[test]
    fn token_counts_skips_zero_records() {
        let value = json!({"tokenCount": {"inputTokens": 0, "outputTokens": 0}});
        assert!(token_counts(&value).is_none());
    }

    #[test]
    fn entry_from_bubble_reads_token_counts() {
        let value = json!({
            "createdAt": "2026-02-06T10:00:00Z",
            "tokenCount": {"inputTokens": 100, "outputTokens": 40},
            "modelInfo": {"modelName": "claude-4-sonnet"}
        });
        let entry = entry_from_bubble(
            "bubbleId:composer-1:bubble-1",
            &value,
            &HashMap::new(),
            tz(),
        )
        .unwrap();

        assert_eq!(entry.session_id, "composer-1");
        assert_eq!(entry.message_id.as_deref(), Some("bubble-1"));
        assert_eq!(entry.date_str, "2026-02-06");
        assert_eq!(entry.model, "claude-4-sonnet");
        assert_eq!(entry.input_tokens, 100);
        assert_eq!(entry.output_tokens, 40);
    }

    #[test]
    fn entry_from_bubble_uses_composer_model_fallback() {
        let mut composers = HashMap::new();
        composers.insert(
            "composer-1".to_string(),
            ComposerMeta {
                model: Some("gpt-5".to_string()),
                project_path: Some("/tmp/project".to_string()),
            },
        );
        let value = json!({
            "createdAt": 1_770_372_000_000_i64,
            "tokenCount": {"inputTokens": 10, "outputTokens": 5}
        });

        let entry =
            entry_from_bubble("bubbleId:composer-1:bubble-1", &value, &composers, tz()).unwrap();

        assert_eq!(entry.model, "gpt-5");
        assert_eq!(entry.project_path, "/tmp/project");
        assert_eq!(entry.timestamp, "2026-02-06T10:00:00+00:00");
    }
}
