mod common;

use base64::Engine;
use base64::prelude::BASE64_STANDARD;
use common::{run_ccstats, unique_temp_dir, write_file};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;

#[cfg(unix)]
fn make_private(path: &Path) {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).expect("set private fixture mode");
}

#[cfg(not(unix))]
fn make_private(_: &Path) {}

#[cfg(unix)]
fn make_private_dir(path: &Path) {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .expect("set private fixture directory mode");
}

#[cfg(not(unix))]
fn make_private_dir(_: &Path) {}

fn make_fx_recovery_dirs(root: &Path, session_id: &str) {
    for path in [
        root.join(".fx"),
        root.join(".fx/sessions"),
        root.join(format!(".fx/sessions/{session_id}")),
        root.join(".fx/usage-recovery"),
    ] {
        if path.exists() {
            make_private_dir(&path);
        }
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn daily_json(source: &str, envs: &[(&str, &Path)], with_cost: bool) -> Value {
    let mut args = vec![
        "daily",
        "--source",
        source,
        "--json",
        "--offline",
        "--timezone",
        "UTC",
        "--since",
        "2026-08-31",
        "--until",
        "2026-08-31",
    ];
    if !with_cost {
        args.push("--no-cost");
    }
    let (ok, stdout, stderr) = run_ccstats(&args, envs);
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    serde_json::from_slice(&stdout).expect("daily JSON")
}

fn top_models_json(source: &str, envs: &[(&str, &Path)]) -> Value {
    let (ok, stdout, stderr) = run_ccstats(
        &[
            "top",
            "--source",
            source,
            "--dim",
            "model",
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
    serde_json::from_slice(&stdout).expect("top models JSON")
}

fn write_fx_canonical_session(root: &Path, session_id: &str, durable_usage: &str) {
    const GENERATION: &str = "000102030405060708090a0b0c0d0e0f";
    const EVENT_ID: &str = "101112131415161718191a1b1c1d1e1f";
    let session = root.join(format!(".fx/sessions/{session_id}"));
    write_file(
        &session.join("authority.json"),
        &format!(
            r#"{{"schema_version":1,"session_id":"{session_id}","authority_id":"202122232425262728292a2b2c2d2e2f","storage_format":"event_log_v1","source":"native_create"}}"#,
        ),
    );
    make_private(&session.join("authority.json"));
    let event = format!(
        "{{\"schema_version\":1,\"log_generation\":\"{GENERATION}\",\"seq\":1,\"event_id\":\"{EVENT_ID}\",\"timestamp_ms\":1788145200000,\"kind\":\"session_started\",\"payload\":{{\"id\":\"{session_id}\",\"created_at_ms\":1788145200000,\"origin_workspace_root\":\"/tmp/fx\",\"workspace_root\":\"/tmp/fx\",\"conversation_language\":\"en\",\"preferences\":{{\"model\":\"openai/gpt-5\",\"effort\":\"high\",\"fast_mode\":false,\"provider\":\"gateway\"}},\"usage\":{durable_usage}}}}}\n"
    );
    write_file(&session.join("events.jsonl"), &event);
    make_private(&session.join("events.jsonl"));
    write_file(
        &session.join(format!("commit.{GENERATION}.json")),
        &format!(
            "{{\"schema_version\":1,\"session_id\":\"{session_id}\",\"log_generation\":\"{GENERATION}\",\"through_seq\":1,\"through_event_id\":\"{EVENT_ID}\",\"through_event_log_bytes\":{}}}\n",
            event.len()
        ),
    );
    make_private(&session.join(format!("commit.{GENERATION}.json")));
    make_fx_recovery_dirs(root, session_id);
}

#[test]
fn reasonix_reads_authoritative_stats_and_preserves_usd_valuation() {
    let root = unique_temp_dir("reasonix-e2e");
    let state = root.join("state");
    let other_home = root.join("other-home");
    let stats = state.join("stats");
    write_file(
        &stats.join("2026-08-31.jsonl"),
        concat!(
            r#"{"ts":"2026-08-31T03:00:00Z","model":"deepseek/deepseek-v4-pro","source":"desktop","prompt":100,"completion":25,"reasoning":5,"cache_hit":30,"cache_miss":70,"total":125,"requests":2,"cost_complete":true,"valuation_usd":"0.01725"}"#,
            "\n",
            r#"{"ts":"2026-08-31T03:01:00Z","turn":true}"#,
            "\nnot-json\n",
            r#"{"ts":"2026-08-31T03:02:00Z","model":"anthropic/claude-sonnet-4","source":"subagent","prompt":40,"completion":10,"reasoning":2,"cache_hit":40,"total":50,"cost_complete":true,"cost_currency":"USD","cost_amount":"0"}"#,
            "\n",
            r#"{"ts":"2026-08-31T03:03:00Z","model":"openai/gpt-5","source":"compaction","total":0,"requests":3,"cost_complete":true,"cost_currency":"USD","cost_amount":"0"}"#,
            "\n",
        ),
    );
    write_file(
        &stats.join("notes.jsonl"),
        r#"{"ts":"2026-08-31T03:04:00Z","model":"fake/model","prompt":999,"total":999}"#,
    );
    write_file(
        &other_home.join("stats/2026-08-31.jsonl"),
        r#"{"ts":"2026-08-31T03:05:00Z","model":"fake/model","prompt":888,"total":888}"#,
    );
    write_file(
        &state.join("sessions/copied.jsonl"),
        r#"{"ts":"2026-08-31T03:06:00Z","model":"fake/model","prompt":777,"total":777}"#,
    );

    let json = daily_json(
        "reasonix",
        &[
            ("REASONIX_STATE_HOME", &state),
            ("REASONIX_HOME", &other_home),
            ("HOME", &root),
        ],
        true,
    );
    let row = &json.as_array().expect("array")[0];
    assert_eq!(row["input_tokens"].as_i64(), Some(70));
    assert_eq!(row["output_tokens"].as_i64(), Some(28));
    assert_eq!(row["reasoning_tokens"].as_i64(), Some(7));
    assert_eq!(row["cache_read_tokens"].as_i64(), Some(70));
    assert_eq!(row["total_tokens"].as_i64(), Some(175));
    assert_eq!(row["cost"].as_f64(), Some(0.01725));
    assert_eq!(row["data_quality"]["valid_entries"].as_i64(), Some(3));
    assert_eq!(row["data_quality"]["parse_errors"].as_u64(), Some(1));
    let top = top_models_json(
        "reasonix",
        &[
            ("REASONIX_STATE_HOME", &state),
            ("REASONIX_HOME", &other_home),
            ("HOME", &root),
        ],
    );
    let entries = top["entries"].as_array().expect("top entries");
    assert!(entries.iter().any(|entry| {
        entry["name"] == "deepseek/deepseek-v4-pro" && entry["count"].as_i64() == Some(2)
    }));
    assert!(
        entries
            .iter()
            .any(|entry| { entry["name"] == "openai/gpt-5" && entry["count"].as_i64() == Some(3) })
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn reasonix_rejects_corrupt_buckets_but_keeps_later_rows() {
    let root = unique_temp_dir("reasonix-errors");
    write_file(
        &root.join("stats/2026-08-31.jsonl"),
        concat!(
            r#"{"ts":"2026-08-31T03:00:00Z","model":"bad/negative","prompt":-1,"total":1}"#,
            "\n",
            r#"{"ts":"2026-08-31T03:01:00Z","model":"bad/reasoning","prompt":1,"completion":2,"reasoning":3,"total":3}"#,
            "\n",
            r#"{"ts":"2026-08-31T03:01:30Z","model":"bad/incomplete","total":7}"#,
            "\n",
            r#"{"ts":"2026-08-31T03:02:00Z","model":"good/model","prompt":5,"completion":2,"cache_miss":5,"total":7,"requests":1}"#,
            "\n",
            r#"{"ts":"2026-08-31T03:03:00Z","model":"good/provider-total","prompt":100,"completion":20,"cache_hit":30,"cache_miss":10,"total":120,"requests":2}"#,
            "\n",
        ),
    );

    let json = daily_json(
        "reasonix",
        &[("REASONIX_STATE_HOME", &root), ("HOME", &root)],
        false,
    );
    let row = &json.as_array().expect("array")[0];
    assert_eq!(row["total_tokens"].as_i64(), Some(67));
    assert_eq!(row["data_quality"]["valid_entries"].as_i64(), Some(2));
    assert_eq!(row["data_quality"]["parse_errors"].as_u64(), Some(3));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn fx_splits_parent_token_totals_and_uses_model_costs() {
    let root = unique_temp_dir("fx-e2e");
    let ledger = root.join(".fx/usage.jsonl");
    write_file(
        &ledger,
        concat!(
            r#"{"schema_version":1,"kind":"coverage","started_at_ms":1788130000000}"#,
            "\n",
            r#"{"schema_version":1,"kind":"generation","fact":{"id":"gen_01ARZ3NDEKTSV4RRFFQ69G5FAV","created_at_ms":1788145200000,"model":"openai/gpt-5","input_tokens":130,"output_tokens":25,"cache_read_tokens":20,"cache_write_tokens":10,"reasoning_tokens":5,"billable_web_search_calls":2,"total_cost":0.03}}"#,
            "\n",
            r#"{"schema_version":1,"kind":"generation","fact":{"id":"gen_01ARZ3NDEKTSV4RRFFQ69G5FAV","created_at_ms":1788145200000,"model":"openai/gpt-5","input_tokens":130,"output_tokens":25,"cache_read_tokens":20,"cache_write_tokens":10,"reasoning_tokens":5,"billable_web_search_calls":2,"total_cost":0.03}}"#,
            "\n",
        ),
    );
    // Recovery/session snapshots are cumulative copies and must never be added
    // to the profile generation ledger.
    write_file(
        &root.join(".fx/sessions/session-one/usage-v2.json"),
        r#"{"schema_version":1,"session_id":"session-one","snapshot":{"schema_version":3,"total_cost":9,"input_tokens":999,"output_tokens":999,"models":[]}}"#,
    );

    let json = daily_json("fx", &[("HOME", &root)], true);
    let row = &json.as_array().expect("array")[0];
    assert_eq!(row["input_tokens"].as_i64(), Some(100));
    assert_eq!(row["output_tokens"].as_i64(), Some(20));
    assert_eq!(row["reasoning_tokens"].as_i64(), Some(5));
    assert_eq!(row["cache_read_tokens"].as_i64(), Some(20));
    assert_eq!(row["cache_creation_tokens"].as_i64(), Some(10));
    assert_eq!(row["total_tokens"].as_i64(), Some(155));
    assert_eq!(row["cost"].as_f64(), Some(0.03));
    assert_eq!(row["data_quality"]["valid_entries"].as_i64(), Some(1));
    assert_eq!(row["data_quality"]["parse_errors"].as_u64(), Some(0));
    let top = top_models_json("fx", &[("HOME", &root)]);
    assert_eq!(top["entries"][0]["name"], "openai/gpt-5");
    assert_eq!(top["entries"][0]["count"].as_i64(), Some(1));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn fx_reports_conflicting_generation_and_invalid_buckets() {
    let root = unique_temp_dir("fx-errors");
    write_file(
        &root.join(".fx/usage.jsonl"),
        concat!(
            r#"{"schema_version":1,"kind":"coverage","started_at_ms":1788130000000}"#,
            "\n",
            r#"{"schema_version":1,"kind":"generation","fact":{"id":"gen_01ARZ3NDEKTSV4RRFFQ69G5FAV","created_at_ms":1788145200000,"model":"openai/gpt-5","input_tokens":10,"output_tokens":2,"cache_read_tokens":0,"cache_write_tokens":0,"reasoning_tokens":0,"billable_web_search_calls":0,"total_cost":0}}"#,
            "\n",
            r#"{"schema_version":1,"kind":"generation","fact":{"id":"gen_01ARZ3NDEKTSV4RRFFQ69G5FAV","created_at_ms":1788145200000,"model":"openai/gpt-5","input_tokens":11,"output_tokens":2,"cache_read_tokens":0,"cache_write_tokens":0,"reasoning_tokens":0,"billable_web_search_calls":0,"total_cost":0}}"#,
            "\n",
            r#"{"schema_version":1,"kind":"generation","fact":{"id":"gen_01ARZ3NDEKTSV4RRFFQ69G5FAW","created_at_ms":1788145260000,"model":"anthropic/claude-sonnet-4","input_tokens":5,"output_tokens":2,"cache_read_tokens":4,"cache_write_tokens":3,"reasoning_tokens":0,"billable_web_search_calls":0,"total_cost":0.01}}"#,
            "\n",
        ),
    );

    let json = daily_json("fx", &[("HOME", &root)], false);
    let row = &json.as_array().expect("array")[0];
    assert_eq!(row["total_tokens"].as_i64(), Some(12));
    assert_eq!(row["data_quality"]["valid_entries"].as_i64(), Some(1));
    assert_eq!(row["data_quality"]["parse_errors"].as_u64(), Some(2));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn fx_preserves_subscription_zero_and_resolves_pending_generation() {
    let root = unique_temp_dir("fx-zero-cost");
    write_file(
        &root.join(".fx/usage.jsonl"),
        concat!(
            r#"{"schema_version":1,"kind":"coverage","started_at_ms":1788130000000}"#,
            "\n",
            r#"{"schema_version":1,"kind":"pending","id":"gen_01ARZ3NDEKTSV4RRFFQ69G5FAV","observed_at_ms":1788145199000}"#,
            "\n",
            r#"{"schema_version":1,"kind":"generation","fact":{"id":"gen_01ARZ3NDEKTSV4RRFFQ69G5FAV","created_at_ms":1788145200000,"model":"custom/subscription-model","input_tokens":10,"output_tokens":2,"cache_read_tokens":0,"cache_write_tokens":0,"reasoning_tokens":null,"billable_web_search_calls":0,"total_cost":0}}"#,
            "\n",
        ),
    );

    let json = daily_json("fx", &[("HOME", &root)], true);
    let row = &json.as_array().expect("array")[0];
    assert_eq!(row["total_tokens"].as_i64(), Some(12));
    assert_eq!(row["cost"].as_f64(), Some(0.0));
    assert_eq!(row["data_quality"]["parse_errors"].as_u64(), Some(0));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn fx_merges_only_marker_bounded_publication_backlog() {
    let root = unique_temp_dir("fx-recovery");
    write_file(
        &root.join(".fx/usage.jsonl"),
        concat!(
            r#"{"schema_version":1,"kind":"coverage","started_at_ms":1788130000000}"#,
            "\n",
            r#"{"schema_version":1,"kind":"generation","fact":{"id":"gen_01ARZ3NDEKTSV4RRFFQ69G5FAV","created_at_ms":1788145200000,"model":"anthropic/claude-sonnet-4","input_tokens":20,"output_tokens":4,"cache_read_tokens":5,"cache_write_tokens":0,"reasoning_tokens":1,"billable_web_search_calls":0,"total_cost":0.04}}"#,
            "\n",
        ),
    );
    write_file(
        &root.join(".fx/usage-recovery/recovered-session"),
        "v1 1788145200000\n",
    );
    make_private(&root.join(".fx/usage-recovery/recovered-session"));
    make_fx_recovery_dirs(&root, "recovered-session");
    write_file(
        &root.join(".fx/usage-recovery/z-missing-session"),
        "v1 1788145200000\n",
    );
    make_private(&root.join(".fx/usage-recovery/z-missing-session"));
    write_fx_canonical_session(
        &root,
        "recovered-session",
        r#"{"billing":"complete","api_duration_complete":true,"wall_duration_complete":true,"code_complete":true,"next_sequence":3,"settled_through_sequence":2,"api_duration_ms":1,"wall_duration_ms":1,"total_cost":0.04,"input_tokens":30,"output_tokens":6,"cache_read_tokens":5,"cache_write_tokens":0,"billable_web_search_calls":0,"lines_added":0,"lines_removed":0,"models":[{"model":"anthropic/claude-sonnet-4","first_sequence":1,"total_cost":0.04,"input_tokens":20,"output_tokens":4,"cache_read_tokens":5,"cache_write_tokens":0,"billable_web_search_calls":0},{"model":"openai/gpt-5","first_sequence":2,"total_cost":0.0,"input_tokens":10,"output_tokens":2,"cache_read_tokens":0,"cache_write_tokens":0,"billable_web_search_calls":0}],"pending":[]}"#,
    );
    write_file(
        &root.join(".fx/sessions/recovered-session/usage-v2.json"),
        r#"{"schema_version":1,"session_id":"recovered-session","snapshot":{"schema_version":3,"billing":"complete","api_duration_complete":true,"wall_duration_complete":true,"code_complete":true,"next_sequence":3,"settled_through_sequence":2,"api_duration_ms":1,"wall_duration_ms":1,"total_cost":0.04,"input_tokens":30,"output_tokens":6,"cache_read_tokens":5,"cache_write_tokens":0,"reasoning_tokens":1,"request_count":2,"billable_web_search_calls":0,"lines_added":0,"lines_removed":0,"models":[{"model":"anthropic/claude-sonnet-4","first_sequence":1,"total_cost":0.04,"input_tokens":20,"output_tokens":4,"cache_read_tokens":5,"cache_write_tokens":0,"reasoning_tokens":1,"request_count":1,"billable_web_search_calls":0},{"model":"openai/gpt-5","first_sequence":2,"total_cost":0.0,"input_tokens":10,"output_tokens":2,"cache_read_tokens":0,"cache_write_tokens":0,"reasoning_tokens":0,"request_count":1,"billable_web_search_calls":0}],"pending":[],"publication_backlog":[{"id":"gen_01ARZ3NDEKTSV4RRFFQ69G5FAV","created_at_ms":1788145200000,"model":"anthropic/claude-sonnet-4","input_tokens":20,"output_tokens":4,"cache_read_tokens":5,"cache_write_tokens":0,"reasoning_tokens":1,"billable_web_search_calls":0,"total_cost":0.04},{"id":"gen_01ARZ3NDEKTSV4RRFFQ69G5FAW","created_at_ms":1788145260000,"model":"openai/gpt-5","input_tokens":10,"output_tokens":2,"cache_read_tokens":0,"cache_write_tokens":0,"reasoning_tokens":0,"billable_web_search_calls":0,"total_cost":0}],"incidents":[]}}"#,
    );
    make_private(&root.join(".fx/sessions/recovered-session/usage-v2.json"));
    write_file(
        &root.join(".fx/sessions/unmarked-session/usage-v2.json"),
        r#"{"schema_version":1,"session_id":"unmarked-session","snapshot":{"publication_backlog":[{"id":"gen_01ARZ3NDEKTSV4RRFFQ69G5FAW","created_at_ms":1788145200000,"model":"fake/model","input_tokens":999,"output_tokens":0,"cache_read_tokens":0,"cache_write_tokens":0,"reasoning_tokens":0,"total_cost":9}]}}"#,
    );

    let json = daily_json("fx", &[("HOME", &root)], true);
    let row = &json.as_array().expect("array")[0];
    assert_eq!(row["input_tokens"].as_i64(), Some(25));
    assert_eq!(row["output_tokens"].as_i64(), Some(5));
    assert_eq!(row["reasoning_tokens"].as_i64(), Some(1));
    assert_eq!(row["cache_read_tokens"].as_i64(), Some(5));
    assert_eq!(row["total_tokens"].as_i64(), Some(36));
    assert_eq!(row["cost"].as_f64(), Some(0.04));
    assert_eq!(row["data_quality"]["valid_entries"].as_i64(), Some(2));
    assert_eq!(row["data_quality"]["parse_errors"].as_u64(), Some(1));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn fx_rejects_marker_and_sidecar_without_canonical_session() {
    let root = unique_temp_dir("fx-sidecar-only");
    write_file(
        &root.join(".fx/usage.jsonl"),
        concat!(
            r#"{"schema_version":1,"kind":"coverage","started_at_ms":1788130000000}"#,
            "\n",
            r#"{"schema_version":1,"kind":"generation","fact":{"id":"gen_01ARZ3NDEKTSV4RRFFQ69G5FAV","created_at_ms":1788145200000,"model":"openai/gpt-5","input_tokens":10,"output_tokens":2,"cache_read_tokens":0,"cache_write_tokens":0,"reasoning_tokens":0,"billable_web_search_calls":0,"total_cost":0}}"#,
            "\n",
        ),
    );
    write_file(
        &root.join(".fx/usage-recovery/sidecar-only"),
        "v1 1788145200000\n",
    );
    make_private(&root.join(".fx/usage-recovery/sidecar-only"));
    write_file(
        &root.join(".fx/sessions/sidecar-only/usage-v2.json"),
        r#"{"schema_version":1,"session_id":"sidecar-only","snapshot":{"schema_version":3,"billing":"complete","api_duration_complete":true,"wall_duration_complete":true,"code_complete":true,"next_sequence":2,"settled_through_sequence":1,"api_duration_ms":0,"wall_duration_ms":0,"total_cost":9,"input_tokens":999,"output_tokens":0,"cache_read_tokens":0,"cache_write_tokens":0,"reasoning_tokens":0,"request_count":1,"billable_web_search_calls":0,"lines_added":0,"lines_removed":0,"models":[{"model":"fake/model","first_sequence":1,"total_cost":9,"input_tokens":999,"output_tokens":0,"cache_read_tokens":0,"cache_write_tokens":0,"reasoning_tokens":0,"request_count":1,"billable_web_search_calls":0}],"pending":[],"publication_backlog":[{"id":"gen_01ARZ3NDEKTSV4RRFFQ69G5FAW","created_at_ms":1788145260000,"model":"fake/model","input_tokens":999,"output_tokens":0,"cache_read_tokens":0,"cache_write_tokens":0,"reasoning_tokens":0,"billable_web_search_calls":0,"total_cost":9}],"incidents":[]}}"#,
    );
    make_private(&root.join(".fx/sessions/sidecar-only/usage-v2.json"));
    make_fx_recovery_dirs(&root, "sidecar-only");

    let json = daily_json("fx", &[("HOME", &root)], true);
    let row = &json.as_array().expect("array")[0];
    assert_eq!(row["total_tokens"].as_i64(), Some(12));
    assert_eq!(row["cost"].as_f64(), Some(0.0));
    assert_eq!(row["data_quality"]["valid_entries"].as_i64(), Some(1));
    assert_eq!(row["data_quality"]["parse_errors"].as_u64(), Some(1));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn fx_replays_committed_state_replacement_before_recovery() {
    const SESSION_ID: &str = "replaced-session";
    const GENERATION: &str = "000102030405060708090a0b0c0d0e0f";
    const REPLACEMENT_ID: &str = "303132333435363738393a3b3c3d3e3f";
    const COMMIT_ID: &str = "404142434445464748494a4b4c4d4e4f";
    let root = unique_temp_dir("fx-replaced-session");
    let initial_usage = r#"{"billing":"complete","api_duration_complete":true,"wall_duration_complete":true,"code_complete":true,"next_sequence":1,"settled_through_sequence":0,"api_duration_ms":0,"wall_duration_ms":0,"total_cost":0,"input_tokens":0,"output_tokens":0,"cache_read_tokens":0,"cache_write_tokens":0,"billable_web_search_calls":0,"lines_added":0,"lines_removed":0,"models":[],"pending":[]}"#;
    let replacement_usage = r#"{"billing":"complete","api_duration_complete":true,"wall_duration_complete":true,"code_complete":true,"next_sequence":2,"settled_through_sequence":1,"api_duration_ms":1,"wall_duration_ms":1,"total_cost":0.01,"input_tokens":10,"output_tokens":2,"cache_read_tokens":2,"cache_write_tokens":0,"billable_web_search_calls":0,"lines_added":0,"lines_removed":0,"models":[{"model":"openai/gpt-5","first_sequence":1,"total_cost":0.01,"input_tokens":10,"output_tokens":2,"cache_read_tokens":2,"cache_write_tokens":0,"billable_web_search_calls":0}],"pending":[]}"#;
    write_fx_canonical_session(&root, SESSION_ID, initial_usage);
    let state = format!(
        "{{\"id\":\"{SESSION_ID}\",\"origin_workspace_root\":\"/tmp/fx\",\"workspace_root\":\"/tmp/fx\",\"created_at_ms\":1788145200000,\"updated_at_ms\":1788145260000,\"conversation_language\":\"en\",\"preferences\":{{\"model\":\"openai/gpt-5\",\"effort\":\"high\",\"fast_mode\":false,\"provider\":\"gateway\"}},\"history\":[],\"total_input_tokens\":10,\"total_output_tokens\":2,\"context_history_start\":0,\"permission_state\":{{\"schema_version\":2,\"next_generation\":1,\"rules\":[]}},\"usage\":{replacement_usage}}}"
    );
    let digest = sha256_hex(state.as_bytes());
    let encoded = BASE64_STANDARD.encode(state.as_bytes());
    let first = format!(
        "{{\"schema_version\":1,\"log_generation\":\"{GENERATION}\",\"seq\":1,\"event_id\":\"101112131415161718191a1b1c1d1e1f\",\"timestamp_ms\":1788145200000,\"kind\":\"session_started\",\"payload\":{{\"id\":\"{SESSION_ID}\",\"created_at_ms\":1788145200000,\"origin_workspace_root\":\"/tmp/fx\",\"workspace_root\":\"/tmp/fx\",\"conversation_language\":\"en\",\"preferences\":{{\"model\":\"openai/gpt-5\",\"effort\":\"high\",\"fast_mode\":false,\"provider\":\"gateway\"}},\"usage\":{initial_usage}}}}}\n"
    );
    let events = format!(
        "{first}{{\"schema_version\":1,\"log_generation\":\"{GENERATION}\",\"seq\":2,\"event_id\":\"202122232425262728292a2b2c2d2e2f\",\"timestamp_ms\":1788145260000,\"kind\":\"state_replacement_started\",\"payload\":{{\"replacement_id\":\"{REPLACEMENT_ID}\",\"reason\":\"compaction\",\"encoded_bytes\":{},\"sha256\":\"{digest}\",\"chunk_count\":1}}}}\n{{\"schema_version\":1,\"log_generation\":\"{GENERATION}\",\"seq\":3,\"event_id\":\"303132333435363738393a3b3c3d3e30\",\"timestamp_ms\":1788145260000,\"kind\":\"state_replacement_chunk\",\"payload\":{{\"replacement_id\":\"{REPLACEMENT_ID}\",\"chunk_index\":0,\"raw_bytes\":{},\"chunk_sha256\":\"{digest}\",\"base64\":\"{encoded}\"}}}}\n{{\"schema_version\":1,\"log_generation\":\"{GENERATION}\",\"seq\":4,\"event_id\":\"{COMMIT_ID}\",\"timestamp_ms\":1788145260000,\"kind\":\"state_replacement_committed\",\"payload\":{{\"replacement_id\":\"{REPLACEMENT_ID}\",\"encoded_bytes\":{},\"sha256\":\"{digest}\",\"chunk_count\":1}}}}\n",
        state.len(),
        state.len(),
        state.len(),
    );
    let session = root.join(format!(".fx/sessions/{SESSION_ID}"));
    write_file(&session.join("events.jsonl"), &events);
    make_private(&session.join("events.jsonl"));
    write_file(
        &session.join(format!("commit.{GENERATION}.json")),
        &format!(
            "{{\"schema_version\":1,\"session_id\":\"{SESSION_ID}\",\"log_generation\":\"{GENERATION}\",\"through_seq\":4,\"through_event_id\":\"{COMMIT_ID}\",\"through_event_log_bytes\":{}}}\n",
            events.len()
        ),
    );
    make_private(&session.join(format!("commit.{GENERATION}.json")));
    write_file(
        &root.join(format!(".fx/usage-recovery/{SESSION_ID}")),
        "v1 1788145260000\n",
    );
    make_private(&root.join(format!(".fx/usage-recovery/{SESSION_ID}")));
    make_fx_recovery_dirs(&root, SESSION_ID);
    write_file(
        &session.join("usage-v2.json"),
        r#"{"schema_version":1,"session_id":"replaced-session","snapshot":{"schema_version":3,"billing":"complete","api_duration_complete":true,"wall_duration_complete":true,"code_complete":true,"next_sequence":2,"settled_through_sequence":1,"api_duration_ms":1,"wall_duration_ms":1,"total_cost":0.01,"input_tokens":10,"output_tokens":2,"cache_read_tokens":2,"cache_write_tokens":0,"reasoning_tokens":1,"request_count":1,"billable_web_search_calls":0,"lines_added":0,"lines_removed":0,"models":[{"model":"openai/gpt-5","first_sequence":1,"total_cost":0.01,"input_tokens":10,"output_tokens":2,"cache_read_tokens":2,"cache_write_tokens":0,"reasoning_tokens":1,"request_count":1,"billable_web_search_calls":0}],"pending":[],"publication_backlog":[{"id":"gen_01ARZ3NDEKTSV4RRFFQ69G5FAV","created_at_ms":1788145260000,"model":"openai/gpt-5","input_tokens":10,"output_tokens":2,"cache_read_tokens":2,"cache_write_tokens":0,"reasoning_tokens":1,"billable_web_search_calls":0,"total_cost":0.01}],"incidents":[]}}"#,
    );
    make_private(&session.join("usage-v2.json"));

    let json = daily_json("fx", &[("HOME", &root)], true);
    let row = &json.as_array().expect("array")[0];
    assert_eq!(row["total_tokens"].as_i64(), Some(12));
    assert_eq!(row["reasoning_tokens"].as_i64(), Some(1));
    assert_eq!(row["cache_read_tokens"].as_i64(), Some(2));
    assert_eq!(row["cost"].as_f64(), Some(0.01));
    assert_eq!(row["data_quality"]["valid_entries"].as_i64(), Some(1));
    assert_eq!(row["data_quality"]["parse_errors"].as_u64(), Some(0));
    let _ = fs::remove_dir_all(root);
}
