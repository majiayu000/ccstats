//! Data source registry
//!
//! Manages all available data sources and provides lookup by name/alias.

use std::sync::LazyLock;

use super::claude::ClaudeSource;
use super::codex::CodexSource;
use super::{BoxedSource, Source};

/// All registered data sources
static SOURCES: LazyLock<Vec<BoxedSource>> = LazyLock::new(|| {
    vec![
        Box::new(ClaudeSource::new()),
        Box::new(CodexSource::new()),
        // Add new sources here:
        // Box::new(CursorSource::new()),
        // Box::new(WindsurfSource::new()),
    ]
});

/// Get a source by name or alias
pub(crate) fn get_source(name: &str) -> Option<&'static dyn Source> {
    let name_lower = name.to_lowercase();
    SOURCES.iter().find_map(|s: &BoxedSource| {
        if s.name() == name_lower || s.aliases().contains(&name_lower.as_str()) {
            Some(s.as_ref())
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_source_by_name() {
        assert!(get_source("claude").is_some());
        assert!(get_source("codex").is_some());
        assert!(get_source("unknown").is_none());
    }

    #[test]
    fn test_get_source_by_alias() {
        assert!(get_source("cc").is_some());
    }
}
