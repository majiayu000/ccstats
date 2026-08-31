//! Public projection of the internal usage-source registry.

use serde::{Deserialize, Serialize};

use crate::sdk::{SdkError, UsageSource};
use crate::source::all_sources;

/// Stable source metadata for graphical and SDK consumers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "serialized source capability flags are independent"
)]
pub struct SourceDescriptor {
    pub source: UsageSource,
    pub name: String,
    pub display_name: String,
    pub aliases: Vec<String>,
    pub has_projects: bool,
    pub has_reasoning_tokens: bool,
    pub has_cache_creation: bool,
    pub has_cache_read: bool,
}

/// List every source currently registered by ccstats.
///
/// # Errors
///
/// Returns [`SdkError::InvalidSource`] when a registry entry has no matching
/// public [`UsageSource`] variant.
pub fn list_usage_sources() -> Result<Vec<SourceDescriptor>, SdkError> {
    all_sources()
        .map(|source| {
            let capabilities = source.capabilities();
            Ok(SourceDescriptor {
                source: source.name().parse::<UsageSource>()?,
                name: source.name().to_string(),
                display_name: source.display_name().to_string(),
                aliases: source
                    .aliases()
                    .iter()
                    .map(|alias| (*alias).to_string())
                    .collect(),
                has_projects: capabilities.has_projects,
                has_reasoning_tokens: capabilities.has_reasoning_tokens,
                has_cache_creation: capabilities.has_cache_creation,
                has_cache_read: capabilities.has_cache_read,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn catalog_is_a_complete_unique_registry_projection() {
        let descriptors = list_usage_sources().expect("registry projection");
        let names = descriptors
            .iter()
            .map(|descriptor| descriptor.name.as_str())
            .collect::<HashSet<_>>();

        assert_eq!(descriptors.len(), 29);
        assert_eq!(names.len(), descriptors.len());
        assert_eq!(
            descriptors
                .iter()
                .map(|descriptor| descriptor.name.as_str())
                .collect::<Vec<_>>(),
            crate::source::all_sources()
                .map(crate::source::Source::name)
                .collect::<Vec<_>>()
        );
    }
}
