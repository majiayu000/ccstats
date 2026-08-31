//! Fx's bounded recovery registry and canonical event-log boundary.

use std::collections::HashSet;
use std::fs;
use std::path::Path;

use serde::Deserialize;
use serde_json::Value;

use crate::utils::Timezone;

use super::fx::{Ledger, deserialize_fact, valid_id};

mod event;

const MAX_SIDECAR_BYTES: usize = 256 * 1024 + 512;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Authority {
    schema_version: u64,
    session_id: String,
    #[serde(rename = "authority_id")]
    token: String,
    storage_format: String,
    source: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Watermark {
    schema_version: u64,
    session_id: String,
    log_generation: String,
    through_seq: u64,
    through_event_id: String,
    through_event_log_bytes: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EventFrame {
    schema_version: u64,
    log_generation: String,
    seq: u64,
    event_id: String,
    timestamp_ms: i64,
    kind: String,
    payload: Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionStarted {
    id: String,
    created_at_ms: i64,
    origin_workspace_root: String,
    workspace_root: String,
    conversation_language: String,
    preferences: Preferences,
    usage: Option<DurableSnapshot>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Preferences {
    model: String,
    effort: String,
    fast_mode: bool,
    #[serde(default)]
    provider: Option<String>,
}

#[derive(Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct DurableModel {
    model: String,
    first_sequence: u64,
    total_cost: f64,
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    cache_write_tokens: u64,
    billable_web_search_calls: u64,
}

#[derive(Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct DurablePending {
    id: String,
    sequence: u64,
    provider: String,
    origin: String,
    team: Option<String>,
    credential_source: Option<String>,
    credential_identity: Option<String>,
    account_id: Option<String>,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct DurableSnapshot {
    billing: String,
    api_duration_complete: bool,
    wall_duration_complete: bool,
    code_complete: bool,
    next_sequence: u64,
    settled_through_sequence: u64,
    api_duration_ms: u64,
    wall_duration_ms: u64,
    total_cost: f64,
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    cache_write_tokens: u64,
    billable_web_search_calls: u64,
    lines_added: u64,
    lines_removed: u64,
    models: Vec<DurableModel>,
    pending: Vec<DurablePending>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RecoverySidecar {
    schema_version: u64,
    session_id: String,
    snapshot: RichSnapshot,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RichModel {
    model: String,
    first_sequence: u64,
    total_cost: f64,
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    cache_write_tokens: u64,
    reasoning_tokens: Option<u64>,
    request_count: Option<u64>,
    billable_web_search_calls: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RichPending {
    id: String,
    sequence: u64,
    provider: String,
    origin: String,
    team: Option<String>,
    credential_source: Option<String>,
    credential_identity: Option<String>,
    account_id: Option<String>,
    observed_at_ms: Option<i64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RecoveryIncident {
    occurred_at_ms: i64,
    completeness: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RichSnapshot {
    schema_version: u64,
    billing: String,
    api_duration_complete: bool,
    wall_duration_complete: bool,
    code_complete: bool,
    next_sequence: u64,
    settled_through_sequence: u64,
    api_duration_ms: u64,
    wall_duration_ms: u64,
    total_cost: f64,
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    cache_write_tokens: u64,
    reasoning_tokens: Option<u64>,
    request_count: Option<u64>,
    billable_web_search_calls: u64,
    lines_added: u64,
    lines_removed: u64,
    models: Vec<RichModel>,
    pending: Vec<RichPending>,
    publication_backlog: Vec<Value>,
    incidents: Vec<RecoveryIncident>,
}

struct CanonicalState {
    updated_at_ms: i64,
    usage: DurableSnapshot,
}

enum SidecarReconciliation {
    ExactBillingAndRollback,
    RichExtension,
    BillingMismatch,
}

pub(super) fn parse_recovery_dir(
    root: &Path,
    timezone: Timezone,
    debug: bool,
    ledger: &mut Ledger,
) {
    let recovery_dir = root.join("usage-recovery");
    if !private_directory(root) || !private_directory(&recovery_dir) {
        if recovery_dir.exists() {
            ledger.add_error();
        }
        return;
    }
    let entries = match fs::read_dir(&recovery_dir) {
        Ok(entries) => entries.collect::<Result<Vec<_>, _>>(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(_) => {
            ledger.add_error();
            return;
        }
    };
    let Ok(entries) = entries else {
        ledger.add_error();
        return;
    };
    if entries.len() > 512
        || entries
            .iter()
            .any(|entry| !entry.file_type().is_ok_and(|kind| kind.is_file()))
    {
        ledger.add_error();
        return;
    }
    let mut markers = entries
        .into_iter()
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    markers.sort();
    if markers.iter().any(|marker| {
        marker
            .file_name()
            .and_then(|name| name.to_str())
            .is_none_or(|id| !valid_session_id(id))
            || parse_marker_timestamp(marker).is_err()
    }) {
        ledger.add_error();
        return;
    }
    for marker in markers {
        let mut staged = ledger.clone();
        if let Err(error) = parse_marker(root, &marker, timezone, &mut staged) {
            ledger.add_error();
            if debug {
                eprintln!("Invalid Fx recovery marker {}: {error}", marker.display());
            }
        } else {
            *ledger = staged;
        }
    }
}

fn parse_marker(
    root: &Path,
    marker: &Path,
    timezone: Timezone,
    ledger: &mut Ledger,
) -> Result<(), &'static str> {
    let session_id = marker
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|id| valid_session_id(id))
        .ok_or("invalid recovery session id")?;
    let protected_at = parse_marker_timestamp(marker)?;
    let session_dir = root.join("sessions").join(session_id);
    if !private_directory(&root.join("sessions")) || !private_directory(&session_dir) {
        return Err("Fx canonical session directory is not private");
    }
    let canonical = load_canonical_state(&session_dir, session_id)?;
    let (rich, reconciliation) = load_rich_snapshot(&session_dir, session_id, &canonical.usage)?;
    let billing_mismatch = matches!(reconciliation, SidecarReconciliation::BillingMismatch);
    if billing_mismatch {
        // Fx preserves validated recovery hints across a billing-projection
        // mismatch, but marks the continuity gap as incomplete.
        ledger.add_error();
    }
    let (billing, next_sequence, settled_through_sequence, pending) = if billing_mismatch {
        (
            canonical.usage.billing.as_str(),
            canonical.usage.next_sequence,
            canonical.usage.settled_through_sequence,
            canonical
                .usage
                .pending
                .iter()
                .map(|pending| (pending.id.clone(), None))
                .collect::<Vec<_>>(),
        )
    } else {
        (
            rich.billing.as_str(),
            rich.next_sequence,
            rich.settled_through_sequence,
            rich.pending
                .iter()
                .map(|pending| (pending.id.clone(), pending.observed_at_ms))
                .collect::<Vec<_>>(),
        )
    };
    let needs_recovery = settled_through_sequence != next_sequence - 1
        || !pending.is_empty()
        || !rich.publication_backlog.is_empty()
        || !rich.incidents.is_empty();
    if !needs_recovery {
        return if canonical.updated_at_ms >= protected_at {
            Ok(())
        } else {
            Err("recovery marker is newer than canonical session")
        };
    }
    if canonical.updated_at_ms < protected_at {
        ledger.add_error();
    }
    let unsettled = settled_through_sequence != next_sequence - 1;
    let has_incidents = !rich.incidents.is_empty();
    let facts = rich
        .publication_backlog
        .into_iter()
        .map(deserialize_fact)
        .collect::<Result<Vec<_>, _>>()?;
    let pending = pending
        .into_iter()
        .map(|(id, observed_at_ms)| {
            let observed_at = observed_at_ms.unwrap_or(canonical.updated_at_ms);
            (observed_at >= 0)
                .then_some((id, observed_at))
                .ok_or("invalid recovery pending timestamp")
        })
        .collect::<Result<Vec<_>, _>>()?;
    ledger.add_recovery_batch(facts, pending, timezone)?;
    for _ in rich.incidents {
        ledger.add_error();
    }
    if unsettled || (billing == "incomplete" && !has_incidents) {
        ledger.add_error();
    }
    Ok(())
}

fn parse_marker_timestamp(marker: &Path) -> Result<i64, &'static str> {
    let bytes = read_private_file(marker).map_err(|_| "unreadable recovery marker")?;
    let text = std::str::from_utf8(&bytes).map_err(|_| "invalid recovery marker")?;
    text.strip_prefix("v1 ")
        .and_then(|value| value.strip_suffix('\n'))
        .filter(|value| !value.is_empty() && !value.contains('\n'))
        .and_then(|value| value.parse::<i64>().ok())
        .filter(|value| *value >= 0)
        .ok_or("invalid recovery marker")
}

fn load_canonical_state(
    session_dir: &Path,
    session_id: &str,
) -> Result<CanonicalState, &'static str> {
    if session_dir.join("authority.pending.json").exists()
        || session_dir.join("commit.pending.json").exists()
    {
        return Err("canonical session has an in-flight commit");
    }
    let authority: Authority = read_json(&session_dir.join("authority.json"))?;
    if authority.schema_version != 1
        || authority.session_id != session_id
        || !valid_hex_id(&authority.token)
        || authority.storage_format != "event_log_v1"
        || !matches!(
            authority.source.as_str(),
            "native_create" | "legacy_migration"
        )
    {
        return Err("invalid canonical authority marker");
    }
    let event_bytes = read_private_file(&session_dir.join("events.jsonl"))
        .map_err(|_| "missing canonical event log")?;
    let first_line_end = event_bytes
        .iter()
        .position(|byte| *byte == b'\n')
        .map(|index| index + 1)
        .ok_or("invalid canonical event log")?;
    let first: EventFrame = serde_json::from_slice(&event_bytes[..first_line_end])
        .map_err(|_| "invalid canonical event frame")?;
    if first.schema_version != 1
        || first.seq != 1
        || first.kind != "session_started"
        || !valid_hex_id(&first.log_generation)
        || !valid_hex_id(&first.event_id)
    {
        return Err("invalid canonical first event");
    }
    let watermark_name = format!("commit.{}.json", first.log_generation);
    let watermark: Watermark = read_json(&session_dir.join(watermark_name))?;
    if watermark.schema_version != 1
        || watermark.session_id != session_id
        || watermark.log_generation != first.log_generation
        || !valid_hex_id(&watermark.through_event_id)
    {
        return Err("invalid canonical commit watermark");
    }
    let boundary = usize::try_from(watermark.through_event_log_bytes)
        .ok()
        .filter(|length| *length > 0 && *length <= event_bytes.len())
        .ok_or("invalid canonical event boundary")?;
    let committed = &event_bytes[..boundary];
    if !committed.ends_with(b"\n") {
        return Err("truncated canonical event boundary");
    }
    event::replay_events(committed, session_id, &watermark)
}

fn validate_session_start(started: &SessionStarted, session_id: &str) -> Result<(), &'static str> {
    if started.id != session_id
        || started.created_at_ms < 0
        || started.origin_workspace_root.is_empty()
        || started.workspace_root.is_empty()
        || started.conversation_language.is_empty()
        || started.preferences.model.is_empty()
        || started.preferences.effort.is_empty()
    {
        return Err("invalid canonical session identity");
    }
    let _ = started.preferences.fast_mode;
    let _ = &started.preferences.provider;
    Ok(())
}

fn load_rich_snapshot(
    session_dir: &Path,
    session_id: &str,
    durable: &DurableSnapshot,
) -> Result<(RichSnapshot, SidecarReconciliation), &'static str> {
    let bytes = read_private_file(&session_dir.join("usage-v2.json"))
        .map_err(|_| "missing recovery sidecar")?;
    if bytes.is_empty() || bytes.len() > MAX_SIDECAR_BYTES {
        return Err("invalid recovery sidecar size");
    }
    let sidecar: RecoverySidecar =
        serde_json::from_slice(&bytes).map_err(|_| "invalid recovery sidecar")?;
    if sidecar.schema_version != 1 || sidecar.session_id != session_id {
        return Err("recovery sidecar identity mismatch");
    }
    validate_rich_snapshot(&sidecar.snapshot)?;
    let reconciliation = if !billing_projection_matches(durable, &sidecar.snapshot) {
        SidecarReconciliation::BillingMismatch
    } else if rollback_projection_matches(durable, &sidecar.snapshot) {
        SidecarReconciliation::ExactBillingAndRollback
    } else {
        SidecarReconciliation::RichExtension
    };
    Ok((sidecar.snapshot, reconciliation))
}

fn validate_durable_snapshot(snapshot: &DurableSnapshot) -> Result<(), &'static str> {
    validate_snapshot_base(
        &snapshot.billing,
        snapshot.next_sequence,
        snapshot.settled_through_sequence,
        snapshot.total_cost,
        snapshot.input_tokens,
        snapshot.output_tokens,
        snapshot.cache_read_tokens,
        snapshot.cache_write_tokens,
        snapshot.billable_web_search_calls,
        &snapshot.models,
        &snapshot.pending,
    )
}

#[allow(clippy::too_many_lines)]
fn validate_rich_snapshot(snapshot: &RichSnapshot) -> Result<(), &'static str> {
    if snapshot.schema_version != 3
        || snapshot.models.len() > 32
        || snapshot.pending.len() > 16
        || snapshot.publication_backlog.len() > 16
        || snapshot.incidents.len() > 16
    {
        return Err("invalid rich recovery snapshot shape");
    }
    let durable_models = snapshot
        .models
        .iter()
        .map(|model| DurableModel {
            model: model.model.clone(),
            first_sequence: model.first_sequence,
            total_cost: model.total_cost,
            input_tokens: model.input_tokens,
            output_tokens: model.output_tokens,
            cache_read_tokens: model.cache_read_tokens,
            cache_write_tokens: model.cache_write_tokens,
            billable_web_search_calls: model.billable_web_search_calls,
        })
        .collect::<Vec<_>>();
    let durable_pending = snapshot
        .pending
        .iter()
        .map(|pending| DurablePending {
            id: pending.id.clone(),
            sequence: pending.sequence,
            provider: pending.provider.clone(),
            origin: pending.origin.clone(),
            team: pending.team.clone(),
            credential_source: pending.credential_source.clone(),
            credential_identity: pending.credential_identity.clone(),
            account_id: pending.account_id.clone(),
        })
        .collect::<Vec<_>>();
    validate_snapshot_base(
        &snapshot.billing,
        snapshot.next_sequence,
        snapshot.settled_through_sequence,
        snapshot.total_cost,
        snapshot.input_tokens,
        snapshot.output_tokens,
        snapshot.cache_read_tokens,
        snapshot.cache_write_tokens,
        snapshot.billable_web_search_calls,
        &durable_models,
        &durable_pending,
    )?;
    let reasoning_sum = snapshot.models.iter().try_fold(0_u64, |sum, model| {
        let reasoning = model.reasoning_tokens?;
        (reasoning <= model.output_tokens)
            .then(|| sum.checked_add(reasoning))
            .flatten()
    });
    if snapshot.reasoning_tokens.is_some() && reasoning_sum != snapshot.reasoning_tokens {
        return Err("rich reasoning aggregate mismatch");
    }
    let request_sum = snapshot
        .models
        .iter()
        .try_fold(0_u64, |sum, model| sum.checked_add(model.request_count?));
    if snapshot.request_count.is_some() && request_sum != snapshot.request_count {
        return Err("rich request aggregate mismatch");
    }
    let mut fact_ids = HashSet::new();
    for value in &snapshot.publication_backlog {
        let fact = deserialize_fact(value.clone())?;
        if !fact_ids.insert(fact.id) {
            return Err("duplicate recovery generation fact");
        }
    }
    for incident in &snapshot.incidents {
        if incident.occurred_at_ms < 0
            || !matches!(incident.completeness.as_str(), "pending" | "incomplete")
        {
            return Err("invalid recovery incident");
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn validate_snapshot_base(
    billing: &str,
    next_sequence: u64,
    settled_through_sequence: u64,
    total_cost: f64,
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    cache_write_tokens: u64,
    web_search_calls: u64,
    models: &[DurableModel],
    pending: &[DurablePending],
) -> Result<(), &'static str> {
    if !matches!(billing, "complete" | "pending" | "incomplete" | "legacy")
        || next_sequence == 0
        || settled_through_sequence >= next_sequence
        || !total_cost.is_finite()
        || total_cost < 0.0
        || models.len() > 32
        || pending.len() > 16
        || (billing == "complete" && !pending.is_empty())
        || (billing == "pending" && pending.is_empty())
    {
        return Err("invalid usage snapshot state");
    }
    let mut names = HashSet::new();
    let mut sums = (0_u64, 0_u64, 0_u64, 0_u64, 0_u64, 0_f64);
    let mut prior_sequence = 0;
    for model in models {
        if model.model.is_empty()
            || model.model.len() > 1024
            || !model
                .model
                .bytes()
                .all(|byte| (0x21..=0x7e).contains(&byte))
            || !names.insert(model.model.as_str())
            || model.first_sequence == 0
            || model.first_sequence >= next_sequence
            || model.first_sequence <= prior_sequence
            || !model.total_cost.is_finite()
            || model.total_cost < 0.0
            || model.cache_read_tokens > model.input_tokens
            || model.cache_write_tokens > model.input_tokens
        {
            return Err("invalid usage model aggregate");
        }
        prior_sequence = model.first_sequence;
        sums.0 = sums
            .0
            .checked_add(model.input_tokens)
            .ok_or("usage overflow")?;
        sums.1 = sums
            .1
            .checked_add(model.output_tokens)
            .ok_or("usage overflow")?;
        sums.2 = sums
            .2
            .checked_add(model.cache_read_tokens)
            .ok_or("usage overflow")?;
        sums.3 = sums
            .3
            .checked_add(model.cache_write_tokens)
            .ok_or("usage overflow")?;
        sums.4 = sums
            .4
            .checked_add(model.billable_web_search_calls)
            .ok_or("usage overflow")?;
        sums.5 += model.total_cost;
    }
    let tolerance = (total_cost * 1e-12).max(1e-12);
    if (sums.0, sums.1, sums.2, sums.3, sums.4)
        != (
            input_tokens,
            output_tokens,
            cache_read_tokens,
            cache_write_tokens,
            web_search_calls,
        )
        || !sums.5.is_finite()
        || (sums.5 - total_cost).abs() > tolerance
    {
        return Err("usage snapshot aggregate mismatch");
    }
    let mut pending_ids = HashSet::new();
    let mut pending_sequences = HashSet::new();
    for item in pending {
        if !valid_id(&item.id)
            || item.sequence == 0
            || item.sequence >= next_sequence
            || item.provider.is_empty()
            || item.origin.is_empty()
            || !pending_ids.insert(item.id.as_str())
            || !pending_sequences.insert(item.sequence)
        {
            return Err("invalid pending generation");
        }
    }
    Ok(())
}

fn billing_projection_matches(durable: &DurableSnapshot, rich: &RichSnapshot) -> bool {
    durable.billing == rich.billing
        && durable.next_sequence == rich.next_sequence
        && durable.settled_through_sequence == rich.settled_through_sequence
        && float_equal(durable.total_cost, rich.total_cost)
        && durable.input_tokens == rich.input_tokens
        && durable.output_tokens == rich.output_tokens
        && durable.cache_read_tokens == rich.cache_read_tokens
        && durable.cache_write_tokens == rich.cache_write_tokens
        && durable.billable_web_search_calls == rich.billable_web_search_calls
        && durable.models.len() == rich.models.len()
        && durable
            .models
            .iter()
            .zip(&rich.models)
            .all(|(left, right)| {
                left.model == right.model
                    && left.first_sequence == right.first_sequence
                    && float_equal(left.total_cost, right.total_cost)
                    && left.input_tokens == right.input_tokens
                    && left.output_tokens == right.output_tokens
                    && left.cache_read_tokens == right.cache_read_tokens
                    && left.cache_write_tokens == right.cache_write_tokens
                    && left.billable_web_search_calls == right.billable_web_search_calls
            })
        && durable.pending.len() == rich.pending.len()
        && durable
            .pending
            .iter()
            .zip(&rich.pending)
            .all(|(left, right)| {
                left.id == right.id
                    && left.sequence == right.sequence
                    && left.provider == right.provider
                    && left.origin == right.origin
                    && left.team == right.team
                    && left.credential_source == right.credential_source
                    && left.credential_identity == right.credential_identity
                    && left.account_id == right.account_id
            })
}

fn rollback_projection_matches(durable: &DurableSnapshot, rich: &RichSnapshot) -> bool {
    billing_projection_matches(durable, rich)
        && durable.api_duration_complete == rich.api_duration_complete
        && durable.wall_duration_complete == rich.wall_duration_complete
        && durable.code_complete == rich.code_complete
        && durable.api_duration_ms == rich.api_duration_ms
        && durable.wall_duration_ms == rich.wall_duration_ms
        && durable.lines_added == rich.lines_added
        && durable.lines_removed == rich.lines_removed
}

fn valid_session_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 255
        && !matches!(id, "." | "..")
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn valid_hex_id(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn float_equal(left: f64, right: f64) -> bool {
    left.partial_cmp(&right) == Some(std::cmp::Ordering::Equal)
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, &'static str> {
    let bytes = read_private_file(path).map_err(|_| "missing canonical session file")?;
    serde_json::from_slice(&bytes).map_err(|_| "invalid canonical session file")
}

fn read_private_file(path: &Path) -> Result<Vec<u8>, std::io::Error> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.file_type().is_file() || !private_file_metadata(&metadata) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "Fx managed file is not private",
        ));
    }
    fs::read(path)
}

#[cfg(unix)]
fn private_file_metadata(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;

    metadata.nlink() == 1 && metadata.mode() & 0o777 == 0o600
}

#[cfg(not(unix))]
fn private_file_metadata(_: &fs::Metadata) -> bool {
    true
}

fn private_directory(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| {
        metadata.file_type().is_dir() && private_directory_metadata(&metadata)
    })
}

#[cfg(unix)]
fn private_directory_metadata(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;

    metadata.mode() & 0o777 == 0o700
}

#[cfg(not(unix))]
fn private_directory_metadata(_: &fs::Metadata) -> bool {
    true
}
