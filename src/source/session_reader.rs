//! ccstats-owned policy and projections over the shared native reader.
use agent_sessions::{
    AccountingPolicy, Agent, CodexUsageMode, EventKinds, ReadError, ReadOptions, Roots,
    SessionReader, TokenCounts,
};
use std::cell::RefCell;
use std::io::{self, BufRead, Read};
use std::rc::Rc;
use std::{fs::File, io::BufReader, path::Path};

pub(super) fn reader(
    path: &Path,
    agent: Agent,
    include: EventKinds,
    mode: CodexUsageMode,
) -> Result<SessionReader, ReadError> {
    let file = File::open(path).map_err(ReadError::Io)?;
    agent_sessions::read_from(agent, BufReader::new(file), &options(include, mode))
}

fn options(include: EventKinds, mode: CodexUsageMode) -> ReadOptions {
    ReadOptions {
        include,
        codex_usage: mode,
        accounting: AccountingPolicy::UsageStatistics,
        max_file_bytes: None,
        max_line_bytes: None,
        ..Default::default()
    }
}

/// Sees each native line before the shared reader parses it, for fields the
/// shared reader does not project. Events of a line are emitted before the
/// next line is observed.
pub(super) trait LineObserver {
    fn observe(&mut self, line: &[u8]);
}

/// Same reader as [`reader`], with `observer` fed every line first.
pub(super) fn observed_reader<O: LineObserver>(
    path: &Path,
    agent: Agent,
    include: EventKinds,
    mode: CodexUsageMode,
    observer: Rc<RefCell<O>>,
) -> Result<SessionReader<ObservedLines<O>>, ReadError> {
    let file = File::open(path).map_err(ReadError::Io)?;
    agent_sessions::read_from(
        agent,
        ObservedLines {
            file,
            buffer: Vec::new(),
            position: 0,
            line_end: 0,
            filled: 0,
            eof: false,
            observer,
        },
        &options(include, mode),
    )
}

/// Line-at-a-time reader that buffers file bytes once (no second copy per
/// line) and shows each complete line to the observer before handing it out.
pub(super) struct ObservedLines<O> {
    file: File,
    buffer: Vec<u8>,
    position: usize,
    line_end: usize,
    filled: usize,
    eof: bool,
    observer: Rc<RefCell<O>>,
}

const READ_CHUNK: usize = 64 * 1024;

impl<O> ObservedLines<O> {
    /// End of the line starting at `position`, reading more of the file as needed.
    fn next_line_end(&mut self) -> io::Result<usize> {
        let mut searched = self.position;
        loop {
            if let Some(offset) = memchr::memchr(b'\n', &self.buffer[searched..self.filled]) {
                return Ok(searched + offset + 1);
            }
            if self.eof {
                return Ok(self.filled);
            }
            searched = self.filled;
            if self.position > 0 {
                self.buffer.copy_within(self.position..self.filled, 0);
                searched -= self.position;
                self.filled -= self.position;
                self.position = 0;
            }
            if self.buffer.len() - self.filled < READ_CHUNK {
                self.buffer.resize(self.filled + READ_CHUNK, 0);
            }
            let n = self.file.read(&mut self.buffer[self.filled..])?;
            self.filled += n;
            self.eof = n == 0;
        }
    }
}

impl<O: LineObserver> Read for ObservedLines<O> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        let n = self.fill_buf()?.read(bytes)?;
        self.consume(n);
        Ok(n)
    }
}

impl<O: LineObserver> BufRead for ObservedLines<O> {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        if self.position == self.line_end {
            self.line_end = self.next_line_end()?;
            self.observer
                .borrow_mut()
                .observe(&self.buffer[self.position..self.line_end]);
        }
        Ok(&self.buffer[self.position..self.line_end])
    }

    fn consume(&mut self, n: usize) {
        self.position = (self.position + n).min(self.line_end);
    }
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
        assert_ne!(status.detail, "");
    }
}
