//! Native title indices; prompt/response bodies are never used as fallback.
use crate::UsageSource;
pub use agent_sessions::{SessionTitle, SessionTitleOrigin};
use std::{collections::HashMap, io};
/// Load requested native titles. Missing indices return no titles; malformed
/// or unreadable indices return an error independently of usage statistics.
///
/// # Errors
/// Returns an error for invalid roots, unreadable indices or malformed index data.
pub fn load_session_titles(
    source: UsageSource,
    ids: &[String],
) -> io::Result<HashMap<String, SessionTitle>> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let agent = match source.as_str() {
        "claude" => agent_sessions::Agent::ClaudeCode,
        "codex" => agent_sessions::Agent::Codex,
        _ => return Ok(HashMap::new()),
    };
    agent_sessions::load_session_titles(agent, &agent_sessions::Roots::from_env_for(agent)?, ids)
}
#[cfg(test)]
fn codex_titles(
    path: &std::path::Path,
    ids: &std::collections::HashSet<&str>,
) -> io::Result<HashMap<String, SessionTitle>> {
    agent_sessions::load_session_titles(
        agent_sessions::Agent::Codex,
        &agent_sessions::Roots {
            claude: None,
            codex: path.parent().map(Into::into),
        },
        &ids.iter().map(|id| (*id).to_owned()).collect::<Vec<_>>(),
    )
}
#[cfg(test)]
fn claude_titles(
    projects: &std::path::Path,
    ids: &std::collections::HashSet<&str>,
) -> io::Result<HashMap<String, SessionTitle>> {
    agent_sessions::load_session_titles(
        agent_sessions::Agent::ClaudeCode,
        &agent_sessions::Roots {
            claude: projects.parent().map(Into::into),
            codex: None,
        },
        &ids.iter().map(|id| (*id).to_owned()).collect::<Vec<_>>(),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::HashSet, fs};

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
        let project = dir.path().join("projects/project");
        fs::create_dir_all(&project).unwrap();
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
        let titles = claude_titles(
            &dir.path().join("projects"),
            &HashSet::from(["one", "two", "three"]),
        )
        .unwrap();
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
        fs::create_dir_all(dir.path().join("projects/project")).unwrap();
        fs::write(
            dir.path().join("projects/project/sessions-index.json"),
            "{}",
        )
        .unwrap();
        assert_eq!(
            claude_titles(&dir.path().join("projects"), &ids)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn index_io_errors_are_not_treated_as_missing_titles() {
        let dir = tempfile::tempdir().unwrap();
        let index = dir.path().join("session_index.jsonl");
        fs::create_dir(&index).unwrap();
        assert!(codex_titles(&index, &HashSet::from(["one"])).is_err());
        let file = dir.path().join("projects");
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
