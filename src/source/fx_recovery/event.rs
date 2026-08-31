//! Canonical Fx event-log replay, including atomic state replacement.

use base64::Engine;
use base64::prelude::BASE64_STANDARD;
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::{
    CanonicalState, DurableSnapshot, EventFrame, Preferences, SessionStarted, Watermark,
    valid_hex_id, validate_durable_snapshot, validate_session_start,
};

const MAX_EVENT_FRAME_BYTES: usize = 8 * 1024 * 1024;
const RAW_STATE_CHUNK_BYTES: usize = 4 * 1024 * 1024;

struct ReplayState {
    id: String,
    origin_workspace_root: String,
    workspace_root: String,
    created_at_ms: i64,
    updated_at_ms: i64,
    usage: Option<DurableSnapshot>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkspaceRebound {
    previous_workspace_root: String,
    workspace_root: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PreferencesChanged {
    model: Option<String>,
    effort: Option<String>,
    fast_mode: Option<bool>,
    provider: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HistoryCommitted {
    conversation_language: String,
    total_input_tokens: u64,
    total_output_tokens: u64,
    turn: Value,
    #[serde(default)]
    work_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RecoverySet {
    checkpoint: Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RecoveryCleared {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReplacementStart {
    replacement_id: String,
    reason: String,
    encoded_bytes: u64,
    sha256: String,
    chunk_count: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReplacementChunk {
    replacement_id: String,
    chunk_index: u64,
    raw_bytes: u64,
    chunk_sha256: String,
    base64: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReplacementCommit {
    replacement_id: String,
    encoded_bytes: u64,
    sha256: String,
    chunk_count: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReplacementState {
    id: String,
    origin_workspace_root: String,
    workspace_root: String,
    created_at_ms: i64,
    updated_at_ms: i64,
    conversation_language: String,
    preferences: Preferences,
    history: Vec<Value>,
    total_input_tokens: u64,
    total_output_tokens: u64,
    #[serde(default)]
    context_history_start: usize,
    permission_state: Value,
    #[serde(default)]
    usage: Option<DurableSnapshot>,
    #[serde(default)]
    last_subagent_work_id: Option<String>,
    #[serde(default)]
    recovery_checkpoint: Option<Value>,
}

pub(super) fn replay_events(
    committed: &[u8],
    session_id: &str,
    watermark: &Watermark,
) -> Result<CanonicalState, &'static str> {
    let frames = parse_frames(committed, watermark)?;
    let first = frames.first().ok_or("empty canonical event log")?;
    if first.kind != "session_started" {
        return Err("canonical log does not start a session");
    }
    let started: SessionStarted = serde_json::from_value(first.payload.clone())
        .map_err(|_| "invalid canonical session start")?;
    validate_session_start(&started, session_id)?;
    let mut state = ReplayState {
        id: started.id,
        origin_workspace_root: started.origin_workspace_root,
        workspace_root: started.workspace_root,
        created_at_ms: started.created_at_ms,
        updated_at_ms: first.timestamp_ms,
        usage: started.usage,
    };
    let mut index = 1;
    while index < frames.len() {
        let frame = &frames[index];
        match frame.kind.as_str() {
            "usage_checkpointed" => {
                state.usage = Some(parse_usage_checkpoint(&frame.payload)?);
                state.updated_at_ms = frame.timestamp_ms;
                index += 1;
            }
            "workspace_rebound" => {
                let rebound: WorkspaceRebound = serde_json::from_value(frame.payload.clone())
                    .map_err(|_| "invalid workspace rebound")?;
                if rebound.previous_workspace_root != state.workspace_root
                    || rebound.workspace_root.is_empty()
                    || rebound.workspace_root == state.workspace_root
                {
                    return Err("invalid workspace rebound");
                }
                state.workspace_root = rebound.workspace_root;
                state.updated_at_ms = frame.timestamp_ms;
                index += 1;
            }
            "preferences_changed" => {
                validate_preferences_changed(&frame.payload)?;
                state.updated_at_ms = frame.timestamp_ms;
                index += 1;
            }
            "history_turn_committed" => {
                validate_history_committed(&frame.payload)?;
                state.updated_at_ms = frame.timestamp_ms;
                index += 1;
            }
            "recovery_checkpoint_set" => {
                let payload: RecoverySet = serde_json::from_value(frame.payload.clone())
                    .map_err(|_| "invalid recovery checkpoint event")?;
                if !payload.checkpoint.is_object() {
                    return Err("invalid recovery checkpoint event");
                }
                state.updated_at_ms = frame.timestamp_ms;
                index += 1;
            }
            "recovery_checkpoint_cleared" => {
                serde_json::from_value::<RecoveryCleared>(frame.payload.clone())
                    .map_err(|_| "invalid recovery checkpoint clear")?;
                state.updated_at_ms = frame.timestamp_ms;
                index += 1;
            }
            "state_replacement_started" => {
                index = apply_replacement(&frames, index, &mut state)?;
            }
            "session_started" | "state_replacement_chunk" | "state_replacement_committed" => {
                return Err("invalid canonical event order");
            }
            _ => return Err("unknown canonical event kind"),
        }
    }
    let final_frame = frames.last().ok_or("empty canonical event log")?;
    if final_frame.seq != watermark.through_seq
        || final_frame.event_id != watermark.through_event_id
    {
        return Err("canonical watermark does not match event log");
    }
    let usage = state
        .usage
        .ok_or("canonical session has no durable usage")?;
    validate_durable_snapshot(&usage)?;
    Ok(CanonicalState {
        updated_at_ms: state.updated_at_ms,
        usage,
    })
}

fn parse_frames(committed: &[u8], watermark: &Watermark) -> Result<Vec<EventFrame>, &'static str> {
    let mut frames = Vec::new();
    for (index, line) in committed.split_inclusive(|byte| *byte == b'\n').enumerate() {
        if line.len() > MAX_EVENT_FRAME_BYTES {
            return Err("canonical event frame is too large");
        }
        let frame: EventFrame =
            serde_json::from_slice(line).map_err(|_| "invalid canonical event frame")?;
        let expected_seq = u64::try_from(index)
            .ok()
            .and_then(|value| value.checked_add(1))
            .ok_or("canonical event sequence overflow")?;
        if frame.schema_version != 1
            || frame.seq != expected_seq
            || frame.log_generation != watermark.log_generation
            || !valid_hex_id(&frame.event_id)
            || frame.timestamp_ms < 0
        {
            return Err("invalid canonical event sequence");
        }
        frames.push(frame);
    }
    Ok(frames)
}

fn parse_usage_checkpoint(payload: &Value) -> Result<DurableSnapshot, &'static str> {
    let object = payload
        .as_object()
        .filter(|object| object.len() == 1)
        .ok_or("invalid usage checkpoint")?;
    serde_json::from_value(
        object
            .get("usage")
            .cloned()
            .ok_or("missing usage checkpoint")?,
    )
    .map_err(|_| "invalid usage checkpoint")
}

fn validate_preferences_changed(payload: &Value) -> Result<(), &'static str> {
    let changed: PreferencesChanged =
        serde_json::from_value(payload.clone()).map_err(|_| "invalid preferences change")?;
    if changed.model.is_none()
        && changed.effort.is_none()
        && changed.fast_mode.is_none()
        && changed.provider.is_none()
    {
        return Err("empty preferences change");
    }
    if changed.model.as_deref().is_some_and(str::is_empty)
        || changed.effort.as_deref().is_some_and(str::is_empty)
        || changed.provider.as_deref().is_some_and(str::is_empty)
    {
        return Err("invalid preferences change");
    }
    Ok(())
}

fn validate_history_committed(payload: &Value) -> Result<(), &'static str> {
    let committed: HistoryCommitted =
        serde_json::from_value(payload.clone()).map_err(|_| "invalid history commit")?;
    if committed.conversation_language.is_empty()
        || !committed.turn.is_object()
        || committed.work_id.as_deref().is_some_and(str::is_empty)
    {
        return Err("invalid history commit");
    }
    let _ = committed.total_input_tokens;
    let _ = committed.total_output_tokens;
    Ok(())
}

fn apply_replacement(
    frames: &[EventFrame],
    start_index: usize,
    prior: &mut ReplayState,
) -> Result<usize, &'static str> {
    let start_frame = &frames[start_index];
    let start: ReplacementStart = serde_json::from_value(start_frame.payload.clone())
        .map_err(|_| "invalid state replacement start")?;
    if !valid_hex_id(&start.replacement_id)
        || !valid_digest(&start.sha256)
        || !matches!(
            start.reason.as_str(),
            "compaction" | "migration" | "recovery" | "log_compaction"
        )
        || start.encoded_bytes == 0
        || start.chunk_count == 0
        || start.chunk_count != start.encoded_bytes.div_ceil(RAW_STATE_CHUNK_BYTES as u64)
    {
        return Err("invalid state replacement start");
    }
    let chunk_count =
        usize::try_from(start.chunk_count).map_err(|_| "state replacement chunk count overflow")?;
    let commit_index = start_index
        .checked_add(chunk_count)
        .and_then(|value| value.checked_add(1))
        .ok_or("state replacement sequence overflow")?;
    if commit_index >= frames.len() {
        return Err("truncated state replacement");
    }
    let mut encoded_state = Vec::new();
    for chunk_index in 0..chunk_count {
        let frame = &frames[start_index + chunk_index + 1];
        if frame.kind != "state_replacement_chunk" {
            return Err("invalid state replacement chunk order");
        }
        let chunk: ReplacementChunk = serde_json::from_value(frame.payload.clone())
            .map_err(|_| "invalid state replacement chunk")?;
        let bytes = BASE64_STANDARD
            .decode(&chunk.base64)
            .map_err(|_| "invalid state replacement base64")?;
        if BASE64_STANDARD.encode(&bytes) != chunk.base64
            || chunk.replacement_id != start.replacement_id
            || chunk.chunk_index != chunk_index as u64
            || chunk.raw_bytes != bytes.len() as u64
            || !digest_matches(&bytes, &chunk.chunk_sha256)
        {
            return Err("invalid state replacement chunk");
        }
        let final_chunk = chunk_index + 1 == chunk_count;
        if (!final_chunk && bytes.len() != RAW_STATE_CHUNK_BYTES)
            || (final_chunk && (bytes.is_empty() || bytes.len() > RAW_STATE_CHUNK_BYTES))
        {
            return Err("invalid state replacement chunk size");
        }
        encoded_state.extend_from_slice(&bytes);
    }
    if encoded_state.len() as u64 != start.encoded_bytes
        || !digest_matches(&encoded_state, &start.sha256)
    {
        return Err("state replacement digest mismatch");
    }
    let commit_frame = &frames[commit_index];
    if commit_frame.kind != "state_replacement_committed" {
        return Err("missing state replacement commit");
    }
    let commit: ReplacementCommit = serde_json::from_value(commit_frame.payload.clone())
        .map_err(|_| "invalid state replacement commit")?;
    if commit.replacement_id != start.replacement_id
        || commit.encoded_bytes != start.encoded_bytes
        || commit.chunk_count != start.chunk_count
        || commit.sha256 != start.sha256
    {
        return Err("state replacement commit mismatch");
    }
    let replacement: ReplacementState =
        serde_json::from_slice(&encoded_state).map_err(|_| "invalid state replacement state")?;
    validate_replacement_state(
        &replacement,
        prior,
        commit_frame.timestamp_ms,
        &start.reason,
    )?;
    prior.workspace_root = replacement.workspace_root;
    prior.updated_at_ms = replacement.updated_at_ms;
    prior.usage = replacement.usage;
    Ok(commit_index + 1)
}

fn validate_replacement_state(
    replacement: &ReplacementState,
    prior: &ReplayState,
    commit_timestamp: i64,
    reason: &str,
) -> Result<(), &'static str> {
    if replacement.id != prior.id
        || replacement.origin_workspace_root != prior.origin_workspace_root
        || replacement.workspace_root != prior.workspace_root
        || replacement.created_at_ms != prior.created_at_ms
        || replacement.updated_at_ms != commit_timestamp
        || (reason == "log_compaction" && commit_timestamp != prior.updated_at_ms)
        || replacement.conversation_language.is_empty()
        || replacement.preferences.model.is_empty()
        || replacement.preferences.effort.is_empty()
        || replacement.context_history_start > replacement.history.len()
        || !replacement.permission_state.is_object()
        || replacement.history.iter().any(|turn| !turn.is_object())
        || replacement
            .last_subagent_work_id
            .as_deref()
            .is_some_and(str::is_empty)
        || replacement
            .recovery_checkpoint
            .as_ref()
            .is_some_and(|checkpoint| !checkpoint.is_object())
    {
        return Err("invalid state replacement identity");
    }
    if replacement
        .preferences
        .provider
        .as_deref()
        .is_some_and(str::is_empty)
    {
        return Err("invalid state replacement preferences");
    }
    let _ = replacement.preferences.fast_mode;
    let _ = replacement.total_input_tokens;
    let _ = replacement.total_output_tokens;
    if let Some(usage) = &replacement.usage {
        validate_durable_snapshot(usage)?;
    }
    Ok(())
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn digest_matches(bytes: &[u8], expected_hex: &str) -> bool {
    if !valid_digest(expected_hex) {
        return false;
    }
    let digest = Sha256::digest(bytes);
    digest.iter().enumerate().all(|(index, byte)| {
        let high = hex_digit(byte >> 4);
        let low = hex_digit(byte & 0x0f);
        expected_hex.as_bytes()[index * 2] == high && expected_hex.as_bytes()[index * 2 + 1] == low
    })
}

fn hex_digit(value: u8) -> u8 {
    match value {
        0..=9 => b'0' + value,
        10..=15 => b'a' + value - 10,
        _ => unreachable!(),
    }
}
