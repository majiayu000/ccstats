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

pub(super) fn files(roots: &Roots, agent: Agent) -> Vec<std::path::PathBuf> {
    let discovery = agent_sessions::discover(
        roots,
        &agent_sessions::DiscoverFilter {
            agents: vec![agent],
            include_subagents: true,
            ..Default::default()
        },
    );
    for error in discovery.errors {
        eprintln!(
            "Cannot discover session files at {}: {}",
            error.path.display(),
            error.source
        );
    }
    discovery.files.into_iter().map(|f| f.path).collect()
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
