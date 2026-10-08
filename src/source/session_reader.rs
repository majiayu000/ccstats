//! ccstats-owned policy and projections over the shared native reader.
use agent_sessions::{
    AccountingPolicy, Agent, CodexUsageMode, EventKinds, ReadError, ReadOptions, Roots,
    SessionReader, TokenCounts,
};
use std::{fs::File, io::BufReader, path::Path};

pub(super) fn reader(
    path: &Path,
    agent: Agent,
    include: EventKinds,
    mode: CodexUsageMode,
) -> Result<SessionReader, ReadError> {
    let file = File::open(path).map_err(ReadError::Io)?;
    agent_sessions::read_from(
        agent,
        BufReader::new(file),
        &ReadOptions {
            include,
            codex_usage: mode,
            accounting: AccountingPolicy::UsageStatistics,
            max_file_bytes: None,
            max_line_bytes: None,
            ..Default::default()
        },
    )
}

pub(super) fn roots(agent: Agent) -> Roots {
    match Roots::from_env_for(agent) {
        Ok(roots) => roots,
        Err(error) => {
            eprintln!("Session root configuration: {error}");
            Roots::default()
        }
    }
}

pub(super) fn discover(agent: Agent) -> (Vec<std::path::PathBuf>, usize) {
    match Roots::from_env_for(agent) {
        Ok(roots) => files(&roots, agent),
        Err(error) => {
            eprintln!("Session root configuration: {error}");
            (Vec::new(), 1)
        }
    }
}

pub(super) fn files(roots: &Roots, agent: Agent) -> (Vec<std::path::PathBuf>, usize) {
    let discovery = agent_sessions::discover(
        roots,
        &agent_sessions::DiscoverFilter {
            agents: vec![agent],
            include_subagents: true,
            ..Default::default()
        },
    );
    let errors = discovery.errors.len();
    for error in discovery.errors {
        eprintln!(
            "Cannot discover session files at {}: {}",
            error.path.display(),
            error.source
        );
    }
    (
        discovery.files.into_iter().map(|f| f.path).collect(),
        errors,
    )
}

pub(super) fn diagnose(agent: Agent) -> super::SourceDiagnostic {
    match Roots::from_env_for(agent) {
        Ok(roots) => diagnose_roots(&roots, agent),
        Err(error) => super::SourceDiagnostic::error(error.to_string()),
    }
}

fn diagnose_roots(roots: &Roots, agent: Agent) -> super::SourceDiagnostic {
    let discovery = agent_sessions::discover(
        roots,
        &agent_sessions::DiscoverFilter {
            agents: vec![agent],
            include_subagents: true,
            ..Default::default()
        },
    );
    if !discovery.errors.is_empty() {
        return super::SourceDiagnostic::error(
            discovery
                .errors
                .iter()
                .map(|error| format!("{}: {}", error.path.display(), error.source))
                .collect::<Vec<_>>()
                .join("; "),
        );
    }
    let count = discovery.files.len();
    if count == 0 {
        super::SourceDiagnostic::missing("No local usage files found")
    } else {
        super::SourceDiagnostic::detected(count, format!("Found {count} local usage file(s)"))
    }
}

/// Explicit application aggregation: absent buckets historically count as zero.
/// The shared event still retains absence separately from reported zero.
pub(super) fn buckets(c: TokenCounts) -> Option<[i64; 7]> {
    let values = [
        c.input,
        c.cache_read,
        c.cache_write,
        c.output,
        c.reasoning,
        c.reported_total,
        c.cache_write_1h,
    ];
    let mut result = [0; 7];
    for (i, value) in values.into_iter().enumerate() {
        result[i] = i64::try_from(value.unwrap_or(0)).ok()?;
    }
    Some(result)
}

#[cfg(test)]
mod diagnostic_tests {
    use super::*;
    #[test]
    fn discovery_errors_are_not_missing_sources() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("codex");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("sessions"), "not a directory").unwrap();
        let status = diagnose_roots(
            &Roots {
                codex: Some(root),
                claude: None,
            },
            Agent::Codex,
        );
        assert_eq!(status.status, crate::source::DiagnosticStatus::Error);
        assert!(!status.detail.is_empty());
    }
}
