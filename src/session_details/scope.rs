//! File selection happens before parsing so unrelated damage cannot poison a report.
use crate::{
    source::{Capabilities, ParseOutput, Source},
    utils::Timezone,
};
use agent_sessions::{
    AccountingPolicy, Agent, Discovery, Event, EventKinds, FileKind, Origin, ReadOptions, Roots,
};
use std::{
    fs::File,
    io::BufReader,
    path::{Path, PathBuf},
};

pub(super) struct Selection<'a> {
    source: &'a dyn Source,
    files: Vec<PathBuf>,
    partition: String,
    pub(super) discovery_errors: usize,
    pub(super) unattributed_files: usize,
}

fn native_context(agent: Agent, path: &Path) -> (Option<String>, bool) {
    let options = ReadOptions {
        include: EventKinds::META,
        accounting: AccountingPolicy::UsageStatistics,
        max_file_bytes: None,
        max_line_bytes: None,
        ..Default::default()
    };
    let reader = File::open(path)
        .map_err(agent_sessions::ReadError::Io)
        .and_then(|file| agent_sessions::read_from(agent, BufReader::new(file), &options));
    let Ok(reader) = reader else {
        return (None, false);
    };
    let mut subagent = false;
    for item in reader {
        if let Ok(item) = item
            && let Event::Meta(meta) = item.value
        {
            subagent |= meta.origin == Some(Origin::Subagent);
            if let Some(cwd) = meta.cwd.filter(|v| !v.is_empty()) {
                return (Some(cwd), subagent);
            }
        }
    }
    (None, subagent)
}

impl<'a> Selection<'a> {
    pub(super) fn new(
        source: &'a dyn Source,
        agent: Agent,
        roots: &Roots,
        discovery: Discovery,
        workdirs: &[String],
        exclude_subagents: bool,
    ) -> Self {
        let mut workdirs = workdirs.to_vec();
        workdirs.sort();
        workdirs.dedup();
        let project_roots: Vec<_> = roots
            .claude
            .iter()
            .flat_map(|root| {
                workdirs
                    .iter()
                    .map(move |wd| root.join("projects").join(wd.replace('/', "-")))
            })
            .collect();
        let discovery_errors = discovery
            .errors
            .iter()
            .filter(|error| {
                if exclude_subagents
                    && error
                        .path
                        .components()
                        .any(|p| p.as_os_str() == "subagents")
                {
                    return false;
                }
                agent != Agent::ClaudeCode
                    || workdirs.is_empty()
                    || project_roots
                        .iter()
                        .any(|root| error.path.starts_with(root))
                    || roots
                        .claude
                        .as_ref()
                        .is_some_and(|root| error.path == root.join("projects"))
            })
            .count();
        let mut unattributed_files = 0;
        let files = discovery
            .files
            .into_iter()
            .filter_map(|file| {
                if exclude_subagents && file.kind == FileKind::Subagent {
                    return None;
                }
                if workdirs.is_empty() && !exclude_subagents {
                    return Some(file.path);
                }
                // Preserve Claude's original directory scope even for malformed files
                // or old records with no native cwd.
                if agent == Agent::ClaudeCode
                    && project_roots.iter().any(|root| file.path.starts_with(root))
                {
                    return Some(file.path);
                }
                let (cwd, subagent) = native_context(agent, &file.path);
                if exclude_subagents && subagent {
                    return None;
                }
                if workdirs.is_empty() {
                    return Some(file.path);
                }
                let Some(cwd) = cwd else {
                    if agent == Agent::Codex {
                        unattributed_files += 1;
                    }
                    return None;
                };
                workdirs.binary_search(&cwd).is_ok().then_some(file.path)
            })
            .collect();
        let partition = format!(
            "{}:scope={}",
            source.cache_partition(),
            serde_json::json!({"workdirs": workdirs, "exclude_subagents": exclude_subagents})
        );
        Self {
            source,
            files,
            partition,
            discovery_errors,
            unattributed_files,
        }
    }
}

impl Source for Selection<'_> {
    fn name(&self) -> &'static str {
        self.source.name()
    }
    fn capabilities(&self) -> Capabilities {
        self.source.capabilities()
    }
    fn find_files(&self) -> Vec<PathBuf> {
        self.files.clone()
    }
    fn parse_file(&self, path: &Path, timezone: Timezone, debug: bool) -> ParseOutput {
        self.source.parse_file(path, timezone, debug)
    }
    fn cache_policy(&self) -> crate::source::CachePolicy {
        self.source.cache_policy()
    }
    fn finalize_entries(&self, entries: Vec<crate::core::RawEntry>) -> Vec<crate::core::RawEntry> {
        self.source.finalize_entries(entries)
    }
    fn cache_partition(&self) -> &str {
        &self.partition
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cache_scope_is_canonical_and_includes_subagent_policy() {
        let source =
            crate::source::CodexSource::with_accounting_diagnostics(crate::source::CodexScope::All);
        let make = |workdirs: &[&str], exclude| {
            Selection::new(
                &source,
                Agent::Codex,
                &Roots::default(),
                Discovery::default(),
                &workdirs.iter().map(|v| (*v).into()).collect::<Vec<_>>(),
                exclude,
            )
            .partition
        };
        assert_eq!(make(&["/b", "/a", "/a"], false), make(&["/a", "/b"], false));
        assert_ne!(make(&["/a"], false), make(&["/b"], false));
        assert_ne!(make(&["/a"], false), make(&["/a"], true));
    }
}
