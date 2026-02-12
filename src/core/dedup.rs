//! Deduplication logic for streaming entries
//!
//! Streaming responses create multiple entries per message ID.
//! We keep the entry with stop_reason (completed message) to get accurate token counts.

use crate::core::types::RawEntry;
use std::collections::HashMap;

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

    fn merge(&mut self, other: CandidateState<T>) {
        let CandidateState { completed, latest } = other;
        if let Some(entry) = completed {
            self.update(entry);
        }
        self.update(latest);
    }

    /// Get the best entry: completed if available, otherwise latest
    fn finalize(self) -> T {
        self.completed.unwrap_or(self.latest)
    }
}

/// Incremental dedup accumulator for chunked/parallel loading.
#[derive(Debug, Clone)]
pub(crate) struct DedupAccumulator<T: Deduplicatable + Clone> {
    message_map: HashMap<String, CandidateState<T>>,
    no_id_entries: Vec<T>,
    total_with_id: i64,
}

impl<T: Deduplicatable + Clone> Default for DedupAccumulator<T> {
    fn default() -> Self {
        Self {
            message_map: HashMap::new(),
            no_id_entries: Vec::new(),
            total_with_id: 0,
        }
    }
}

impl<T: Deduplicatable + Clone> DedupAccumulator<T> {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn push(&mut self, entry: T) {
        if let Some(id) = entry.message_id() {
            self.total_with_id += 1;
            match self.message_map.get_mut(id) {
                Some(state) => state.update(entry),
                None => {
                    self.message_map
                        .insert(id.to_string(), CandidateState::new(entry));
                }
            }
        } else if entry.has_stop_reason() {
            self.no_id_entries.push(entry);
        }
    }

    pub(crate) fn extend<I>(&mut self, entries: I)
    where
        I: IntoIterator<Item = T>,
    {
        for entry in entries {
            self.push(entry);
        }
    }

    pub(crate) fn merge(&mut self, other: DedupAccumulator<T>) {
        self.total_with_id += other.total_with_id;
        self.no_id_entries.extend(other.no_id_entries);

        for (id, state) in other.message_map {
            match self.message_map.get_mut(&id) {
                Some(existing) => existing.merge(state),
                None => {
                    self.message_map.insert(id, state);
                }
            }
        }
    }

    pub(crate) fn finalize(self) -> (Vec<T>, i64) {
        let unique_count = self.message_map.len() as i64;
        let skipped = (self.total_with_id - unique_count).max(0);

        let mut result: Vec<T> = self
            .message_map
            .into_values()
            .map(|s| s.finalize())
            .collect();
        result.extend(self.no_id_entries);

        (result, skipped)
    }
}

/// Deduplicate entries by message ID
/// Returns (deduplicated entries, skipped count)
#[cfg(test)]
pub(crate) fn deduplicate<T, I>(entries: I) -> (Vec<T>, i64)
where
    T: Deduplicatable + Clone,
    I: IntoIterator<Item = T>,
{
    let mut accumulator = DedupAccumulator::new();
    accumulator.extend(entries);
    accumulator.finalize()
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

    #[test]
    fn test_deduplicate_empty_input() {
        let entries: Vec<TestEntry> = vec![];
        let (result, skipped) = deduplicate(entries);
        assert_eq!(result.len(), 0);
        assert_eq!(skipped, 0);
    }

    #[test]
    fn test_deduplicate_single_entry() {
        let entries = vec![TestEntry {
            id: Some("msg1".to_string()),
            ts: 100,
            stop: true,
            value: 1,
        }];
        let (result, skipped) = deduplicate(entries);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].value, 1);
        assert_eq!(skipped, 0);
    }

    #[test]
    fn test_deduplicate_all_duplicates_all_completed() {
        // Multiple entries for same ID, all with stop_reason — keep latest completed
        let entries = vec![
            TestEntry {
                id: Some("msg1".to_string()),
                ts: 100,
                stop: true,
                value: 1,
            },
            TestEntry {
                id: Some("msg1".to_string()),
                ts: 300,
                stop: true,
                value: 3,
            },
            TestEntry {
                id: Some("msg1".to_string()),
                ts: 200,
                stop: true,
                value: 2,
            },
        ];
        let (result, skipped) = deduplicate(entries);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].value, 3); // Latest completed (ts=300)
        assert_eq!(skipped, 2);
    }

    #[test]
    fn test_deduplicate_multiple_distinct_ids() {
        let entries = vec![
            TestEntry {
                id: Some("a".to_string()),
                ts: 100,
                stop: false,
                value: 1,
            },
            TestEntry {
                id: Some("b".to_string()),
                ts: 200,
                stop: true,
                value: 2,
            },
            TestEntry {
                id: Some("a".to_string()),
                ts: 300,
                stop: true,
                value: 3,
            },
            TestEntry {
                id: Some("c".to_string()),
                ts: 400,
                stop: false,
                value: 4,
            },
        ];
        let (mut result, skipped) = deduplicate(entries);
        result.sort_by_key(|e| e.value);
        assert_eq!(result.len(), 3); // a, b, c
        assert_eq!(result[0].value, 2); // b: completed
        assert_eq!(result[1].value, 3); // a: completed wins over non-completed
        assert_eq!(result[2].value, 4); // c: only entry (fallback to latest)
        assert_eq!(skipped, 1);
    }

    #[test]
    fn test_deduplicate_no_id_without_stop_dropped() {
        // Entries without message_id and without stop_reason are dropped
        let entries = vec![
            TestEntry {
                id: None,
                ts: 100,
                stop: false,
                value: 1,
            },
            TestEntry {
                id: None,
                ts: 200,
                stop: false,
                value: 2,
            },
        ];
        let (result, skipped) = deduplicate(entries);
        assert_eq!(result.len(), 0);
        assert_eq!(skipped, 0);
    }

    #[test]
    fn test_deduplicate_mixed_id_and_no_id() {
        let entries = vec![
            TestEntry {
                id: Some("msg1".to_string()),
                ts: 100,
                stop: true,
                value: 1,
            },
            TestEntry {
                id: None,
                ts: 200,
                stop: true,
                value: 2,
            },
            TestEntry {
                id: None,
                ts: 300,
                stop: false,
                value: 3,
            },
        ];
        let (mut result, skipped) = deduplicate(entries);
        result.sort_by_key(|e| e.value);
        assert_eq!(result.len(), 2); // msg1 + no-id-with-stop
        assert_eq!(result[0].value, 1);
        assert_eq!(result[1].value, 2);
        assert_eq!(skipped, 0);
    }

    #[test]
    fn test_dedup_accumulator_merge() {
        let mut left = DedupAccumulator::new();
        left.extend(vec![
            TestEntry {
                id: Some("msg1".to_string()),
                ts: 100,
                stop: false,
                value: 1,
            },
            TestEntry {
                id: Some("msg2".to_string()),
                ts: 100,
                stop: true,
                value: 10,
            },
        ]);

        let mut right = DedupAccumulator::new();
        right.extend(vec![
            TestEntry {
                id: Some("msg1".to_string()),
                ts: 200,
                stop: true,
                value: 2,
            },
            TestEntry {
                id: Some("msg2".to_string()),
                ts: 120,
                stop: false,
                value: 11,
            },
        ]);

        left.merge(right);
        let (mut result, skipped) = left.finalize();
        result.sort_by_key(|entry| entry.value);

        assert_eq!(result.len(), 2);
        assert_eq!(result[0].value, 2); // msg1 chooses completed entry from right chunk
        assert_eq!(result[1].value, 10); // msg2 keeps completed entry from left chunk
        assert_eq!(skipped, 2);
    }
}
