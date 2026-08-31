mod common;

use common::{run_ccstats, unique_temp_dir, write_file};
use serde_json::{Value, json};
use std::fs;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

const DAY_START: i64 = 1_788_145_200_000;

fn header(id: &str, cwd: &str, parent: Option<&str>, seed_length: Option<i64>) -> Value {
    let mut value = json!({
        "type": "session",
        "version": 0,
        "id": id,
        "createdAt": DAY_START,
        "cwd": cwd,
        "delegationDepth": 0
    });
    if let Some(parent) = parent {
        value["parentSession"] = json!(parent);
    }
    if let Some(seed_length) = seed_length {
        value["seedLength"] = json!(seed_length);
    }
    value
}

fn assistant_at(
    seq: i64,
    step: i64,
    model: &str,
    response_model: Option<&str>,
    usage: Value,
) -> Value {
    let mut source = json!({"kind": "model", "provider": "fixture", "model": model});
    if let Some(response_model) = response_model {
        source["replayState"] = json!({
            "response": {
                "kind": "pi-ai",
                "version": 2,
                "api": "openai-responses",
                "provider": "fixture",
                "model": model,
                "responseModel": response_model,
                "stopReason": "stop"
            },
            "blocks": []
        });
    }
    json!({
        "type": "assistant/message",
        "seq": seq,
        "time": DAY_START + seq * 1_000,
        "data": {
            "turn": 1,
            "step": step,
            "message": {
                "role": "assistant",
                "content": [],
                "source": source,
                "id": format!("message-{seq}")
            },
            "usage": usage
        }
    })
}

fn assistant(seq: i64, model: &str, usage: Value) -> Value {
    assistant_at(seq, seq + 1, model, None, usage)
}

fn usage_chunk(seq: i64, step: i64, usage: Value) -> Value {
    json!({
        "type": "assistant/chunk",
        "seq": seq,
        "time": DAY_START + seq * 1_000,
        "data": {"turn": 1, "step": step, "chunk": {"type": "usage", "usage": usage}}
    })
}

fn request_header(seq: i64, model: &str) -> Value {
    json!({
        "type": "request/header",
        "seq": seq,
        "time": DAY_START + seq * 1_000,
        "data": {
            "header": {"config": {"provider": "fixture", "model": model}},
            "reason": "initial"
        }
    })
}

fn retry_started(seq: i64, step: i64) -> Value {
    json!({
        "type": "llm/retry-started",
        "seq": seq,
        "time": DAY_START + seq * 1_000,
        "data": {"retryId": "retry-1", "turn": 1, "step": step, "retry": 1}
    })
}

fn compaction(seq: i64, model: &str, usage: Value) -> Value {
    json!({
        "type": "compaction/summary",
        "seq": seq,
        "time": DAY_START + seq * 1_000,
        "data": {
            "compactionId": format!("compact-{seq}"),
            "summary": [],
            "rawOutput": [],
            "llmStreamCall": true,
            "shadowedRange": {"start": 0, "end": 0},
            "shadowedSeqs": [0],
            "shadowedTokenCount": 1,
            "provider": "fixture",
            "model": model,
            "usage": usage
        }
    })
}

fn session_dir(home: &Path, project: &str, id: &str) -> PathBuf {
    home.join("sessions").join(project).join(id)
}

fn write_plain_session(home: &Path, project: &str, id: &str, records: &[Value]) {
    let content = records
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    write_file(
        &session_dir(home, project, id).join("session.jsonl"),
        &content,
    );
}

fn zstd_frame(content: &str) -> Vec<u8> {
    let mut encoder = zstd::stream::Encoder::new(Vec::new(), 1).expect("zstd encoder");
    encoder.include_checksum(true).expect("enable checksum");
    encoder.write_all(content.as_bytes()).expect("encode frame");
    encoder.finish().expect("finish frame")
}

fn daily_json(home: &Path) -> Value {
    let (ok, stdout, stderr) = run_ccstats(
        &[
            "daily",
            "--source",
            "dsh",
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
        &[("DSH_HOME", home)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    serde_json::from_slice(&stdout).expect("daily JSON")
}

#[test]
fn dsh_counts_final_messages_and_compaction_without_fork_copies() {
    let root = unique_temp_dir("dsh-accounting");
    let home = root.join("home");
    let cwd = "/workspace/project";
    let parent_records = [
        header("parent", cwd, None, None),
        request_header(0, "deepseek-v4-pro"),
        usage_chunk(1, 1, json!({"inputTokens": 9, "outputTokens": 5})),
        assistant_at(
            2,
            1,
            "deepseek-v4-pro",
            Some("deepseek-v4-pro-response"),
            json!({
                "inputTokens": 10,
                "outputTokens": 6,
                "cacheReadTokens": 4,
                "cacheWriteTokens": 2,
                "reasoningTokens": 3,
                "totalTokens": 22
            }),
        ),
        compaction(
            3,
            "deepseek-v4-flash",
            json!({"inputTokens": 2, "outputTokens": 3, "totalTokens": 7}),
        ),
        request_header(4, "failed-model"),
        usage_chunk(
            5,
            2,
            json!({"inputTokens": 1, "outputTokens": 1, "cacheReadTokens": 0, "cacheWriteTokens": 0, "totalTokens": 2}),
        ),
        retry_started(6, 2),
        usage_chunk(
            7,
            2,
            json!({"inputTokens": 2, "outputTokens": 1, "cacheReadTokens": 0, "cacheWriteTokens": 0, "totalTokens": 3}),
        ),
        assistant_at(
            8,
            2,
            "retry-route",
            Some("retry-response-model"),
            json!({"inputTokens": 2, "outputTokens": 1, "cacheReadTokens": 0, "cacheWriteTokens": 0, "totalTokens": 3}),
        ),
        json!({
            "type": "text-chunks",
            "seq0": 9,
            "time0": DAY_START + 9_000,
            "data": {"turn": 1, "step": 2, "index": 0, "dt": [0, 0], "texts": ["A", "B", "C"]}
        }),
    ];
    write_plain_session(&home, "--workspace-project--", "parent", &parent_records);

    let mut child_records = vec![header("child", cwd, Some("parent"), Some(12))];
    child_records.extend(parent_records[1..].iter().cloned());
    child_records.push(assistant_at(
        12,
        3,
        "mock-routed",
        None,
        json!({"inputTokens": 5, "outputTokens": 2}),
    ));
    write_plain_session(&home, "--workspace-project--", "child", &child_records);

    let json = daily_json(&home);
    let row = &json.as_array().expect("array")[0];
    assert_eq!(row["input_tokens"].as_i64(), Some(20));
    assert_eq!(row["output_tokens"].as_i64(), Some(10));
    assert_eq!(row["reasoning_tokens"].as_i64(), Some(3));
    assert_eq!(row["cache_read_tokens"].as_i64(), Some(4));
    assert_eq!(row["cache_creation_tokens"].as_i64(), Some(2));
    assert_eq!(row["total_tokens"].as_i64(), Some(41));
    assert_eq!(row["models"].as_array().map(Vec::len), Some(5));
    assert_eq!(row["data_quality"]["valid_entries"].as_i64(), Some(5));
    assert_eq!(row["data_quality"]["parse_errors"].as_u64(), Some(0));

    let (ok, stdout, stderr) = run_ccstats(
        &[
            "project",
            "--source",
            "dsh",
            "--json",
            "--offline",
            "--no-cost",
            "--timezone",
            "UTC",
        ],
        &[("DSH_HOME", &home)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    let projects: Value = serde_json::from_slice(&stdout).expect("project JSON");
    assert_eq!(projects[0]["project_path"], cwd);
    assert_eq!(projects[0]["session_count"].as_u64(), Some(2));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn dsh_reads_concatenated_zstd_frames_and_recovers_complete_torn_tail_records() {
    let root = unique_temp_dir("dsh-zstd");
    let home = root.join("home");
    let path = session_dir(&home, "--workspace-zstd--", "compressed").join("session.jsonl.zstd");
    write_file(&path.parent().expect("parent").join(".fixture-root"), "");
    let mut bytes =
        zstd_frame(&(header("compressed", "/workspace/zstd", None, None).to_string() + "\n"));
    bytes.extend(zstd_frame(
        &(assistant(
            0,
            "deepseek-v4-pro",
            json!({"inputTokens": 3, "outputTokens": 2}),
        )
        .to_string()
            + "\n"),
    ));
    let mut torn = zstd_frame(
        &(assistant(
            1,
            "deepseek-v4-flash",
            json!({"inputTokens": 4, "outputTokens": 1}),
        )
        .to_string()
            + "\n"),
    );
    torn.pop();
    bytes.extend(torn);
    fs::write(&path, bytes).expect("write compressed session");

    let damaged_path =
        session_dir(&home, "--workspace-zstd--", "damaged-torn").join("session.jsonl.zstd");
    write_file(
        &damaged_path.parent().expect("parent").join(".fixture-root"),
        "",
    );
    let mut damaged_bytes =
        zstd_frame(&(header("damaged-torn", "/workspace/zstd", None, None).to_string() + "\n"));
    let padding = (0..250_000)
        .map(|index| char::from(b'a' + u8::try_from(index % 26).expect("alphabet index")))
        .collect::<String>();
    let mut damaged_tail = zstd_frame(&format!(
        "{}\n{}\n",
        assistant(
            0,
            "must-not-survive-corruption",
            json!({"inputTokens": 100, "outputTokens": 10}),
        ),
        json!({"type": "future/event", "seq": 1, "time": DAY_START + 1_000, "padding": padding})
    ));
    let damage_at = damaged_tail.len().saturating_sub(12);
    damaged_tail[damage_at] ^= 0xff;
    damaged_tail.truncate(damaged_tail.len().saturating_sub(4));
    damaged_bytes.extend(damaged_tail);
    fs::write(damaged_path, damaged_bytes).expect("write damaged torn session");

    let json = daily_json(&home);
    let row = &json.as_array().expect("array")[0];
    assert_eq!(row["input_tokens"].as_i64(), Some(7));
    assert_eq!(row["output_tokens"].as_i64(), Some(3));
    assert_eq!(row["total_tokens"].as_i64(), Some(10));
    assert_eq!(row["data_quality"]["valid_entries"].as_i64(), Some(2));
    assert_eq!(row["data_quality"]["parse_errors"].as_u64(), Some(0));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn dsh_isolates_bad_usage_and_bad_files_without_losing_healthy_sessions() {
    let root = unique_temp_dir("dsh-errors");
    let home = root.join("home");
    write_plain_session(
        &home,
        "--workspace-good--",
        "good",
        &[
            header("good", "/workspace/good", None, None),
            assistant(
                0,
                "deepseek-v4-pro",
                json!({"inputTokens": -1, "outputTokens": 2}),
            ),
            assistant(
                1,
                "deepseek-v4-pro",
                json!({"inputTokens": 5, "outputTokens": 2, "reasoningTokens": 3}),
            ),
            assistant(
                2,
                "deepseek-v4-pro",
                json!({"inputTokens": 6, "outputTokens": 2}),
            ),
        ],
    );
    let corrupt = session_dir(&home, "--workspace-bad--", "bad").join("session.jsonl");
    write_file(&corrupt, "not-json\n");
    let oversized =
        session_dir(&home, "--workspace-oversized--", "oversized").join("session.jsonl");
    write_file(&oversized, "");
    fs::File::options()
        .write(true)
        .open(&oversized)
        .expect("open oversized fixture")
        .set_len(128 * 1024 * 1024 + 1)
        .expect("extend oversized fixture");

    let json = daily_json(&home);
    let row = &json.as_array().expect("array")[0];
    assert_eq!(row["input_tokens"].as_i64(), Some(6));
    assert_eq!(row["output_tokens"].as_i64(), Some(2));
    assert_eq!(row["data_quality"]["valid_entries"].as_i64(), Some(1));
    assert_eq!(row["data_quality"]["parse_errors"].as_u64(), Some(4));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn dsh_rejects_future_session_format_and_honors_relative_home() {
    let root = unique_temp_dir("dsh-home");
    let home = root.join("relative-home");
    let mut future = header("future", "/workspace/future", None, None);
    future["version"] = json!(1);
    write_plain_session(
        &home,
        "--workspace-future--",
        "future",
        &[
            future,
            assistant(
                0,
                "deepseek-v4-pro",
                json!({"inputTokens": 100, "outputTokens": 10}),
            ),
        ],
    );
    write_plain_session(
        &home,
        "--workspace-current--",
        "current",
        &[
            header("current", "/workspace/current", None, None),
            assistant(
                0,
                "deepseek-v4-pro",
                json!({"inputTokens": 4, "outputTokens": 1}),
            ),
        ],
    );

    let relative = Path::new("relative-home");
    let (ok, stdout, stderr) = run_ccstats(
        &[
            "daily",
            "--source",
            "dsh",
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
        &[("DSH_HOME", relative), ("CCSTATS_TEST_CWD", &root)],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    let json: Value = serde_json::from_slice(&stdout).expect("daily JSON");
    let row = &json.as_array().expect("array")[0];
    assert_eq!(row["total_tokens"].as_i64(), Some(5));
    assert_eq!(row["data_quality"]["valid_entries"].as_i64(), Some(1));
    assert_eq!(row["data_quality"]["parse_errors"].as_u64(), Some(1));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn dsh_rejects_committed_checksum_damage_without_losing_other_zstd_sessions() {
    let root = unique_temp_dir("dsh-checksum");
    let home = root.join("home");
    for (id, corrupt) in [("healthy", false), ("corrupt", true)] {
        let path = session_dir(&home, "--workspace-zstd--", id).join("session.jsonl.zstd");
        write_file(&path.parent().expect("parent").join(".fixture-root"), "");
        let mut bytes = zstd_frame(&(header(id, "/workspace/zstd", None, None).to_string() + "\n"));
        let mut event = zstd_frame(
            &(assistant(
                0,
                "deepseek-v4-pro",
                json!({"inputTokens": 3, "outputTokens": 1}),
            )
            .to_string()
                + "\n"),
        );
        if corrupt {
            let last = event.last_mut().expect("frame checksum");
            *last ^= 0xff;
        }
        bytes.extend(event);
        fs::write(path, bytes).expect("write zstd session");
    }

    let json = daily_json(&home);
    let row = &json.as_array().expect("array")[0];
    assert_eq!(row["total_tokens"].as_i64(), Some(4));
    assert_eq!(row["data_quality"]["valid_entries"].as_i64(), Some(1));
    assert_eq!(row["data_quality"]["parse_errors"].as_u64(), Some(1));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn dsh_rejects_a_root_that_mixes_plain_and_zstd_encodings() {
    let root = unique_temp_dir("dsh-mixed-encoding");
    let home = root.join("home");
    write_plain_session(
        &home,
        "--workspace-plain--",
        "plain",
        &[
            header("plain", "/workspace/plain", None, None),
            assistant(
                0,
                "deepseek-v4-pro",
                json!({"inputTokens": 4, "outputTokens": 1}),
            ),
        ],
    );
    let compressed =
        session_dir(&home, "--workspace-zstd--", "compressed").join("session.jsonl.zstd");
    write_file(
        &compressed.parent().expect("parent").join(".fixture-root"),
        "",
    );
    let mut bytes =
        zstd_frame(&(header("compressed", "/workspace/zstd", None, None).to_string() + "\n"));
    bytes.extend(zstd_frame(
        &(assistant(
            0,
            "deepseek-v4-pro",
            json!({"inputTokens": 100, "outputTokens": 10}),
        )
        .to_string()
            + "\n"),
    ));
    fs::write(compressed, bytes).expect("write zstd session");

    let json = daily_json(&home);
    let rows = json.as_array().expect("array");
    assert_eq!(rows.len(), 1);
    assert!(rows[0]["total_tokens"].is_null());
    assert_eq!(rows[0]["data_quality"]["valid_entries"].as_i64(), Some(0));
    assert_eq!(rows[0]["data_quality"]["parse_errors"].as_u64(), Some(1));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn dsh_keeps_zero_calls_seed_zero_children_and_ignores_plain_torn_noise() {
    let root = unique_temp_dir("dsh-zero-seed");
    let home = root.join("home");
    let parent_path = session_dir(&home, "--workspace-zero--", "parent").join("session.jsonl");
    let foreign_replay = json!({
        "type": "assistant/message",
        "seq": 0,
        "time": DAY_START,
        "data": {
            "turn": 1,
            "step": 1,
            "message": {
                "role": "assistant",
                "content": [],
                "source": {
                    "kind": "model",
                    "provider": "fixture",
                    "model": "source-model",
                    "replayState": {"response": {
                        "kind": "pi-ai",
                        "version": 2,
                        "provider": "other-provider",
                        "model": "source-model",
                        "responseModel": "poison-response-model"
                    }}
                },
                "id": "zero-message"
            },
            "usage": {"inputTokens": 0, "outputTokens": 0}
        }
    });
    write_plain_session(
        &home,
        "--workspace-zero--",
        "parent",
        &[
            header("parent", "/workspace/zero", None, None),
            foreign_replay,
        ],
    );
    OpenOptions::new()
        .append(true)
        .open(&parent_path)
        .expect("open parent")
        .write_all(br#"{"type":"assistant/message","seq":1"#)
        .expect("append torn row");
    write_plain_session(
        &home,
        "--workspace-zero--",
        "child",
        &[
            header("child", "/workspace/zero", Some("parent"), Some(0)),
            assistant(
                0,
                "child-model",
                json!({"inputTokens": 3, "outputTokens": 1}),
            ),
        ],
    );
    write_plain_session(
        &home.join("sessions/--workspace-deep--/deep/extra"),
        "ignored",
        "noise",
        &[
            header("noise", "/workspace/noise", None, None),
            assistant(0, "noise", json!({"inputTokens": 999, "outputTokens": 999})),
        ],
    );

    let json = daily_json(&home);
    let row = &json.as_array().expect("array")[0];
    assert_eq!(row["total_tokens"].as_i64(), Some(4));
    assert_eq!(row["data_quality"]["valid_entries"].as_i64(), Some(2));
    assert_eq!(row["data_quality"]["parse_errors"].as_u64(), Some(0));
    let models = row["models"].as_array().expect("models");
    assert!(models.iter().any(|model| model == "source-model"));
    assert!(models.iter().any(|model| model == "child-model"));
    assert!(models.iter().all(|model| model != "poison-response-model"));
    assert!(models.iter().all(|model| model != "noise"));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn dsh_invalid_request_headers_clear_stale_routes_even_in_seeded_history() {
    let root = unique_temp_dir("dsh-invalid-routes");
    let home = root.join("home");
    write_plain_session(
        &home,
        "--workspace-seeded-route--",
        "seeded-route",
        &[
            header("seeded-route", "/workspace/seeded-route", None, Some(2)),
            request_header(0, "stale-seeded-model"),
            json!({
                "type": "request/header",
                "seq": 1,
                "time": DAY_START + 1_000,
                "data": {"header": {}}
            }),
            usage_chunk(2, 1, json!({"inputTokens": 50, "outputTokens": 10})),
        ],
    );
    write_plain_session(
        &home,
        "--workspace-live-route--",
        "live-route",
        &[
            header("live-route", "/workspace/live-route", None, None),
            request_header(0, "stale-live-model"),
            json!({
                "type": "request/header",
                "seq": 1,
                "time": DAY_START + 1_000,
                "data": {"header": {"config": {"provider": "fixture", "model": " "}}}
            }),
            usage_chunk(2, 1, json!({"inputTokens": 60, "outputTokens": 10})),
        ],
    );
    write_plain_session(
        &home,
        "--workspace-healthy-route--",
        "healthy-route",
        &[
            header("healthy-route", "/workspace/healthy-route", None, None),
            assistant(
                0,
                "healthy-model",
                json!({"inputTokens": 3, "outputTokens": 1}),
            ),
        ],
    );

    let json = daily_json(&home);
    let row = &json.as_array().expect("array")[0];
    assert_eq!(row["total_tokens"].as_i64(), Some(4));
    assert_eq!(row["data_quality"]["valid_entries"].as_i64(), Some(1));
    assert_eq!(row["data_quality"]["parse_errors"].as_u64(), Some(4));
    let models = row["models"].as_array().expect("models");
    assert!(models.iter().all(|model| model != "stale-seeded-model"));
    assert!(models.iter().all(|model| model != "stale-live-model"));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn dsh_rejects_malformed_or_mismatched_retry_boundaries_without_phantom_attempts() {
    let root = unique_temp_dir("dsh-retry-boundaries");
    let home = root.join("home");
    let cases = [
        (
            "invalid-id",
            1,
            1,
            json!({"retryId": "", "turn": 1, "step": 1, "retry": 1}),
            5,
        ),
        (
            "missing-retry",
            1,
            2,
            json!({"retryId": "retry-2", "turn": 1, "step": 2}),
            6,
        ),
        (
            "mismatched-turn",
            1,
            3,
            json!({"retryId": "retry-3", "turn": 2, "step": 3, "retry": 1}),
            7,
        ),
        (
            "mismatched-step",
            1,
            4,
            json!({"retryId": "retry-4", "turn": 1, "step": 99, "retry": 1}),
            8,
        ),
    ];
    for (id, turn, step, retry_data, input_tokens) in cases {
        write_plain_session(
            &home,
            &format!("--workspace-{id}--"),
            id,
            &[
                header(id, &format!("/workspace/{id}"), None, None),
                request_header(0, "original-model"),
                usage_chunk(
                    1,
                    step,
                    json!({"inputTokens": input_tokens, "outputTokens": 1}),
                ),
                json!({
                    "type": "llm/retry-started",
                    "seq": 2,
                    "time": DAY_START + 2_000,
                    "data": retry_data
                }),
                json!({
                    "type": "assistant/chunk",
                    "seq": 3,
                    "time": DAY_START + 3_000,
                    "data": {
                        "turn": turn,
                        "step": step,
                        "chunk": {
                            "type": "usage",
                            "usage": {"inputTokens": 100, "outputTokens": 1}
                        }
                    }
                }),
            ],
        );
    }

    let json = daily_json(&home);
    let row = &json.as_array().expect("array")[0];
    assert_eq!(row["input_tokens"].as_i64(), Some(26));
    assert_eq!(row["output_tokens"].as_i64(), Some(4));
    assert_eq!(row["total_tokens"].as_i64(), Some(30));
    assert_eq!(row["data_quality"]["valid_entries"].as_i64(), Some(4));
    assert_eq!(row["data_quality"]["parse_errors"].as_u64(), Some(4));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn dsh_enforces_storage_identity_and_logical_sequence_before_counting() {
    let root = unique_temp_dir("dsh-storage-integrity");
    let home = root.join("[dsh]*home");
    let mut missing_id = assistant_at(
        2,
        1,
        "missing-id-model",
        None,
        json!({"inputTokens": 100, "outputTokens": 10}),
    );
    missing_id["data"]["message"]
        .as_object_mut()
        .expect("message object")
        .remove("id");
    write_plain_session(
        &home,
        "--workspace-healthy--",
        "healthy",
        &[
            header("healthy", "/workspace/healthy", None, None),
            assistant(
                0,
                "healthy-model",
                json!({"inputTokens": 3, "outputTokens": 1}),
            ),
            json!({
                "type": "text-chunks",
                "seq0": 1,
                "time0": DAY_START + 1_000,
                "data": {"turn": 1, "step": 1, "index": 0, "dt": [], "texts": ["A"]}
            }),
            assistant(
                2,
                "healthy-model",
                json!({"inputTokens": 2, "outputTokens": 1}),
            ),
            request_header(3, "request-route"),
            usage_chunk(4, 4, json!({"inputTokens": 7, "outputTokens": 1})),
            json!({
                "type": "assistant/message",
                "seq": 5,
                "time": DAY_START + 5_000,
                "data": {
                    "turn": 1,
                    "step": 4,
                    "message": {
                        "id": "final-route-message",
                        "role": "assistant",
                        "content": [],
                        "source": {
                            "kind": "model",
                            "provider": "fixture",
                            "model": "final-source",
                            "replayState": {
                                "response": {
                                    "kind": "pi-ai", "version": 2,
                                    "api": "openai-responses", "provider": "fixture",
                                    "model": "final-source", "responseModel": "final-response",
                                    "stopReason": "stop"
                                },
                                "blocks": []
                            }
                        }
                    }
                }
            }),
            json!({
                "type": "assistant/message",
                "seq": 6,
                "time": DAY_START + 6_000,
                "data": {
                    "turn": 1,
                    "step": 5,
                    "message": {
                        "id": "malformed-replay-message",
                        "role": "assistant",
                        "content": [],
                        "source": {
                            "kind": "model", "provider": "fixture", "model": "source-good",
                            "replayState": {
                                "response": {
                                    "kind": "pi-ai", "version": 2,
                                    "provider": "fixture", "model": "source-good",
                                    "responseModel": "poison-response", "stopReason": "stop"
                                },
                                "blocks": []
                            }
                        }
                    },
                    "usage": {"inputTokens": 1, "outputTokens": 1}
                }
            }),
        ],
    );
    write_plain_session(
        &home,
        "--workspace-duplicate-seq--",
        "duplicate-seq",
        &[
            header("duplicate-seq", "/workspace/duplicate-seq", None, None),
            assistant(
                0,
                "duplicate-model",
                json!({"inputTokens": 5, "outputTokens": 1}),
            ),
            assistant(
                0,
                "duplicate-model",
                json!({"inputTokens": 50, "outputTokens": 10}),
            ),
        ],
    );
    write_plain_session(
        &home,
        "--workspace-copied--",
        "copied",
        &[
            header("actual", "/workspace/actual", None, None),
            assistant(
                0,
                "copied-model",
                json!({"inputTokens": 100, "outputTokens": 10}),
            ),
        ],
    );
    write_plain_session(
        &home,
        "--workspace-malformed--",
        "malformed",
        &[
            header("malformed", "/workspace/malformed", None, None),
            json!({
                "type": "text-chunks",
                "seq0": 0,
                "time0": DAY_START,
                "data": {
                    "turn": 1, "step": 1, "index": 0, "dt": [0, 0],
                    "texts": ["A", "B", "C"], "args": ["A", "B", "C"]
                }
            }),
            assistant(
                3,
                "malformed-model",
                json!({"inputTokens": 100, "outputTokens": 10}),
            ),
        ],
    );
    write_plain_session(
        &home,
        "--workspace-future-required--",
        "future-required",
        &[
            header("future-required", "/workspace/future-required", None, None),
            json!({"type": "future/required", "seq": 0, "time": DAY_START, "data": {}}),
            assistant(
                1,
                "future-model",
                json!({"inputTokens": 100, "outputTokens": 10}),
            ),
        ],
    );
    write_plain_session(
        &home,
        "--workspace-future-ignorable--",
        "future-ignorable",
        &[
            header(
                "future-ignorable",
                "/workspace/future-ignorable",
                None,
                None,
            ),
            json!({
                "type": "future/ignorable", "seq": 0, "time": DAY_START,
                "data": {}, "ignorable": true
            }),
            assistant(
                1,
                "ignorable-model",
                json!({"inputTokens": 2, "outputTokens": 1}),
            ),
        ],
    );
    write_plain_session(
        &home,
        "--workspace-invalid-final--",
        "invalid-final",
        &[
            header("invalid-final", "/workspace/invalid-final", None, None),
            request_header(0, "provisional-model"),
            usage_chunk(1, 1, json!({"inputTokens": 100, "outputTokens": 10})),
            missing_id,
            assistant_at(
                3,
                2,
                "invalid-final-model",
                None,
                json!({"inputTokens": -1, "outputTokens": 10}),
            ),
        ],
    );
    let mut unsafe_seed = header("unsafe-seed", "/workspace/unsafe-seed", None, None);
    unsafe_seed["seedLength"] = json!(9_007_199_254_740_992_u64);
    write_plain_session(
        &home,
        "--workspace-unsafe-seed--",
        "unsafe-seed",
        &[
            unsafe_seed,
            assistant(
                0,
                "unsafe-seed-model",
                json!({"inputTokens": 100, "outputTokens": 10}),
            ),
        ],
    );
    let mut retired = header("retired", "/workspace/retired", None, None);
    retired["sandboxMode"] = Value::Null;
    write_plain_session(
        &home,
        "--workspace-retired--",
        "retired",
        &[
            retired,
            assistant(
                0,
                "retired-model",
                json!({"inputTokens": 100, "outputTokens": 10}),
            ),
        ],
    );

    let json = daily_json(&home);
    let row = &json.as_array().expect("array")[0];
    assert_eq!(row["input_tokens"].as_i64(), Some(20));
    assert_eq!(row["output_tokens"].as_i64(), Some(6));
    assert_eq!(row["total_tokens"].as_i64(), Some(26));
    assert_eq!(row["data_quality"]["valid_entries"].as_i64(), Some(6));
    assert_eq!(row["data_quality"]["parse_errors"].as_u64(), Some(8));
    let models = row["models"].as_array().expect("models");
    assert!(models.iter().any(|model| model == "final-response"));
    assert!(models.iter().any(|model| model == "source-good"));
    assert!(models.iter().all(|model| model != "request-route"));
    assert!(models.iter().all(|model| model != "poison-response"));
    assert!(models.iter().any(|model| model == "ignorable-model"));
    assert!(models.iter().all(|model| model != "future-model"));
    assert!(models.iter().all(|model| model != "provisional-model"));
    assert!(models.iter().all(|model| model != "missing-id-model"));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn dsh_reported_total_changes_display_but_not_component_pricing() {
    let root = unique_temp_dir("dsh-reported-total-pricing");
    let home = root.join("dsh-home");
    let cache = root.join("cache");
    let prices = r#"{"deepseek-priced-dsh":{"input_cost_per_token":0.001,"output_cost_per_token":0.002,"reasoning_output_cost_per_token":0.005,"cache_read_input_token_cost":0.004}}"#;
    write_file(&cache.join("ccstats/pricing.json"), prices);
    write_file(&root.join("Library/Caches/ccstats/pricing.json"), prices);
    write_file(&root.join(".cache/ccstats/pricing.json"), prices);
    write_plain_session(
        &home,
        "--workspace-priced--",
        "priced",
        &[
            header("priced", "/workspace/priced", None, None),
            assistant(
                0,
                "deepseek-priced-dsh",
                json!({
                    "inputTokens": 10,
                    "outputTokens": 6,
                    "cacheReadTokens": 4,
                    "reasoningTokens": 3,
                    "totalTokens": 30
                }),
            ),
        ],
    );

    let (ok, stdout, stderr) = run_ccstats(
        &[
            "daily",
            "--source",
            "dsh",
            "--json",
            "--offline",
            "--timezone",
            "UTC",
            "--since",
            "2026-08-31",
            "--until",
            "2026-08-31",
        ],
        &[
            ("DSH_HOME", home.as_path()),
            ("HOME", root.as_path()),
            ("XDG_CACHE_HOME", cache.as_path()),
        ],
    );
    assert!(ok, "stderr: {}", String::from_utf8_lossy(&stderr));
    let json: Value = serde_json::from_slice(&stdout).expect("daily JSON");
    let row = &json.as_array().expect("array")[0];
    assert_eq!(row["input_tokens"].as_i64(), Some(10));
    assert_eq!(row["output_tokens"].as_i64(), Some(3));
    assert_eq!(row["reasoning_tokens"].as_i64(), Some(3));
    assert_eq!(row["cache_read_tokens"].as_i64(), Some(4));
    assert_eq!(row["total_tokens"].as_i64(), Some(30));
    assert!((row["cost"].as_f64().expect("cost") - 0.047).abs() < 1e-12);
    let _ = fs::remove_dir_all(root);
}
