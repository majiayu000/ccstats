//! `DeepSeek` Harness JSONL storage-shape validation.

use std::fs;
use std::path::Path;

use serde_json::{Map, Value};

const MAX_SAFE_INTEGER: i64 = 9_007_199_254_740_991;

pub(super) fn valid_header_shape(bytes: &[u8]) -> bool {
    let Ok(Value::Object(header)) = serde_json::from_slice(bytes) else {
        return false;
    };
    !header.contains_key("sandboxMode")
        && !header.contains_key("approvalPolicy")
        && ["cwd", "parentSession", "origin", "agentPreset"]
            .into_iter()
            .all(|key| header.get(key).is_none_or(Value::is_string))
        && header.get("seedLength").is_none_or(Value::is_i64)
}

pub(super) fn matches_storage_identity(path: &Path, id: &str, cwd: Option<&str>) -> bool {
    let Some(root) = path.parent().and_then(Path::parent).and_then(Path::parent) else {
        return false;
    };
    let Some(session) = encode_segment(id) else {
        return false;
    };
    let project = cwd.map_or_else(|| "_no-cwd".to_string(), project_key);
    let Some(filename) = path.file_name() else {
        return false;
    };
    let expected = root.join(project).join(session).join(filename);
    path == expected
        || fs::canonicalize(path)
            .ok()
            .zip(fs::canonicalize(expected).ok())
            .is_some_and(|(actual, expected)| actual == expected)
}

fn encode_segment(raw: &str) -> Option<String> {
    if raw.is_empty() {
        return None;
    }
    if raw == "." {
        return Some("~002E".to_string());
    }
    if raw == ".." {
        return Some("~002E~002E".to_string());
    }
    Some(encode_units(raw, false))
}

fn project_key(cwd: &str) -> String {
    let readable = encode_units(cwd, true);
    let slug = readable.trim_start_matches('-');
    let slug = if slug.is_empty() { "root" } else { slug };
    format!("--{}--", &slug[..slug.len().min(251)])
}

fn encode_units(raw: &str, collapse_separators: bool) -> String {
    let mut encoded = String::new();
    let mut separator_run = false;
    for unit in raw.encode_utf16() {
        let ascii = u8::try_from(unit).ok();
        let safe = ascii
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'));
        let separator =
            collapse_separators && ascii.is_some_and(|byte| matches!(byte, b'/' | b'\\' | b':'));
        if separator {
            if !separator_run {
                encoded.push('-');
            }
            separator_run = true;
        } else {
            separator_run = false;
            if safe {
                encoded.push(char::from(ascii.unwrap_or_default()));
            } else {
                encoded.push('~');
                for shift in [12, 8, 4, 0] {
                    let digit = u8::try_from((unit >> shift) & 0x0f).unwrap_or_default();
                    encoded.push(char::from(if digit < 10 {
                        b'0' + digit
                    } else {
                        b'A' + digit - 10
                    }));
                }
            }
        }
    }
    encoded
}

pub(super) fn packed_row_span(value: &Value, kind: &str) -> Option<(i64, i64)> {
    let row = value.as_object()?;
    if !exact_keys(row, &["type", "seq0", "time0", "data"]) {
        return None;
    }
    let seq0 = safe_integer(row.get("seq0")?, true)?;
    let mut time = safe_integer(row.get("time0")?, false)?;
    let data = row.get("data")?.as_object()?;
    let payload_key = if kind == "tool-call-chunks" {
        "args"
    } else {
        "texts"
    };
    let valid_keys = if payload_key == "args" {
        exact_keys(data, &["turn", "step", "index", "id", "dt", "args"])
            || exact_keys(data, &["turn", "step", "index", "id", "name", "dt", "args"])
    } else {
        exact_keys(data, &["turn", "step", "index", "dt", "texts"])
    };
    if !valid_keys
        || !["turn", "step", "index"]
            .into_iter()
            .all(|key| data.get(key).is_some_and(Value::is_number))
        || payload_key == "args" && !data.get("id").is_some_and(Value::is_string)
        || data.get("name").is_some_and(|name| !name.is_string())
    {
        return None;
    }
    let payload = data.get(payload_key)?.as_array()?;
    let gaps = data.get("dt")?.as_array()?;
    if payload.is_empty()
        || payload.iter().any(|item| !item.is_string())
        || gaps.len() + 1 != payload.len()
    {
        return None;
    }
    for gap in gaps {
        time = time.checked_add(safe_integer(gap, false)?)?;
        if time.unsigned_abs() > MAX_SAFE_INTEGER as u64 {
            return None;
        }
    }
    let count = i64::try_from(payload.len()).ok()?;
    (seq0.checked_add(count - 1)? <= MAX_SAFE_INTEGER).then_some((seq0, count))
}

fn exact_keys(object: &Map<String, Value>, keys: &[&str]) -> bool {
    object.len() == keys.len() && keys.iter().all(|key| object.contains_key(*key))
}

fn safe_integer(value: &Value, nonnegative: bool) -> Option<i64> {
    let value = value.as_i64()?;
    (value.unsigned_abs() <= MAX_SAFE_INTEGER as u64 && (!nonnegative || value >= 0))
        .then_some(value)
}

pub(super) fn valid_event_envelope(value: &Value) -> bool {
    let Some(event) = value.as_object() else {
        return false;
    };
    event.keys().all(|key| {
        matches!(
            key.as_str(),
            "type" | "seq" | "time" | "data" | "surfaceOp" | "sourceEventSeqs" | "ignorable"
        )
    }) && event.get("type").is_some_and(Value::is_string)
        && event
            .get("seq")
            .and_then(|value| safe_integer(value, true))
            .is_some()
        && event
            .get("time")
            .and_then(|value| safe_integer(value, false))
            .is_some()
        && event.contains_key("data")
        && event.get("ignorable").is_none_or(|value| value == true)
}

pub(super) fn supported_event(kind: &str) -> bool {
    matches!(
        kind,
        "agent-preset/selected"
            | "agent/inbox/spliced"
            | "approval/asked"
            | "approval/decided"
            | "approval/policy"
            | "assistant/chunk"
            | "assistant/message"
            | "command/done"
            | "command/run"
            | "compaction/end"
            | "compaction/prune"
            | "compaction/start"
            | "compaction/summary"
            | "feedback/record"
            | "goal/change"
            | "hook/invoked"
            | "hook/result"
            | "llm/retry"
            | "llm/retry-started"
            | "model/selection"
            | "permission/preset"
            | "plan/mode"
            | "request/context"
            | "request/header"
            | "sandbox/mode"
            | "schedule/change"
            | "session-log-deepseek/delivery-accepted"
            | "session/end-seed"
            | "session/title"
            | "session/title-llm-request"
            | "step/end"
            | "step/start"
            | "subagent/descriptor"
            | "subagent/model-selection-policy"
            | "team/member"
            | "team/message/delivered"
            | "team/message/queued"
            | "team/task"
            | "todo/write"
            | "tool-workflow/agent-end"
            | "tool-workflow/agent-start"
            | "tool-workflow/run-end"
            | "tool-workflow/run-start"
            | "tool/call"
            | "tool/code-dispatch"
            | "tool/code-dispatch-start"
            | "tool/result"
            | "turn/end"
            | "turn/start"
            | "user/message"
            | "web/deepseek-search-llm-request"
    )
}
