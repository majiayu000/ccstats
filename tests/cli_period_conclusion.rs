mod common;

use chrono::Utc;
use common::{run_ccstats, unique_temp_dir, write_file};
use std::fs;

fn write_today_claude(root: &std::path::Path, input_tokens: i64, output_tokens: i64) {
    let today = Utc::now().format("%Y-%m-%dT12:00:00Z");
    let path = root.join(".claude/projects/myproject/session-a.jsonl");
    write_file(
        &path,
        &format!(
            r#"{{"timestamp":"{today}","message":{{"id":"msg_1","model":"claude-3-5-sonnet-20241022","stop_reason":"end_turn","usage":{{"input_tokens":{input_tokens},"output_tokens":{output_tokens}}}}}}}
"#
        ),
    );
}

fn first_content_line(stdout: &str) -> &str {
    stdout
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("")
}

fn cost_in(text: &str) -> Option<String> {
    let start = text.find('$')?;
    let rest = &text[start..];
    let mut end = 1;
    let mut seen_dot = false;
    for (i, c) in rest.char_indices().skip(1) {
        if c.is_ascii_digit() || c == ',' {
            end = i + c.len_utf8();
        } else if c == '.' && !seen_dot {
            seen_dot = true;
            end = i + c.len_utf8();
        } else {
            break;
        }
    }
    let token = rest[..end].to_string();
    (token.len() > 1).then_some(token)
}

fn footer_cost(stdout: &str) -> Option<String> {
    stdout
        .lines()
        .find(|line| line.contains("TOTAL"))
        .and_then(cost_in)
}

#[test]
fn today_table_prints_conclusion_before_table() {
    let root = unique_temp_dir("period-conclusion-today");
    write_today_claude(&root, 100_000, 20_000);

    let (ok, stdout, stderr) = run_ccstats(
        &["today", "--source", "claude", "-O", "--timezone", "UTC"],
        &[("HOME", &root)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    let output = String::from_utf8(stdout).expect("utf8 stdout");

    let conclusion = first_content_line(&output);
    let title_at = output.find("Token Usage").expect("table title");
    let conclusion_at = output.find(conclusion).expect("conclusion");
    assert!(
        conclusion_at < title_at,
        "conclusion must precede the table:\n{output}"
    );
    assert!(conclusion.starts_with("Today:"), "conclusion: {conclusion}");
    assert!(conclusion.contains("tokens"), "conclusion: {conclusion}");

    let conclusion_cost = cost_in(conclusion).expect("cost in conclusion");
    let table_cost = footer_cost(&output).expect("cost in TOTAL row");
    assert_eq!(
        conclusion_cost, table_cost,
        "conclusion cost {conclusion_cost} must match TOTAL {table_cost}\n{output}"
    );
    assert!(
        output.contains("usage records"),
        "trailing summary must remain:\n{output}"
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn today_json_stdout_is_pure_structured_data() {
    let root = unique_temp_dir("period-conclusion-today-json");
    write_today_claude(&root, 100_000, 20_000);

    let (ok, stdout, stderr) = run_ccstats(
        &[
            "today",
            "--source",
            "claude",
            "-j",
            "-O",
            "--timezone",
            "UTC",
        ],
        &[("HOME", &root)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    let output = String::from_utf8(stdout).expect("utf8 stdout");
    let trimmed = output.trim_start();
    assert!(
        trimmed.starts_with('[') || trimmed.starts_with('{'),
        "json must start with structured data: {output}"
    );
    assert!(
        !output.contains("Today:"),
        "json must not contain conclusion prose: {output}"
    );
    assert!(
        !output.contains("This period:"),
        "json must not contain conclusion prose: {output}"
    );
    assert!(
        !output.contains("(floor)"),
        "json must not contain conclusion prose: {output}"
    );
    serde_json::from_str::<serde_json::Value>(&output).expect("valid json");

    let _ = fs::remove_dir_all(root);
}

#[test]
fn compact_today_still_prints_conclusion() {
    let root = unique_temp_dir("period-conclusion-compact");
    write_today_claude(&root, 100_000, 20_000);

    let (ok, stdout, stderr) = run_ccstats(
        &[
            "today",
            "--source",
            "claude",
            "--compact",
            "-O",
            "--timezone",
            "UTC",
        ],
        &[("HOME", &root)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    let output = String::from_utf8(stdout).expect("utf8 stdout");
    let conclusion = first_content_line(&output);
    assert!(
        conclusion.starts_with("Today:"),
        "compact conclusion: {conclusion}\n{output}"
    );
    assert!(
        conclusion.contains("tokens"),
        "compact conclusion: {conclusion}"
    );
    assert!(
        !conclusion.contains("mean"),
        "compact omits pace: {conclusion}"
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn no_cost_today_omits_dollars() {
    let root = unique_temp_dir("period-conclusion-no-cost");
    write_today_claude(&root, 100_000, 20_000);

    let (ok, stdout, stderr) = run_ccstats(
        &[
            "today",
            "--source",
            "claude",
            "--no-cost",
            "-O",
            "--timezone",
            "UTC",
        ],
        &[("HOME", &root)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    let output = String::from_utf8(stdout).expect("utf8 stdout");
    let conclusion = first_content_line(&output);
    assert!(
        conclusion.starts_with("Today:"),
        "conclusion: {conclusion}\n{output}"
    );
    assert!(
        !conclusion.contains('$'),
        "no-cost conclusion must not invent dollars: {conclusion}"
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn daily_table_does_not_say_today() {
    let root = unique_temp_dir("period-conclusion-daily");
    let path = root.join(".claude/projects/myproject/session-a.jsonl");
    write_file(
        &path,
        r#"{"timestamp":"2026-02-06T12:00:00Z","message":{"id":"msg_1","model":"claude-3-5-sonnet-20241022","stop_reason":"end_turn","usage":{"input_tokens":100000,"output_tokens":20000}}}
"#,
    );

    let (ok, stdout, stderr) = run_ccstats(
        &[
            "daily",
            "--source",
            "claude",
            "-O",
            "--timezone",
            "UTC",
            "--since",
            "2026-02-06",
            "--until",
            "2026-02-06",
        ],
        &[("HOME", &root)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    let output = String::from_utf8(stdout).expect("utf8 stdout");
    let conclusion = first_content_line(&output);
    assert!(
        conclusion.starts_with("This period:"),
        "daily conclusion: {conclusion}\n{output}"
    );
    assert!(!conclusion.contains("Today"));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn unpriced_table_does_not_headline_zero_dollars() {
    let root = unique_temp_dir("period-conclusion-unpriced");
    let today = Utc::now().format("%Y-%m-%dT12:00:00Z");
    let path = root.join(".claude/projects/myproject/session-a.jsonl");
    write_file(
        &path,
        &format!(
            r#"{{"timestamp":"{today}","message":{{"id":"msg_1","model":"mystery-model","stop_reason":"end_turn","usage":{{"input_tokens":100,"output_tokens":50}}}}}}
"#
        ),
    );

    let (ok, stdout, stderr) = run_ccstats(
        &[
            "today",
            "--source",
            "claude",
            "-O",
            "--strict-pricing",
            "--timezone",
            "UTC",
        ],
        &[("HOME", &root)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    let output = String::from_utf8(stdout).expect("utf8 stdout");
    let conclusion = first_content_line(&output);
    assert!(
        conclusion.starts_with("Today:"),
        "conclusion: {conclusion}\n{output}"
    );
    assert!(
        !conclusion.contains("$0.00"),
        "unpriced fixture must not headline $0.00: {conclusion}\n{output}"
    );

    let _ = fs::remove_dir_all(root);
}
