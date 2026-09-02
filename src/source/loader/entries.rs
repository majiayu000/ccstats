use crate::core::{DateFilter, RawEntry};
use crate::utils::Timezone;

use super::DataLoader;

impl DataLoader<'_> {
    pub(in crate::source) fn load_entries(
        &self,
        filter: &DateFilter,
        timezone: Timezone,
    ) -> (Vec<RawEntry>, i64, usize) {
        if self.source.capabilities().needs_dedup {
            return self.load_deduped_entries_incremental(filter, timezone);
        }

        match self.par_process(
            filter,
            timezone,
            |filtered| filtered,
            Vec::new,
            |mut entries, partial| {
                entries.extend(partial);
                entries
            },
        ) {
            Some((entries, parse_errors)) => {
                (self.source.finalize_entries(entries), 0, parse_errors)
            }
            None => (Vec::new(), 0, 0),
        }
    }
}
