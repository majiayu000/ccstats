//! Fork-copy reconciliation for OpenCode-family databases.

use std::collections::{HashMap, HashSet};

use rusqlite::Connection;

use crate::core::RawEntry;

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct UsageFingerprint {
    timestamp_ms: i64,
    model: String,
    input: i64,
    output: i64,
    reasoning: i64,
    cache_read: i64,
    cache_write: i64,
}

fn usage_fingerprint(entry: &RawEntry) -> UsageFingerprint {
    UsageFingerprint {
        timestamp_ms: entry.timestamp_ms,
        model: entry.model.clone(),
        input: entry.input_tokens,
        output: entry.output_tokens,
        reasoning: entry.reasoning_tokens,
        cache_read: entry.cache_read,
        cache_write: entry.cache_creation,
    }
}

pub(super) fn read_session_creation_times(
    connection: &Connection,
) -> rusqlite::Result<HashMap<String, i64>> {
    let mut statement = connection.prepare("SELECT id, time_created FROM session")?;
    let rows = statement.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;
    let mut creation_times = HashMap::new();
    for row in rows {
        let (session, created_at) = row?;
        creation_times.insert(session, created_at);
    }
    Ok(creation_times)
}

pub(super) fn reconcile_fork_copies(
    entries: &mut [RawEntry],
    creation_times: &HashMap<String, i64>,
) {
    let mut groups = HashMap::<UsageFingerprint, Vec<usize>>::new();
    for (index, entry) in entries.iter().enumerate() {
        groups
            .entry(usage_fingerprint(entry))
            .or_default()
            .push(index);
    }

    for indices in groups.into_values() {
        let original_sessions = indices
            .iter()
            .filter_map(|index| {
                let entry = &entries[*index];
                creation_times
                    .get(&entry.session_id)
                    .filter(|created_at| entry.timestamp_ms >= **created_at)
                    .map(|_| entry.session_id.clone())
            })
            .collect::<HashSet<_>>();
        let has_copy = indices.iter().any(|index| {
            let entry = &entries[*index];
            creation_times
                .get(&entry.session_id)
                .is_some_and(|created_at| entry.timestamp_ms < *created_at)
        });
        if !has_copy || original_sessions.len() > 1 {
            continue;
        }
        let copy_sessions = indices
            .iter()
            .filter_map(|index| {
                let entry = &entries[*index];
                creation_times
                    .get(&entry.session_id)
                    .filter(|created_at| entry.timestamp_ms < **created_at)
                    .map(|_| entry.session_id.as_str())
            })
            .collect::<HashSet<_>>();
        if original_sessions.is_empty() && copy_sessions.len() < 2 {
            continue;
        }

        let representative = if let Some(original_session) = original_sessions.iter().next() {
            indices
                .iter()
                .copied()
                .filter(|index| entries[*index].session_id == *original_session)
                .max_by(|left, right| {
                    entries[*left]
                        .recorded_cost_usd
                        .unwrap_or(-1.0)
                        .total_cmp(&entries[*right].recorded_cost_usd.unwrap_or(-1.0))
                })
        } else {
            indices.iter().copied().max_by(|left, right| {
                entries[*left]
                    .recorded_cost_usd
                    .unwrap_or(-1.0)
                    .total_cmp(&entries[*right].recorded_cost_usd.unwrap_or(-1.0))
            })
        };
        let Some(representative) = representative else {
            continue;
        };
        let identity = entries[representative].message_id.clone();
        for index in indices {
            if index != representative {
                entries[index].message_id.clone_from(&identity);
                entries[index].stop_reason = None;
                entries[index].recorded_cost_usd = None;
            }
        }
    }
}
