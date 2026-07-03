use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PricingSource {
    Live,
    Cache,
    CacheStale,
    Fallback,
    Mixed,
}

impl PricingSource {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            PricingSource::Live => "live",
            PricingSource::Cache => "cache",
            PricingSource::CacheStale => "cache_stale",
            PricingSource::Fallback => "fallback",
            PricingSource::Mixed => "mixed",
        }
    }

    pub(crate) fn combine(self, other: Self) -> Self {
        if self == other {
            self
        } else {
            PricingSource::Mixed
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct CacheMetadata {
    pub(super) age: Duration,
    pub(super) modified: SystemTime,
}

impl CacheMetadata {
    pub(crate) fn age_seconds(self) -> u64 {
        self.age.as_secs()
    }

    pub(crate) fn modified_epoch_seconds(self) -> u64 {
        self.modified
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_secs()
    }
}
