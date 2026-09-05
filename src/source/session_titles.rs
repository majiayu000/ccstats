//! Optional source metadata, independent of usage parsing and accounting.

use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::UsageSource;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionTitleOrigin {
    SourceTitle,
    SourceSummary,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionTitle {
    pub text: String,
    pub origin: SessionTitleOrigin,
}

/// Reads existing titles for the requested session IDs, without generating text.
///
/// Codex uses `CODEX_HOME/session_index.jsonl` (default `~/.codex`); the last
/// nonempty name in append order wins, as in Codex's batch name lookup.
/// Claude uses `CLAUDE_CONFIG_DIR/projects/*/sessions-index.json` (default
/// `~/.claude`) and its existing `summary` field. Prompt/response bodies are
/// neither used nor returned. Missing indices and other sources yield no titles.
/// Callers should display a project/time/ID label for sessions with no title.
///
/// Titles are keyed by session ID within ONE source; callers must include the
/// source when persisting their own overrides. This function never writes files
/// or changes statistical grouping, and does not require pricing/network access.
///
/// # Errors
///
/// Returns an error for unreadable or malformed indices. Callers can report this
/// metadata error separately while still displaying independently loaded usage.
pub fn load_session_titles(
    source: UsageSource,
    session_ids: &[String],
) -> io::Result<HashMap<String, SessionTitle>> {
    if session_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let ids = session_ids.iter().map(String::as_str).collect();
    match source.as_str() {
        "codex" => super::codex::codex_root_candidate().map_or_else(
            || Ok(HashMap::new()),
            |root| codex_titles(&root.join("session_index.jsonl"), &ids),
        ),
        "claude" => super::claude::claude_projects_dir().map_or_else(
            || Ok(HashMap::new()),
            |projects| claude_titles(&projects, &ids),
        ),
        _ => Ok(HashMap::new()),
    }
}

fn contextual_error(path: &Path, error: &io::Error) -> io::Error {
    io::Error::new(error.kind(), format!("{}: {error}", path.display()))
}

fn open_index(path: &Path) -> io::Result<Option<File>> {
    match File::open(path) {
        Ok(file) => Ok(Some(file)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(contextual_error(path, &error)),
    }
}

fn malformed_index(path: &Path, line: usize, column: usize) -> io::Error {
    // Do not echo potentially private field values into diagnostics.
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!(
            "Malformed session title index {} at line {line}, column {column}",
            path.display()
        ),
    )
}

#[derive(Deserialize)]
struct CodexEntry {
    id: String,
    thread_name: String,
}

fn codex_titles(path: &Path, ids: &HashSet<&str>) -> io::Result<HashMap<String, SessionTitle>> {
    let mut titles = HashMap::new();
    let Some(file) = open_index(path)? else {
        return Ok(titles);
    };
    for (index, line) in BufReader::new(file).lines().enumerate() {
        let line = line.map_err(|error| contextual_error(path, &error))?;
        if line.trim().is_empty() {
            continue;
        }
        let entry: CodexEntry = serde_json::from_str(&line)
            .map_err(|error| malformed_index(path, index + 1, error.column()))?;
        let text = entry.thread_name.trim();
        if ids.contains(entry.id.as_str()) && !text.is_empty() {
            titles.insert(
                entry.id,
                SessionTitle {
                    text: text.to_owned(),
                    origin: SessionTitleOrigin::SourceTitle,
                },
            );
        }
    }
    Ok(titles)
}

#[derive(Deserialize)]
struct ClaudeIndex {
    entries: Vec<ClaudeEntry>,
}

#[derive(Deserialize)]
struct ClaudeEntry {
    #[serde(rename = "sessionId")]
    session_id: String,
    summary: Option<String>,
}

fn claude_titles(
    projects: &Path,
    ids: &HashSet<&str>,
) -> io::Result<HashMap<String, SessionTitle>> {
    let mut titles = HashMap::new();
    let directories = match fs::read_dir(projects) {
        Ok(directories) => directories,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(titles),
        Err(error) => return Err(contextual_error(projects, &error)),
    };
    for directory in directories {
        let directory = directory.map_err(|error| contextual_error(projects, &error))?;
        if !directory
            .file_type()
            .map_err(|error| contextual_error(&directory.path(), &error))?
            .is_dir()
        {
            continue;
        }
        let path = directory.path().join("sessions-index.json");
        let Some(file) = open_index(&path)? else {
            continue;
        };
        let index: ClaudeIndex = serde_json::from_reader(BufReader::new(file))
            .map_err(|error| malformed_index(&path, error.line(), error.column()))?;
        for entry in index.entries {
            if let Some(text) = entry
                .summary
                .as_deref()
                .map(str::trim)
                .filter(|text| !text.is_empty())
                && ids.contains(entry.session_id.as_str())
            {
                titles.insert(
                    entry.session_id,
                    SessionTitle {
                        text: text.to_owned(),
                        origin: SessionTitleOrigin::SourceSummary,
                    },
                );
            }
        }
    }
    Ok(titles)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_uses_append_order_filters_ids_and_preserves_unicode() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session_index.jsonl");
        let content = concat!(
            "{\"id\":\"one\",\"thread_name\":\"old\",\"updated_at\":\"2026-09-05\"}\n",
            "{\"id\":\"other\",\"thread_name\":\"not requested\"}\n",
            "{\"id\":\"one\",\"thread_name\":\" 修复缓存 🛠️ \",\"updated_at\":\"2026-09-04\"}\n",
            "{\"id\":\"one\",\"thread_name\":\"  \"}\n\n",
        );
        fs::write(&path, content).unwrap();
        let titles = codex_titles(&path, &HashSet::from(["one", "absent"])).unwrap();
        assert_eq!(titles.len(), 1);
        assert_eq!(titles["one"].text, "修复缓存 🛠️");
        assert_eq!(titles["one"].origin, SessionTitleOrigin::SourceTitle);
        assert_eq!(fs::read_to_string(path).unwrap(), content);
    }

    #[test]
    fn claude_reuses_summary_without_using_first_prompt_or_opening_transcripts() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("project");
        fs::create_dir(&project).unwrap();
        let path = project.join("sessions-index.json");
        let content = r#"{"entries":[
            {"sessionId":"one","summary":"  完善统计  ","firstPrompt":"private prompt"},
            {"sessionId":"two","firstPrompt":"do not use this as a title"},
            {"sessionId":"three","summary":"   "},
            {"sessionId":"other","summary":"not requested"}
        ]}"#;
        fs::write(&path, content).unwrap();
        fs::write(
            project.join("one.jsonl"),
            "not valid JSON; must not be read",
        )
        .unwrap();
        let titles = claude_titles(dir.path(), &HashSet::from(["one", "two", "three"])).unwrap();
        assert_eq!(titles.len(), 1);
        assert_eq!(titles["one"].text, "完善统计");
        assert_eq!(titles["one"].origin, SessionTitleOrigin::SourceSummary);
        assert_eq!(fs::read_to_string(path).unwrap(), content);
    }

    #[test]
    fn missing_indices_are_normal_but_malformed_indices_fail_explicitly() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session_index.jsonl");
        let ids = HashSet::from(["one"]);
        assert!(codex_titles(&path, &ids).unwrap().is_empty());
        assert!(
            claude_titles(&dir.path().join("absent"), &ids)
                .unwrap()
                .is_empty()
        );
        fs::write(&path, "\n{private malformed text").unwrap();
        let error = codex_titles(&path, &ids).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("line 2"));
        assert!(!error.to_string().contains("private malformed text"));
        fs::create_dir(dir.path().join("project")).unwrap();
        fs::write(dir.path().join("project/sessions-index.json"), "{}").unwrap();
        assert_eq!(
            claude_titles(dir.path(), &ids).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn index_io_errors_are_not_treated_as_missing_titles() {
        let dir = tempfile::tempdir().unwrap();
        assert!(codex_titles(dir.path(), &HashSet::from(["one"])).is_err());
        let file = dir.path().join("file");
        fs::write(&file, "").unwrap();
        assert!(claude_titles(&file, &HashSet::from(["one"])).is_err());
    }

    #[test]
    fn unsupported_sources_and_empty_requests_do_not_load_metadata() {
        assert!(
            load_session_titles("cursor".parse().unwrap(), &["one".into()])
                .unwrap()
                .is_empty()
        );
        assert!(
            load_session_titles("codex".parse().unwrap(), &[])
                .unwrap()
                .is_empty()
        );
    }
}
