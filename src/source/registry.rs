//! Data source registry
//!
//! Manages all available data sources and provides lookup by name/alias.

use std::sync::LazyLock;

use super::claude::ClaudeSource;
use super::codex::CodexSource;
use super::cursor::CursorSource;
use super::{BoxedSource, Source};

/// Pseudo-source that aggregates every registered source.
pub(crate) const ALL_SOURCES: &str = "all";

/// All registered data sources
static SOURCES: LazyLock<Vec<BoxedSource>> = LazyLock::new(|| {
    vec![
        Box::new(ClaudeSource::new()),
        Box::new(CodexSource::new()),
        Box::new(CursorSource::new()),
        // Add new sources here:
        // Box::new(WindsurfSource::new()),
    ]
});

/// Iterate all registered sources.
pub(crate) fn all_sources() -> impl Iterator<Item = &'static dyn Source> {
    SOURCES.iter().map(std::convert::AsRef::as_ref)
}

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

/// Return available source names and aliases for CLI hints.
pub(crate) fn source_choices() -> Vec<&'static str> {
    let mut choices = vec![ALL_SOURCES];
    for source in SOURCES.iter() {
        choices.push(source.name());
        choices.extend(source.aliases());
    }
    choices.sort_unstable();
    choices.dedup();
    choices
}

/// Suggest the most likely source token (name or alias) for invalid input.
pub(crate) fn suggest_source(input: &str) -> Option<&'static str> {
    let needle = input.trim().to_lowercase();
    if needle.is_empty() {
        return None;
    }
    if ALL_SOURCES.starts_with(&needle) || needle.starts_with(ALL_SOURCES) {
        return Some(ALL_SOURCES);
    }

    let mut best: Option<(&'static str, usize)> = None;
    for source in SOURCES.iter() {
        let mut tokens = Vec::with_capacity(1 + source.aliases().len());
        tokens.push(source.name());
        tokens.extend(source.aliases());

        for token in tokens {
            if token == needle {
                return Some(token);
            }

            if token.starts_with(&needle) || needle.starts_with(token) {
                return Some(token);
            }

            let distance = edit_distance(token, &needle);
            if distance <= 2 {
                match best {
                    Some((_, best_distance)) if distance >= best_distance => {}
                    _ => best = Some((token, distance)),
                }
            }
        }
    }
    best.map(|(token, _)| token)
}

fn edit_distance(a: &str, b: &str) -> usize {
    if a == b {
        return 0;
    }
    if a.is_empty() {
        return b.chars().count();
    }
    if b.is_empty() {
        return a.chars().count();
    }

    let b_chars: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b_chars.len()).collect();
    let mut curr = vec![0; b_chars.len() + 1];

    for (i, a_ch) in a.chars().enumerate() {
        curr[0] = i + 1;
        for (j, b_ch) in b_chars.iter().enumerate() {
            let cost = usize::from(a_ch != *b_ch);
            curr[j + 1] = (prev[j + 1] + 1).min(curr[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[b_chars.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_source_by_name() {
        assert!(get_source("claude").is_some());
        assert!(get_source("codex").is_some());
        assert!(get_source("cursor").is_some());
        assert!(get_source("unknown").is_none());
    }

    #[test]
    fn test_get_source_by_alias() {
        assert!(get_source("cc").is_some());
        assert!(get_source("cx").is_some());
        assert!(get_source("cur").is_some());
    }

    #[test]
    fn test_get_source_case_insensitive() {
        assert!(get_source("Claude").is_some());
        assert!(get_source("CLAUDE").is_some());
        assert!(get_source("Codex").is_some());
        assert!(get_source("Cursor").is_some());
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
    fn test_cursor_source_properties() {
        let source = get_source("cursor").unwrap();
        assert_eq!(source.name(), "cursor");
        assert_eq!(source.display_name(), "Cursor");
        assert!(source.aliases().contains(&"cur"));
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
    fn test_cursor_capabilities() {
        let source = get_source("cursor").unwrap();
        let caps = source.capabilities();
        assert!(!caps.has_projects);
        assert!(!caps.has_billing_blocks);
        assert!(!caps.has_cache_creation);
        assert!(!caps.needs_dedup);
        assert!(!caps.has_reasoning_tokens);
    }

    #[test]
    fn test_sources_count() {
        // Verify all built-in sources are registered
        assert_eq!(SOURCES.len(), 3);
    }

    #[test]
    fn test_alias_resolves_to_correct_source() {
        let by_name = get_source("claude").unwrap();
        let by_alias = get_source("cc").unwrap();
        assert_eq!(by_name.name(), by_alias.name());

        let by_name = get_source("codex").unwrap();
        let by_alias = get_source("cx").unwrap();
        assert_eq!(by_name.name(), by_alias.name());

        let by_name = get_source("cursor").unwrap();
        let by_alias = get_source("cur").unwrap();
        assert_eq!(by_name.name(), by_alias.name());
    }

    #[test]
    fn test_source_choices_include_names_and_aliases() {
        let choices = source_choices();
        assert!(choices.contains(&"all"));
        assert!(choices.contains(&"claude"));
        assert!(choices.contains(&"codex"));
        assert!(choices.contains(&"cursor"));
        assert!(choices.contains(&"cc"));
        assert!(choices.contains(&"cx"));
        assert!(choices.contains(&"cur"));
    }

    #[test]
    fn test_suggest_source_prefix_and_typo() {
        assert_eq!(suggest_source("clau"), Some("claude"));
        assert_eq!(suggest_source("code"), Some("codex"));
        assert_eq!(suggest_source("curs"), Some("cursor"));
        assert_eq!(suggest_source("claud"), Some("claude"));
        assert_eq!(suggest_source("al"), Some("all"));
        assert_eq!(suggest_source("cx"), Some("cx"));
    }

    #[test]
    fn test_suggest_source_none_for_distant_input() {
        assert_eq!(suggest_source("totally-unknown"), None);
    }
}
