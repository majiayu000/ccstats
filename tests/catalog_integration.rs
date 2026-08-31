use ccstats::list_usage_sources;

#[test]
fn catalog_projects_every_registered_usage_source() {
    let sources = list_usage_sources().expect("registry should map to public usage sources");

    assert_eq!(sources.len(), 29);
    assert_eq!(
        sources.first().map(|source| source.name.as_str()),
        Some("claude")
    );
    assert_eq!(
        sources.last().map(|source| source.name.as_str()),
        Some("dsh")
    );

    let claude = sources
        .iter()
        .find(|source| source.name == "claude")
        .expect("Claude descriptor");
    assert!(claude.has_projects);
    assert!(claude.has_cache_creation);
    assert!(claude.has_cache_read);
    assert!(!claude.has_reasoning_tokens);

    let codex = sources
        .iter()
        .find(|source| source.name == "codex")
        .expect("Codex descriptor");
    assert!(!codex.has_projects);
    assert!(!codex.has_cache_creation);
    assert!(codex.has_cache_read);
    assert!(codex.has_reasoning_tokens);
}
