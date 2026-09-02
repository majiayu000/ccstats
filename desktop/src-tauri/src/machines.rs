use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use ccstats::{
    CurrentUsageWindow, MultiCostSummary, UsageRange, current_usage_windows_with_cli_config,
};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

const SCHEMA_VERSION: i64 = 2;
const MAX_BUNDLE_BYTES: usize = 10 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredMachineSnapshot {
    machine_id: String,
    machine_name: String,
    captured_at_ms: i64,
    windows: Vec<CurrentUsageWindow>,
    sources: Vec<MultiCostSummary>,
}

#[derive(Debug, Serialize, Deserialize)]
struct SnapshotBundle {
    schema_version: i64,
    machines: Vec<StoredMachineSnapshot>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub(crate) struct MachineUsageTotals {
    today_tokens: i64,
    week_tokens: i64,
    month_tokens: i64,
    today_cost: Option<f64>,
    week_cost: Option<f64>,
    month_cost: Option<f64>,
}

#[derive(Debug, Serialize)]
pub(crate) struct MachineUsage {
    machine_id: String,
    machine_name: String,
    captured_at_ms: i64,
    source_count: usize,
    currency: Option<String>,
    is_local: bool,
    today_current: bool,
    week_current: bool,
    month_current: bool,
    totals: MachineUsageTotals,
}

#[derive(Debug, Serialize)]
pub(crate) struct MachineRollup {
    local_machine_id: String,
    local_machine_name: Option<String>,
    currency: Option<String>,
    today_current_machines: usize,
    week_current_machines: usize,
    month_current_machines: usize,
    machines: Vec<MachineUsage>,
    totals: MachineUsageTotals,
}

fn open_database(path: &Path) -> Result<Connection, String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("machine database path has no parent: {}", path.display()))?;
    fs::create_dir_all(parent).map_err(|error| {
        format!(
            "failed to create machine database directory {}: {error}",
            parent.display()
        )
    })?;
    let connection = Connection::open(path).map_err(|error| {
        format!(
            "failed to open machine database {}: {error}",
            path.display()
        )
    })?;
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(|error| format!("failed to read machine database schema: {error}"))?;
    match version {
        0 => connection
            .execute_batch(
                "BEGIN;
                 CREATE TABLE app_state (key TEXT PRIMARY KEY, value TEXT NOT NULL);
                 CREATE TABLE machine_snapshots (
                   machine_id TEXT PRIMARY KEY,
                   machine_name TEXT NOT NULL,
                   captured_at_ms INTEGER NOT NULL,
                   windows_json TEXT NOT NULL,
                   payload_json TEXT NOT NULL
                 );
                 PRAGMA user_version = 2;
                 COMMIT;",
            )
            .map_err(|error| format!("failed to initialize machine database: {error}"))?,
        SCHEMA_VERSION => {}
        other => {
            return Err(format!(
                "unsupported machine database schema version {other}"
            ));
        }
    }
    Ok(connection)
}

fn local_machine_id(connection: &Connection) -> Result<String, String> {
    connection
        .execute(
            "INSERT OR IGNORE INTO app_state(key, value) VALUES ('local_machine_id', lower(hex(randomblob(16))))",
            [],
        )
        .map_err(|error| format!("failed to create local machine identity: {error}"))?;
    connection
        .query_row(
            "SELECT value FROM app_state WHERE key = 'local_machine_id'",
            [],
            |row| row.get(0),
        )
        .map_err(|error| format!("failed to read local machine identity: {error}"))
}

fn current_time_ms() -> Result<i64, String> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("system clock is before the Unix epoch: {error}"))?
        .as_millis();
    i64::try_from(millis)
        .map_err(|_| "current timestamp does not fit in SQLite INTEGER".to_string())
}

fn summary_for(source: &MultiCostSummary, range: UsageRange) -> Result<(i64, Option<f64>), String> {
    let rows: Vec<_> = source
        .summaries
        .iter()
        .filter(|summary| summary.range == range)
        .collect();
    if rows.len() != 1 {
        return Err(format!(
            "{} must contain exactly one {range:?} summary",
            source.display_name
        ));
    }
    let row = rows[0];
    if row.source != source.source
        || row.source_name != source.source_name
        || row.display_name != source.display_name
        || row.currency != source.currency
    {
        return Err(format!(
            "{} contains inconsistent summary identity",
            source.display_name
        ));
    }
    if row.tokens.total_tokens < 0 {
        return Err(format!(
            "{} contains negative token totals",
            source.display_name
        ));
    }
    if row.cost.is_some_and(|cost| !cost.is_finite() || cost < 0.0)
        || row
            .cost_usd
            .is_some_and(|cost| !cost.is_finite() || cost < 0.0)
    {
        return Err(format!("{} contains an invalid cost", source.display_name));
    }
    if !matches!(row.cost_kind.as_str(), "real" | "estimated_proxy" | "mixed") {
        return Err(format!(
            "{} contains an invalid cost kind",
            source.display_name
        ));
    }
    if !matches!(
        row.pricing_source.as_str(),
        "recorded" | "live" | "cache" | "cache_stale" | "fallback" | "unknown" | "mixed"
    ) {
        return Err(format!(
            "{} contains an invalid pricing source",
            source.display_name
        ));
    }
    let trusted_cost = row.cost_usd.filter(|_| {
        !row.api_equivalent_cost_coverage
            .as_ref()
            .is_some_and(|coverage| coverage.cost_is_lower_bound)
            && matches!(row.pricing_source.as_str(), "recorded" | "live" | "cache")
            && row.cost_kind == "real"
    });
    Ok((row.tokens.total_tokens, trusted_cost))
}

fn rolling_ranges() -> [UsageRange; 3] {
    [
        UsageRange::Today,
        UsageRange::ThisWeek,
        UsageRange::ThisMonth,
    ]
}

fn window_for<'a>(
    windows: &'a [CurrentUsageWindow],
    range: &UsageRange,
) -> Result<&'a CurrentUsageWindow, String> {
    let rows: Vec<_> = windows
        .iter()
        .filter(|window| window.range == *range)
        .collect();
    if rows.len() != 1 {
        return Err(format!(
            "machine snapshot must contain exactly one {range:?} window"
        ));
    }
    Ok(rows[0])
}

fn snapshot_freshness_against(
    snapshot: &StoredMachineSnapshot,
    current_windows: &[CurrentUsageWindow],
) -> Result<[bool; 3], String> {
    let mut freshness = [false; 3];
    for (index, range) in rolling_ranges().iter().enumerate() {
        freshness[index] =
            window_for(&snapshot.windows, range)? == window_for(current_windows, range)?;
    }
    Ok(freshness)
}

fn validate_snapshot(snapshot: &StoredMachineSnapshot) -> Result<(), String> {
    if snapshot.machine_id.trim().is_empty() || snapshot.machine_name.trim().is_empty() {
        return Err("machine ID and name must not be empty".to_string());
    }
    if snapshot.machine_id.len() > 128 || snapshot.machine_name.len() > 128 {
        return Err("machine ID and name must be at most 128 bytes".to_string());
    }
    if snapshot.captured_at_ms <= 0 {
        return Err(format!(
            "{} has an invalid capture time",
            snapshot.machine_name
        ));
    }
    if snapshot.sources.is_empty() {
        return Err(format!("{} has no source summaries", snapshot.machine_name));
    }
    for range in rolling_ranges() {
        let window = window_for(&snapshot.windows, &range)?;
        if window.since > window.until || window.since_utc_ms >= window.until_exclusive_utc_ms {
            return Err(format!(
                "{} contains an invalid {range:?} window",
                snapshot.machine_name
            ));
        }
    }
    let mut source_names = HashSet::new();
    for source in &snapshot.sources {
        if source.currency.len() != 3
            || !source
                .currency
                .bytes()
                .all(|byte| byte.is_ascii_uppercase())
        {
            return Err(format!(
                "{} contains an invalid currency",
                snapshot.machine_name
            ));
        }
        if source.source_name != source.source.as_str() {
            return Err(format!(
                "{} contains inconsistent source identity {}",
                snapshot.machine_name, source.source_name
            ));
        }
        if !source_names.insert(source.source.as_str()) {
            return Err(format!(
                "{} contains duplicate source {}",
                snapshot.machine_name, source.source_name
            ));
        }
        for range in rolling_ranges() {
            summary_for(source, range.clone())?;
            let summary = source
                .summaries
                .iter()
                .find(|summary| summary.range == range)
                .ok_or_else(|| format!("{} is missing {range:?}", source.display_name))?;
            let window = window_for(&snapshot.windows, &range)?;
            if summary.since != Some(window.since) || summary.until != Some(window.until) {
                return Err(format!(
                    "{} summary does not match its {range:?} window",
                    source.display_name
                ));
            }
        }
    }
    Ok(())
}

fn write_snapshot(
    connection: &Connection,
    snapshot: &StoredMachineSnapshot,
    only_if_newer: bool,
) -> Result<(), String> {
    let payload = serde_json::to_string(&snapshot.sources)
        .map_err(|error| format!("failed to serialize machine snapshot: {error}"))?;
    let windows = serde_json::to_string(&snapshot.windows)
        .map_err(|error| format!("failed to serialize machine windows: {error}"))?;
    let query = if only_if_newer {
        "INSERT INTO machine_snapshots(machine_id, machine_name, captured_at_ms, windows_json, payload_json)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(machine_id) DO UPDATE SET
           machine_name = excluded.machine_name,
           captured_at_ms = excluded.captured_at_ms,
           windows_json = excluded.windows_json,
           payload_json = excluded.payload_json
         WHERE excluded.captured_at_ms > machine_snapshots.captured_at_ms"
    } else {
        "INSERT INTO machine_snapshots(machine_id, machine_name, captured_at_ms, windows_json, payload_json)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(machine_id) DO UPDATE SET
           machine_name = excluded.machine_name,
           captured_at_ms = excluded.captured_at_ms,
           windows_json = excluded.windows_json,
           payload_json = excluded.payload_json"
    };
    connection
        .execute(
            query,
            params![
                snapshot.machine_id,
                snapshot.machine_name.trim(),
                snapshot.captured_at_ms,
                windows,
                payload
            ],
        )
        .map_err(|error| format!("failed to persist machine snapshot: {error}"))?;
    Ok(())
}

fn read_snapshots(connection: &Connection) -> Result<Vec<StoredMachineSnapshot>, String> {
    let mut statement = connection
        .prepare(
            "SELECT machine_id, machine_name, captured_at_ms, windows_json, payload_json
             FROM machine_snapshots ORDER BY captured_at_ms DESC, machine_name ASC",
        )
        .map_err(|error| format!("failed to prepare machine snapshot query: {error}"))?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        })
        .map_err(|error| format!("failed to query machine snapshots: {error}"))?;
    rows.map(|row| {
        let (machine_id, machine_name, captured_at_ms, windows_json, payload) =
            row.map_err(|error| format!("failed to read machine snapshot: {error}"))?;
        let windows = serde_json::from_str(&windows_json)
            .map_err(|error| format!("stored windows for {machine_name} are malformed: {error}"))?;
        let sources = serde_json::from_str(&payload)
            .map_err(|error| format!("stored snapshot for {machine_name} is malformed: {error}"))?;
        let snapshot = StoredMachineSnapshot {
            machine_id,
            machine_name,
            captured_at_ms,
            windows,
            sources,
        };
        validate_snapshot(&snapshot)?;
        Ok(snapshot)
    })
    .collect()
}

fn add_range(
    totals: &mut MachineUsageTotals,
    tokens: i64,
    cost: Option<f64>,
    range: UsageRange,
) -> Result<(), String> {
    let (token_total, cost_total) = match range {
        UsageRange::Today => (&mut totals.today_tokens, &mut totals.today_cost),
        UsageRange::ThisWeek => (&mut totals.week_tokens, &mut totals.week_cost),
        UsageRange::ThisMonth => (&mut totals.month_tokens, &mut totals.month_cost),
        _ => return Err("machine rollup received an unsupported range".to_string()),
    };
    *token_total = token_total
        .checked_add(tokens)
        .ok_or_else(|| "machine token rollup overflowed".to_string())?;
    if let Some(current) = cost_total {
        match cost {
            Some(value) => {
                *current += value;
                if !current.is_finite() {
                    return Err("machine cost rollup overflowed".to_string());
                }
            }
            None if tokens > 0 => *cost_total = None,
            None => {}
        }
    }
    Ok(())
}

fn project_snapshot(
    snapshot: &StoredMachineSnapshot,
    local_id: &str,
    current_windows: &[CurrentUsageWindow],
) -> Result<MachineUsage, String> {
    let [today_current, week_current, month_current] =
        snapshot_freshness_against(snapshot, current_windows)?;
    let mut totals = MachineUsageTotals {
        today_cost: Some(0.0),
        week_cost: Some(0.0),
        month_cost: Some(0.0),
        ..MachineUsageTotals::default()
    };
    for source in &snapshot.sources {
        for range in [
            UsageRange::Today,
            UsageRange::ThisWeek,
            UsageRange::ThisMonth,
        ] {
            let (tokens, cost) = summary_for(source, range.clone())?;
            add_range(&mut totals, tokens, cost, range)?;
        }
    }
    Ok(MachineUsage {
        machine_id: snapshot.machine_id.clone(),
        machine_name: snapshot.machine_name.clone(),
        captured_at_ms: snapshot.captured_at_ms,
        source_count: snapshot.sources.len(),
        currency: Some("USD".to_string()),
        is_local: snapshot.machine_id == local_id,
        today_current,
        week_current,
        month_current,
        totals,
    })
}

fn machine_rollup(connection: &Connection) -> Result<MachineRollup, String> {
    let local_id = local_machine_id(connection)?;
    let snapshots = read_snapshots(connection)?;
    let current_windows =
        current_usage_windows_with_cli_config().map_err(|error| error.to_string())?;
    let mut machines: Vec<_> = snapshots
        .iter()
        .map(|snapshot| project_snapshot(snapshot, &local_id, &current_windows))
        .collect::<Result<_, _>>()?;
    machines.sort_by(|left, right| {
        right
            .is_local
            .cmp(&left.is_local)
            .then_with(|| left.machine_name.cmp(&right.machine_name))
    });
    let today_current_machines = machines
        .iter()
        .filter(|machine| machine.today_current)
        .count();
    let week_current_machines = machines
        .iter()
        .filter(|machine| machine.week_current)
        .count();
    let month_current_machines = machines
        .iter()
        .filter(|machine| machine.month_current)
        .count();
    let mut totals = MachineUsageTotals {
        today_cost: Some(0.0),
        week_cost: Some(0.0),
        month_cost: Some(0.0),
        ..MachineUsageTotals::default()
    };
    for machine in &machines {
        if machine.today_current {
            add_range(
                &mut totals,
                machine.totals.today_tokens,
                machine.totals.today_cost,
                UsageRange::Today,
            )?;
        }
        if machine.week_current {
            add_range(
                &mut totals,
                machine.totals.week_tokens,
                machine.totals.week_cost,
                UsageRange::ThisWeek,
            )?;
        }
        if machine.month_current {
            add_range(
                &mut totals,
                machine.totals.month_tokens,
                machine.totals.month_cost,
                UsageRange::ThisMonth,
            )?;
        }
    }
    Ok(MachineRollup {
        local_machine_name: machines
            .iter()
            .find(|machine| machine.is_local)
            .map(|machine| machine.machine_name.clone()),
        local_machine_id: local_id,
        currency: Some("USD".to_string()),
        today_current_machines,
        week_current_machines,
        month_current_machines,
        machines,
        totals,
    })
}

pub(crate) fn machine_rollup_at(path: &Path) -> Result<MachineRollup, String> {
    machine_rollup(&open_database(path)?)
}

pub(crate) fn save_local_snapshot_at(
    path: &Path,
    machine_name: &str,
    sources: Vec<MultiCostSummary>,
) -> Result<MachineRollup, String> {
    let mut connection = open_database(path)?;
    let machine_id = local_machine_id(&connection)?;
    let snapshot = StoredMachineSnapshot {
        machine_id,
        machine_name: machine_name.trim().to_string(),
        captured_at_ms: current_time_ms()?,
        windows: current_usage_windows_with_cli_config().map_err(|error| error.to_string())?,
        sources,
    };
    validate_snapshot(&snapshot)?;
    let transaction = connection
        .transaction()
        .map_err(|error| format!("failed to start machine snapshot transaction: {error}"))?;
    write_snapshot(&transaction, &snapshot, false)?;
    let rollup = machine_rollup(&transaction)?;
    transaction
        .commit()
        .map_err(|error| format!("failed to commit machine snapshot: {error}"))?;
    Ok(rollup)
}

pub(crate) fn export_bundle_at(path: &Path) -> Result<String, String> {
    let connection = open_database(path)?;
    local_machine_id(&connection)?;
    serde_json::to_string_pretty(&SnapshotBundle {
        schema_version: SCHEMA_VERSION,
        machines: read_snapshots(&connection)?,
    })
    .map_err(|error| format!("failed to export machine snapshots: {error}"))
}

pub(crate) fn import_bundle_at(path: &Path, content: &str) -> Result<MachineRollup, String> {
    if content.len() > MAX_BUNDLE_BYTES {
        return Err(format!(
            "machine snapshot bundle exceeds {MAX_BUNDLE_BYTES} bytes"
        ));
    }
    let bundle: SnapshotBundle = serde_json::from_str(content)
        .map_err(|error| format!("machine snapshot bundle is malformed: {error}"))?;
    if bundle.schema_version != SCHEMA_VERSION {
        return Err(format!(
            "unsupported machine snapshot bundle version {}",
            bundle.schema_version
        ));
    }
    let mut connection = open_database(path)?;
    let local_id = local_machine_id(&connection)?;
    let mut ids = HashSet::new();
    for snapshot in &bundle.machines {
        if !ids.insert(snapshot.machine_id.as_str()) {
            return Err(format!(
                "bundle contains duplicate machine {}",
                snapshot.machine_id
            ));
        }
        if snapshot.machine_id == local_id {
            continue;
        }
        validate_snapshot(snapshot)?;
    }
    let transaction = connection
        .transaction()
        .map_err(|error| format!("failed to start machine import transaction: {error}"))?;
    for snapshot in bundle
        .machines
        .iter()
        .filter(|snapshot| snapshot.machine_id != local_id)
    {
        write_snapshot(&transaction, snapshot, true)?;
    }
    let rollup = machine_rollup(&transaction)?;
    transaction
        .commit()
        .map_err(|error| format!("failed to commit machine import: {error}"))?;
    Ok(rollup)
}

#[cfg(test)]
#[path = "machines/tests.rs"]
mod tests;
