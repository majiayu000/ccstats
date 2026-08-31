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

#[test]
fn gjc_partial_child_subtracts_only_emitted_usage_from_parent_rollup() {
    let temp = tempdir().unwrap();
    let parent = temp.path().join("session.jsonl");
    let child = temp.path().join("session/child.jsonl");
    fs::create_dir_all(child.parent().unwrap()).unwrap();
    fs::write(
        &parent,
        concat!(
            r#"{"type":"session","version":5,"id":"parent","cwd":"/project"}"#,
            "\n",
            r#"{"type":"message","id":"task","timestamp":"2026-08-31T03:00:00Z","message":{"role":"toolResult","toolName":"task","timestamp":1788145200000,"details":{"usage":{"input":100,"output":10,"cacheRead":0,"cacheWrite":0,"cost":{"total":1.0}},"results":[{"id":"child","usage":{"input":80,"output":8,"cacheRead":0,"cacheWrite":0,"cost":{"total":0.8}}}]}}}"#,
            "\n"
        ),
    )
    .unwrap();
    fs::write(
        &child,
        concat!(
            r#"{"type":"session","version":5,"id":"child","cwd":"/worktree"}"#,
            "\n",
            r#"{"type":"message","id":"call","timestamp":"2026-08-31T03:00:01Z","message":{"role":"assistant","provider":"openai","model":"gpt-5","timestamp":1788145201000,"usage":{"input":20,"output":2,"cacheRead":0,"cacheWrite":0,"cost":{"total":0.2}}}}"#,
            "\n",
            "{malformed}\n"
        ),
    )
    .unwrap();

    let parent_output = parse_file(
        &parent,
        Timezone::Named(chrono_tz::UTC),
        false,
        ForkProfile::gjc(),
    );
    let child_output = parse_file(
        &child,
        Timezone::Named(chrono_tz::UTC),
        false,
        ForkProfile::gjc(),
    );

    assert_eq!(child_output.errors, 1);
    assert_eq!(child_output.entries.len(), 1);
    assert_eq!(parent_output.errors, 0);
    assert_eq!(parent_output.entries.len(), 1);
    assert_eq!(parent_output.entries[0].input_tokens, 80);
    assert_eq!(parent_output.entries[0].output_tokens, 8);
    assert_eq!(parent_output.entries[0].recorded_cost_usd, Some(0.8));
    assert_eq!(
        parent_output.entries[0].input_tokens + child_output.entries[0].input_tokens,
        100
    );
    assert_eq!(
        parent_output.entries[0].output_tokens + child_output.entries[0].output_tokens,
        10
    );
    let total_cost = parent_output.entries[0].recorded_cost_usd.unwrap()
        + child_output.entries[0].recorded_cost_usd.unwrap();
    assert!((total_cost - 1.0).abs() < f64::EPSILON);
}
