mod common;

use common::{run_ccstats, unique_temp_dir, write_file};
use serde_json::Value;
use std::fs;
use std::path::Path;

fn write_session(path: &Path, version: u32, id: &str, input: i64) {
    write_file(
        path,
        &format!(
            "{}\n{}\n",
            format_args!(
                r#"{{"type":"session","version":{version},"id":"{id}","timestamp":"2026-08-31T08:00:00Z","cwd":"/tmp/{id}"}}"#
            ),
            format_args!(
                r#"{{"type":"message","id":"call-{id}","timestamp":"2026-08-31T08:00:01Z","message":{{"role":"assistant","provider":"openai","model":"gpt-5","timestamp":1788163201000,"usage":{{"input":{input},"output":1,"cacheRead":0,"cacheWrite":0,"totalTokens":{},"cost":{{"total":0}}}},"stopReason":"stop"}}}}"#,
                input + 1
            )
        ),
    );
}

fn daily_input(source: &str, envs: &[(&str, &Path)]) -> i64 {
    let (ok, stdout, stderr) = run_ccstats(
        &[
            "daily",
            "--source",
            source,
            "--json",
            "--offline",
            "--no-cost",
            "--timezone",
            "UTC",
            "--since",
            "2026-08-31",
            "--until",
            "2026-08-31",
        ],
        envs,
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    let rows: Value = serde_json::from_slice(&stdout).unwrap_or_else(|error| {
        panic!(
            "daily JSON for {envs:?}: {error}; stdout: {}; stderr: {}",
            String::from_utf8_lossy(&stdout),
            String::from_utf8_lossy(&stderr)
        )
    });
    rows.as_array().expect("array")[0]["input_tokens"]
        .as_i64()
        .unwrap_or_else(|| panic!("input tokens for {envs:?}: {rows}"))
}

#[test]
fn prime_session_env_and_project_settings_follow_official_priority() {
    let root = unique_temp_dir("prime-paths");
    let agent = root.join("prime-agent");
    let cwd = root.join("project");
    let explicit = root.join("explicit-sessions");
    let global = root.join("global-sessions");
    let project = cwd.join("project-sessions");
    let default = agent.join("sessions");
    write_session(&explicit.join("session.jsonl"), 3, "explicit", 4);
    write_session(&global.join("session.jsonl"), 3, "global", 3);
    write_session(&project.join("session.jsonl"), 3, "project", 2);
    write_session(&default.join("session.jsonl"), 3, "default", 1);
    write_file(
        &agent.join("settings.json"),
        r#"{"sessionDir":"~/global-sessions"}"#,
    );
    write_file(
        &cwd.join(".prime/agent/settings.json"),
        r#"{"sessionDir":"project-sessions"}"#,
    );

    assert_eq!(
        daily_input(
            "prime",
            &[
                ("HOME", &root),
                ("CCSTATS_TEST_CWD", &cwd),
                ("PRIME_AGENT_CODING_AGENT_DIR", &agent),
                ("PRIME_AGENT_SESSION_DIR", &explicit),
            ],
        ),
        4
    );
    assert_eq!(
        daily_input(
            "prime",
            &[
                ("HOME", &root),
                ("CCSTATS_TEST_CWD", &cwd),
                ("PRIME_AGENT_CODING_AGENT_DIR", &agent),
            ],
        ),
        2
    );

    write_file(
        &cwd.join(".prime/agent/settings.json"),
        r#"{"sessionDir":123}"#,
    );
    let (ok, stdout, stderr) = run_ccstats(
        &[
            "daily",
            "--source",
            "prime",
            "--json",
            "--offline",
            "--no-cost",
            "--timezone",
            "UTC",
            "--since",
            "2026-08-31",
            "--until",
            "2026-08-31",
        ],
        &[
            ("HOME", &root),
            ("CCSTATS_TEST_CWD", &cwd),
            ("PRIME_AGENT_CODING_AGENT_DIR", &agent),
        ],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    let rows: Value = serde_json::from_slice(&stdout).expect("daily JSON");
    let row = &rows.as_array().expect("array")[0];
    assert_eq!(row["input_tokens"].as_i64(), Some(3));
    assert_eq!(row["data_quality"]["parse_errors"].as_u64(), Some(1));

    write_file(
        &cwd.join(".prime/agent/settings.json"),
        r#"{"sessionDir":null}"#,
    );
    assert_eq!(
        daily_input(
            "prime",
            &[
                ("HOME", &root),
                ("CCSTATS_TEST_CWD", &cwd),
                ("PRIME_AGENT_CODING_AGENT_DIR", &agent),
            ],
        ),
        1
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn gjc_uses_xdg_only_for_the_default_agent_directory() {
    let root = unique_temp_dir("gjc-paths");
    let xdg = root.join("xdg-data");
    let default_agent = root.join(".gjc/agent");
    let equivalent_default_agent = root.join(".gjc/ghost/../agent");
    let custom_agent = root.join("custom-agent");
    write_session(&xdg.join("gjc/sessions/session.jsonl"), 5, "xdg-gjc", 7);
    write_session(
        &default_agent.join("sessions/session.jsonl"),
        5,
        "home-gjc",
        6,
    );
    write_session(
        &custom_agent.join("sessions/session.jsonl"),
        5,
        "custom-gjc",
        8,
    );

    assert_eq!(
        daily_input(
            "gjc",
            &[
                ("HOME", &root),
                ("XDG_DATA_HOME", &xdg),
                ("GJC_CODING_AGENT_DIR", &default_agent),
            ],
        ),
        7
    );
    assert_eq!(
        daily_input(
            "gjc",
            &[
                ("HOME", &root),
                ("XDG_DATA_HOME", &xdg),
                ("GJC_CODING_AGENT_DIR", &equivalent_default_agent),
            ],
        ),
        7
    );
    assert_eq!(
        daily_input(
            "gjc",
            &[
                ("HOME", &root),
                ("XDG_DATA_HOME", &xdg),
                ("GJC_CODING_AGENT_DIR", &custom_agent),
            ],
        ),
        8
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn omp_session_env_profile_xdg_and_config_root_follow_official_priority() {
    let root = unique_temp_dir("omp-paths");
    let xdg = root.join("xdg-data");
    let direct = root.join("direct-sessions");
    let default = xdg.join("omp/sessions");
    let default_agent = root.join(".custom-omp/agent");
    let named_home = root.join(".custom-omp/profiles/home/agent/sessions");
    let named_xdg = xdg.join("omp/profiles/work/sessions");
    write_session(&direct.join("session.jsonl"), 3, "direct-omp", 11);
    write_session(&default.join("session.jsonl"), 3, "default-omp", 14);
    write_session(&named_home.join("session.jsonl"), 3, "home-profile-omp", 13);
    write_session(&named_xdg.join("session.jsonl"), 3, "xdg-profile-omp", 12);
    let config = Path::new(".custom-omp");

    assert_eq!(
        daily_input(
            "omp",
            &[
                ("HOME", &root),
                ("XDG_DATA_HOME", &xdg),
                ("PI_CONFIG_DIR", config),
                ("OMP_PROFILE", Path::new("work")),
                ("PI_CODING_AGENT_SESSION_DIR", &direct),
            ],
        ),
        11
    );
    assert_eq!(
        daily_input(
            "omp",
            &[
                ("HOME", &root),
                ("XDG_DATA_HOME", &xdg),
                ("PI_CONFIG_DIR", config),
                ("OMP_PROFILE", Path::new("work")),
            ],
        ),
        12
    );
    assert_eq!(
        daily_input(
            "omp",
            &[
                ("HOME", &root),
                ("XDG_DATA_HOME", &xdg),
                ("PI_CONFIG_DIR", config),
                ("OMP_PROFILE", Path::new("home")),
            ],
        ),
        13
    );
    assert_eq!(
        daily_input(
            "omp",
            &[
                ("HOME", &root),
                ("XDG_DATA_HOME", &xdg),
                ("PI_CONFIG_DIR", config),
                ("OMP_PROFILE", Path::new("")),
                ("PI_PROFILE", Path::new("work")),
            ],
        ),
        14
    );
    let inherited_profile_agent = root.join(".custom-omp/profiles/work/agent");
    assert_eq!(
        daily_input(
            "omp",
            &[
                ("HOME", &root),
                ("XDG_DATA_HOME", &xdg),
                ("PI_CONFIG_DIR", config),
                ("OMP_PROFILE", Path::new("")),
                ("PI_PROFILE", Path::new("work")),
                ("PI_CODING_AGENT_DIR", &inherited_profile_agent),
            ],
        ),
        14
    );
    assert_eq!(
        daily_input(
            "omp",
            &[
                ("HOME", &root),
                ("XDG_DATA_HOME", &xdg),
                ("PI_CONFIG_DIR", config),
                ("OMP_PROFILE", Path::new("")),
                ("PI_CODING_AGENT_DIR", &default_agent),
            ],
        ),
        14
    );

    let literal_tilde = root.join("~/literal-sessions");
    let expanded_tilde = root.join("literal-sessions");
    write_session(
        &literal_tilde.join("session.jsonl"),
        3,
        "literal-tilde-omp",
        15,
    );
    write_session(
        &expanded_tilde.join("session.jsonl"),
        3,
        "expanded-tilde-omp",
        16,
    );
    assert_eq!(
        daily_input(
            "omp",
            &[
                ("HOME", &root),
                ("CCSTATS_TEST_CWD", &root),
                (
                    "PI_CODING_AGENT_SESSION_DIR",
                    Path::new("~/literal-sessions"),
                ),
            ],
        ),
        15
    );

    let traversing_config = Path::new("nested/../omp-local");
    fs::create_dir_all(root.join("nested")).expect("create config parent");
    write_session(
        &root.join("omp-local/agent/sessions/session.jsonl"),
        3,
        "traversing-config-omp",
        17,
    );
    assert_eq!(
        daily_input(
            "omp",
            &[("HOME", &root), ("PI_CONFIG_DIR", traversing_config)],
        ),
        17
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn omp_invalid_profile_is_reported_as_a_data_quality_error() {
    let root = unique_temp_dir("omp-invalid-profile");
    let (ok, stdout, stderr) = run_ccstats(
        &[
            "daily",
            "--source",
            "omp",
            "--json",
            "--offline",
            "--no-cost",
            "--debug",
        ],
        &[("HOME", &root), ("OMP_PROFILE", Path::new("../escape"))],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    let rows: Value = serde_json::from_slice(&stdout).expect("daily JSON");
    assert_eq!(
        rows.as_array().expect("array")[0]["data_quality"]["parse_errors"].as_u64(),
        Some(1)
    );
    assert!(
        String::from_utf8_lossy(&stderr).contains("invalid-omp-profile.jsonl"),
        "stderr: {}",
        String::from_utf8_lossy(&stderr)
    );
    let _ = fs::remove_dir_all(root);
}
