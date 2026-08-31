use super::*;

use tempfile::tempdir;

#[test]
fn gjc_child_patch_does_not_replace_inherited_project() {
    let temp = tempdir().unwrap();
    let parent = temp.path().join("session.jsonl");
    let child = temp.path().join("session/child.jsonl");
    fs::create_dir_all(child.parent().unwrap()).unwrap();
    fs::write(
        parent,
        concat!(
            r#"{"type":"session","version":5,"id":"parent","cwd":"/project"}"#,
            "\n"
        ),
    )
    .unwrap();
    fs::write(
        &child,
        concat!(
            r#"{"type":"session","version":5,"id":"child","cwd":"/worktree"}"#,
            "\n",
            r#"{"type":"header_patch","patch":{"cwd":"/patched-worktree"}}"#,
            "\n",
            r#"{"type":"message","id":"call","timestamp":"2026-08-31T03:00:00Z","message":{"role":"assistant","provider":"openai","model":"gpt-5","timestamp":1788145200000,"usage":{"input":1,"output":1,"cacheRead":0,"cacheWrite":0,"cost":{"total":0.01}}}}"#,
            "\n"
        ),
    )
    .unwrap();

    let parsed = parse_file(
        &child,
        Timezone::Named(chrono_tz::UTC),
        false,
        ForkProfile::gjc(),
    );

    assert_eq!(parsed.errors, 0);
    assert_eq!(parsed.entries.len(), 1);
    assert_eq!(parsed.entries[0].project_path, "/project");
}
