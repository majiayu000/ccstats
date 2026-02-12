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
        assert!(get_source("cx").is_some());
    }

    #[test]
    fn test_get_source_case_insensitive() {
        assert!(get_source("Claude").is_some());
        assert!(get_source("CLAUDE").is_some());
        assert!(get_source("Codex").is_some());
        assert!(get_source("CC").is_some());
    }

    #[test]
    fn test_claude_source_properties() {
        let source = get_source("claude").unwrap();
        assert_eq!(source.name(), "claude");
        assert_eq!(source.display_name(), "Claude Code");
        assert!(source.aliases().contains(&"cc"));
    }

    #[test]
    fn test_codex_source_properties() {
        let source = get_source("codex").unwrap();
        assert_eq!(source.name(), "codex");
        assert_eq!(source.display_name(), "OpenAI Codex");
        assert!(source.aliases().contains(&"cx"));
    }

    #[test]
    fn test_claude_capabilities() {
        let source = get_source("claude").unwrap();
        let caps = source.capabilities();
        assert!(caps.has_projects);
        assert!(caps.has_billing_blocks);
        assert!(caps.has_cache_creation);
        assert!(caps.needs_dedup);
        assert!(!caps.has_reasoning_tokens);
    }

    #[test]
    fn test_codex_capabilities() {
        let source = get_source("codex").unwrap();
        let caps = source.capabilities();
        assert!(!caps.has_projects);
        assert!(!caps.has_billing_blocks);
        assert!(!caps.has_cache_creation);
        assert!(!caps.needs_dedup);
        assert!(caps.has_reasoning_tokens);
    }

    #[test]
    fn test_sources_count() {
        // Verify we have exactly 2 registered sources
        assert_eq!(SOURCES.len(), 2);
    }

    #[test]
    fn test_alias_resolves_to_correct_source() {
        let by_name = get_source("claude").unwrap();
        let by_alias = get_source("cc").unwrap();
        assert_eq!(by_name.name(), by_alias.name());

        let by_name = get_source("codex").unwrap();
        let by_alias = get_source("cx").unwrap();
        assert_eq!(by_name.name(), by_alias.name());
    }
}
