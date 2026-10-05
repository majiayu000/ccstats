//! Cross-device ledger through a user-chosen sync directory.
//!
//! Each device writes one aggregate file to `<sync_dir>/ccstats/devices/<device_id>.json`.
//! ccstats never talks to a sync service: iCloud Drive, Dropbox, Syncthing, or a NAS
//! moves the files. Rows are `date × source × model` token facts, the same `Stats`
//! the local period reports aggregate, so the reading device prices them with its own
//! pricing table and provenance (`recorded`, `estimated_proxy`, unknown) survives.

use std::collections::hash_map::RandomState;
use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::fs;
use std::hash::{BuildHasher, Hasher};
use std::io::Write as _;
use std::path::{Path, PathBuf};

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::core::{DateFilter, DayStats, Stats};
use crate::utils::paths;

pub(crate) const SCHEMA_VERSION: u64 = 1;
pub(crate) const SYNC_DIR_ENV: &str = "CCSTATS_SYNC_DIR";
const DEVICE_ID_FILE: &str = "device-id";

/// One aggregate row: a day (in the exporting device's timezone), a source, a model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct DeviceRow {
    pub(crate) date: String,
    pub(crate) source: String,
    pub(crate) model: String,
    /// `Stats` skips this field in serde; keep provider-reported totals intact.
    #[serde(default)]
    pub(crate) reported_total_adjustment: i64,
    pub(crate) stats: Stats,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct DeviceFile {
    pub(crate) schema_version: u64,
    pub(crate) ccstats_version: String,
    pub(crate) device_id: String,
    pub(crate) device_label: String,
    pub(crate) generated_at: DateTime<Utc>,
    /// UTC offset that defined the exporting device's day boundaries.
    pub(crate) utc_offset: String,
    pub(crate) rows: Vec<DeviceRow>,
}

/// `--sync-dir` wins over `CCSTATS_SYNC_DIR`, which wins over config `sync_dir`.
pub(crate) fn resolve_sync_dir(flag: Option<&Path>, config: Option<&str>) -> Option<PathBuf> {
    if let Some(path) = flag {
        return Some(path.to_path_buf());
    }
    if let Some(value) = std::env::var_os(SYNC_DIR_ENV).filter(|value| !value.is_empty()) {
        return Some(PathBuf::from(value));
    }
    config
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

pub(crate) fn devices_dir(sync_dir: &Path) -> PathBuf {
    sync_dir.join("ccstats").join("devices")
}

fn device_id_path() -> Option<PathBuf> {
    paths::data_dir().map(|dir| dir.join("ccstats").join(DEVICE_ID_FILE))
}

fn valid_device_id(id: &str) -> bool {
    (8..=64).contains(&id.len())
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

/// Read this device's persisted id without creating one.
pub(crate) fn load_device_id() -> Result<Option<String>, String> {
    let Some(path) = device_id_path() else {
        return Ok(None);
    };
    read_device_id(&path)
}

fn read_device_id(path: &Path) -> Result<Option<String>, String> {
    match fs::read_to_string(path) {
        Ok(content) => {
            let id = content.trim();
            if valid_device_id(id) {
                Ok(Some(id.to_string()))
            } else {
                Err(format!("invalid device id in {}", path.display()))
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("failed to read {}: {error}", path.display())),
    }
}

pub(crate) fn load_or_create_device_id() -> Result<String, String> {
    let path = device_id_path().ok_or("no platform data directory for the device id")?;
    load_or_create_device_id_at(&path)
}

fn load_or_create_device_id_at(path: &Path) -> Result<String, String> {
    if let Some(id) = read_device_id(path)? {
        return Ok(id);
    }
    let id = new_device_id();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create {}: {error}", parent.display()))?;
    }
    fs::write(path, format!("{id}\n"))
        .map_err(|error| format!("failed to write {}: {error}", path.display()))?;
    Ok(id)
}

/// 128 random bits from the std hasher's OS-seeded keys, rendered as 32 hex chars.
fn new_device_id() -> String {
    let nanos = Utc::now().timestamp_nanos_opt().unwrap_or_default();
    let mut id = String::with_capacity(32);
    for salt in 0..2_u8 {
        let mut hasher = RandomState::new().build_hasher();
        hasher.write_i64(nanos);
        hasher.write_u32(std::process::id());
        hasher.write_u8(salt);
        let _ = write!(id, "{:016x}", hasher.finish());
    }
    id
}

pub(crate) fn default_label(device_id: &str) -> String {
    device_id.chars().take(8).collect()
}

/// Absolute paths used as model names (local GGUF files, for example) keep only their basename.
fn export_model_name(model: &str) -> String {
    let trimmed = model.trim();
    let bytes = trimmed.as_bytes();
    let windows_drive = bytes.len() > 2
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/');
    let looks_like_path = trimmed.starts_with(['/', '\\', '~']) || windows_drive;
    if !looks_like_path {
        return trimmed.to_string();
    }
    trimmed
        .rsplit(['/', '\\'])
        .find(|part| !part.is_empty())
        .unwrap_or("unknown")
        .to_string()
}

/// Build this device's export from per-source daily stats.
pub(crate) fn build_device_file<'a>(
    device_id: &str,
    device_label: &str,
    utc_offset: &str,
    generated_at: DateTime<Utc>,
    sources: impl IntoIterator<Item = (&'a str, &'a HashMap<String, DayStats>)>,
) -> DeviceFile {
    let mut merged: BTreeMap<(String, String, String), Stats> = BTreeMap::new();
    for (source, days) in sources {
        for (date, day) in days {
            for (model, stats) in &day.models {
                merged
                    .entry((date.clone(), source.to_string(), export_model_name(model)))
                    .or_default()
                    .add(stats);
            }
        }
    }
    let rows = merged
        .into_iter()
        .map(|((date, source, model), stats)| DeviceRow {
            date,
            source,
            model,
            reported_total_adjustment: stats.reported_total_adjustment,
            stats,
        })
        .collect();
    DeviceFile {
        schema_version: SCHEMA_VERSION,
        ccstats_version: crate::VERSION.to_string(),
        device_id: device_id.to_string(),
        device_label: device_label.to_string(),
        generated_at,
        utc_offset: utc_offset.to_string(),
        rows,
    }
}

/// Write via a hidden temp file plus rename so sync clients never pick up half a file.
pub(crate) fn write_device_file(sync_dir: &Path, file: &DeviceFile) -> Result<PathBuf, String> {
    if !sync_dir.is_dir() {
        return Err(format!(
            "sync dir {} is not an existing directory",
            sync_dir.display()
        ));
    }
    let dir = devices_dir(sync_dir);
    fs::create_dir_all(&dir)
        .map_err(|error| format!("failed to create {}: {error}", dir.display()))?;
    let target = dir.join(format!("{}.json", file.device_id));
    let temp = dir.join(format!(
        ".{}.json.{}.tmp",
        file.device_id,
        std::process::id()
    ));
    let json = serde_json::to_vec_pretty(file)
        .map_err(|error| format!("failed to serialize device file: {error}"))?;
    let written = fs::File::create(&temp).and_then(|mut handle| {
        handle.write_all(&json)?;
        handle.sync_all()
    });
    if let Err(error) = written.and_then(|()| fs::rename(&temp, &target)) {
        let _ = fs::remove_file(&temp);
        return Err(format!("failed to write {}: {error}", target.display()));
    }
    Ok(target)
}

#[derive(Debug, Default)]
pub(crate) struct SyncScan {
    pub(crate) devices: Vec<DeviceFile>,
    pub(crate) warnings: Vec<String>,
}

/// Read every device file. Broken, partial, or foreign-version files become warnings.
pub(crate) fn scan_devices(sync_dir: &Path) -> Result<SyncScan, String> {
    if !sync_dir.is_dir() {
        return Err(format!(
            "sync dir {} is not an existing directory",
            sync_dir.display()
        ));
    }
    let dir = devices_dir(sync_dir);
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(SyncScan::default());
        }
        Err(error) => return Err(format!("failed to read {}: {error}", dir.display())),
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            !name.starts_with('.')
                && path
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
                && path.is_file()
        })
        .collect();
    paths.sort();

    let mut scan = SyncScan::default();
    let mut by_id: BTreeMap<String, DeviceFile> = BTreeMap::new();
    for path in paths {
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let file = match parse_device_file(&path) {
            Ok(file) => file,
            Err(reason) => {
                scan.warnings.push(format!("skipping {name}: {reason}"));
                continue;
            }
        };
        match by_id.get(&file.device_id) {
            Some(existing) if existing.generated_at >= file.generated_at => {
                scan.warnings.push(format!(
                    "skipping {name}: older duplicate of device {}",
                    file.device_label
                ));
            }
            Some(existing) => {
                scan.warnings.push(format!(
                    "using {name} over an older duplicate of device {}",
                    existing.device_label
                ));
                by_id.insert(file.device_id.clone(), file);
            }
            None => {
                by_id.insert(file.device_id.clone(), file);
            }
        }
    }
    scan.devices = by_id.into_values().collect();
    scan.devices
        .sort_by(|a, b| a.device_label.cmp(&b.device_label));
    Ok(scan)
}

fn parse_device_file(path: &Path) -> Result<DeviceFile, String> {
    let bytes = fs::read(path).map_err(|error| format!("unreadable ({error})"))?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|error| format!("not valid JSON, possibly still syncing ({error})"))?;
    match value.get("schema_version").and_then(Value::as_u64) {
        Some(SCHEMA_VERSION) => {}
        Some(other) => {
            return Err(format!(
                "schema version {other} is not supported by this ccstats (expects {SCHEMA_VERSION})"
            ));
        }
        None => return Err("missing schema_version".to_string()),
    }
    let file: DeviceFile =
        serde_json::from_value(value).map_err(|error| format!("invalid device file ({error})"))?;
    if !valid_device_id(&file.device_id) {
        return Err("invalid device_id".to_string());
    }
    if let Some(row) = file
        .rows
        .iter()
        .find(|row| NaiveDate::parse_from_str(&row.date, "%Y-%m-%d").is_err())
    {
        return Err(format!("invalid row date '{}'", row.date));
    }
    Ok(file)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DeviceSelection {
    This,
    All,
    Label(String),
}

impl DeviceSelection {
    pub(crate) fn parse(value: &str) -> Self {
        let trimmed = value.trim();
        if trimmed.eq_ignore_ascii_case("this") {
            Self::This
        } else if trimmed.eq_ignore_ascii_case("all") {
            Self::All
        } else {
            Self::Label(trimmed.to_string())
        }
    }
}

/// Which data a report shows: optionally local data, plus other devices' rows.
#[derive(Debug, Default)]
pub(crate) struct DeviceMerge {
    pub(crate) include_local: bool,
    pub(crate) devices: Vec<DeviceFile>,
}

impl DeviceMerge {
    /// Resolve `--devices`. This device's own file is never used: live local data replaces it.
    pub(crate) fn select(
        selection: &DeviceSelection,
        scanned: Vec<DeviceFile>,
        own_id: Option<&str>,
        own_label: &str,
    ) -> Result<Self, String> {
        let others = scanned
            .into_iter()
            .filter(|file| Some(file.device_id.as_str()) != own_id);
        match selection {
            DeviceSelection::This => Ok(Self {
                include_local: true,
                devices: Vec::new(),
            }),
            DeviceSelection::All => Ok(Self {
                include_local: true,
                devices: others.collect(),
            }),
            DeviceSelection::Label(label) => {
                let is_own = label.eq_ignore_ascii_case(own_label)
                    || own_id.is_some_and(|id| label.len() >= 4 && id.starts_with(label));
                if is_own {
                    return Ok(Self {
                        include_local: true,
                        devices: Vec::new(),
                    });
                }
                let others: Vec<DeviceFile> = others.collect();
                let known: Vec<String> = others
                    .iter()
                    .map(|file| file.device_label.clone())
                    .collect();
                let devices: Vec<DeviceFile> = others
                    .into_iter()
                    .filter(|file| {
                        file.device_label.eq_ignore_ascii_case(label)
                            || (label.len() >= 4 && file.device_id.starts_with(label.as_str()))
                    })
                    .collect();
                if devices.is_empty() {
                    let known = if known.is_empty() {
                        "none".to_string()
                    } else {
                        known.join(", ")
                    };
                    return Err(format!(
                        "no device matches --devices '{label}' (this device: {own_label}; other devices: {known})"
                    ));
                }
                Ok(Self {
                    include_local: false,
                    devices,
                })
            }
        }
    }

    /// Other devices' daily stats for one registered source within the date filter.
    pub(crate) fn day_stats_for(
        &self,
        source: &str,
        filter: &DateFilter,
    ) -> HashMap<String, DayStats> {
        let mut days: HashMap<String, DayStats> = HashMap::new();
        for row in self.devices.iter().flat_map(|file| &file.rows) {
            if row.source != source {
                continue;
            }
            let Ok(date) = NaiveDate::parse_from_str(&row.date, "%Y-%m-%d") else {
                continue;
            };
            if !filter.contains(date) {
                continue;
            }
            let mut stats = row.stats.clone();
            stats.reported_total_adjustment = row.reported_total_adjustment;
            days.entry(row.date.clone())
                .or_default()
                .add_stats(row.model.clone(), &stats);
        }
        days
    }
}

#[cfg(test)]
mod tests;
