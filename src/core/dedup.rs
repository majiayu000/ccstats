//! Deduplication logic for streaming entries
//!
//! Streaming responses create multiple entries per message ID.
//! We keep the entry with stop_reason (completed message) to get accurate token counts.

use crate::core::types::RawEntry;

/// Trait for entries that can be deduplicated
pub(crate) trait Deduplicatable {
    fn timestamp_ms(&self) -> i64;
    fn has_stop_reason(&self) -> bool;
    fn message_id(&self) -> Option<&str>;
}

impl Deduplicatable for RawEntry {
    fn timestamp_ms(&self) -> i64 {
        self.timestamp_ms
    }

    fn has_stop_reason(&self) -> bool {
        self.stop_reason.is_some()
    }

    fn message_id(&self) -> Option<&str> {
        self.message_id.as_deref()
    }
}

/// State machine for tracking best candidate entry for a message ID
#[derive(Debug, Clone)]
struct CandidateState<T: Deduplicatable + Clone> {
    /// Entry with stop_reason (preferred)
    completed: Option<T>,
    /// Latest entry by timestamp (fallback)
    latest: T,
}

impl<T: Deduplicatable + Clone> CandidateState<T> {
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

    /// Get the best entry: completed if available, otherwise latest
    fn finalize(self) -> T {
        self.completed.unwrap_or(self.latest)
    }
}

/// Deduplicate entries by message ID
/// Returns (deduplicated entries, skipped count)
pub(crate) fn deduplicate<T, I>(entries: I) -> (Vec<T>, i64)
where
    T: Deduplicatable + Clone,
    I: IntoIterator<Item = T>,
{
    use std::collections::HashMap;

    let mut message_map: HashMap<String, CandidateState<T>> = HashMap::new();
    let mut no_id_entries: Vec<T> = Vec::new();
    let mut total_with_id = 0i64;

    for entry in entries {
        if let Some(id) = entry.message_id() {
            total_with_id += 1;
            match message_map.get_mut(id) {
                Some(state) => state.update(entry),
                None => {
                    message_map.insert(id.to_string(), CandidateState::new(entry));
                }
            }
        } else if entry.has_stop_reason() {
            no_id_entries.push(entry);
        }
    }

    let unique_count = message_map.len() as i64;
    let skipped = (total_with_id - unique_count).max(0);

    let mut result: Vec<T> = message_map.into_values().map(|s| s.finalize()).collect();
    result.extend(no_id_entries);

    (result, skipped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone)]
    struct TestEntry {
        id: Option<String>,
        ts: i64,
        stop: bool,
        value: i32,
    }

    impl Deduplicatable for TestEntry {
        fn timestamp_ms(&self) -> i64 {
            self.ts
        }
        fn has_stop_reason(&self) -> bool {
            self.stop
        }
        fn message_id(&self) -> Option<&str> {
            self.id.as_deref()
        }
    }

    #[test]
    fn test_deduplicate_keeps_completed() {
        let entries = vec![
            TestEntry {
                id: Some("msg1".to_string()),
                ts: 100,
                stop: false,
                value: 1,
            },
            TestEntry {
                id: Some("msg1".to_string()),
                ts: 200,
                stop: true,
                value: 2,
            },
            TestEntry {
                id: Some("msg1".to_string()),
                ts: 300,
                stop: false,
                value: 3,
            },
        ];

        let (result, skipped) = deduplicate(entries);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].value, 2); // Completed entry
        assert_eq!(skipped, 2);
    }

    #[test]
    fn test_deduplicate_fallback_to_latest() {
        let entries = vec![
            TestEntry {
                id: Some("msg1".to_string()),
                ts: 100,
                stop: false,
                value: 1,
            },
            TestEntry {
                id: Some("msg1".to_string()),
                ts: 200,
                stop: false,
                value: 2,
            },
        ];

        let (result, skipped) = deduplicate(entries);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].value, 2); // Latest entry
        assert_eq!(skipped, 1);
    }

    #[test]
    fn test_deduplicate_no_id_with_stop() {
        let entries = vec![
            TestEntry {
                id: None,
                ts: 100,
                stop: true,
                value: 1,
            },
            TestEntry {
                id: None,
                ts: 200,
                stop: false,
                value: 2,
            }, // Ignored
        ];

        let (result, skipped) = deduplicate(entries);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].value, 1);
        assert_eq!(skipped, 0);
    }
}
