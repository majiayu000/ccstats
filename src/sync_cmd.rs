//! `ccstats sync push|status` and `--devices` resolution for period reports.

use std::path::PathBuf;

use chrono::Utc;

use crate::cli::{Cli, SourceCommand, SyncCommands};
use crate::config::Config;
use crate::core::DateFilter;
use crate::device_sync::{
    DeviceMerge, DeviceSelection, SYNC_DIR_ENV, build_device_file, default_label, devices_dir,
    load_device_id, load_or_create_device_id, resolve_sync_dir, scan_devices, write_device_file,
};
use crate::source::{ALL_SOURCES, all_sources, get_source, load_daily};
use crate::utils::Timezone;

fn fail(message: &str) -> ! {
    eprintln!("Error: {message}");
    std::process::exit(1);
}

fn require_sync_dir(cli: &Cli, config: &Config) -> PathBuf {
    resolve_sync_dir(cli.sync_dir.as_deref(), config.sync_dir.as_deref()).unwrap_or_else(|| {
        fail(&format!(
            "no sync directory configured; pass --sync-dir, set {SYNC_DIR_ENV}, or add sync_dir to config.toml"
        ))
    })
}

fn own_label(config: &Config, device_id: Option<&str>) -> String {
    config
        .device_label
        .as_deref()
        .map(str::trim)
        .filter(|label| !label.is_empty())
        .map(str::to_string)
        .or_else(|| device_id.map(default_label))
        .unwrap_or_else(|| "this device".to_string())
}

fn utc_offset(timezone: Timezone) -> String {
    timezone.to_fixed_offset(Utc::now()).offset().to_string()
}

pub(crate) fn handle(command: &SyncCommands, cli: &Cli, config: &Config, timezone: Timezone) {
    let sync_dir = require_sync_dir(cli, config);
    match command {
        SyncCommands::Push => push(&sync_dir, cli, config, timezone),
        SyncCommands::Status => status(&sync_dir, config),
    }
}

fn push(sync_dir: &std::path::Path, cli: &Cli, config: &Config, timezone: Timezone) {
    let device_id = load_or_create_device_id().unwrap_or_else(|error| fail(&error));
    let label = own_label(config, Some(&device_id));
    let filter = DateFilter::new(None, None);
    let loaded: Vec<_> = all_sources()
        .map(|source| {
            let result = load_daily(source, &filter, timezone, true, cli.debug);
            (source.name(), result)
        })
        .collect();
    let failures: Vec<_> = loaded
        .iter()
        .filter(|(_, result)| result.parse_errors > 0)
        .map(|(name, result)| format!("{name}: {} parse error(s)", result.parse_errors))
        .collect();
    if !failures.is_empty() {
        fail(&format!(
            "sync push aborted; snapshot was not updated ({}). Fix the source data and retry; use --debug for details",
            failures.join(", ")
        ));
    }
    let file = build_device_file(
        &device_id,
        &label,
        &utc_offset(timezone),
        Utc::now(),
        loaded
            .iter()
            .map(|(name, result)| (*name, &result.day_stats)),
    );
    let path = write_device_file(sync_dir, &file).unwrap_or_else(|error| fail(&error));
    println!(
        "Wrote {} ({} rows, device {label})",
        path.display(),
        file.rows.len()
    );
}

fn status(sync_dir: &std::path::Path, config: &Config) {
    let own_id = load_device_id().unwrap_or_else(|error| fail(&error));
    let label = own_label(config, own_id.as_deref());
    let scan = scan_devices(sync_dir).unwrap_or_else(|error| fail(&error));
    println!("Sync dir: {}", devices_dir(sync_dir).display());
    match own_id.as_deref() {
        Some(id) => println!("This device: {label} ({id})"),
        None => println!("This device: {label} (not pushed yet; run `ccstats sync push`)"),
    }
    if scan.devices.is_empty() {
        println!("No device files found.");
    }
    for file in &scan.devices {
        let marker = if Some(file.device_id.as_str()) == own_id.as_deref() {
            "  (this device)"
        } else {
            ""
        };
        let dates = file.rows.iter().map(|row| row.date.as_str());
        let first = dates.clone().min().unwrap_or("-");
        let last = dates.max().unwrap_or("-");
        println!(
            "- {} [{}] generated {} by ccstats {} (schema {}), {} rows {first}..{last}, UTC{}{marker}",
            file.device_label,
            default_label(&file.device_id),
            file.generated_at
                .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            file.ccstats_version,
            file.schema_version,
            file.rows.len(),
            file.utc_offset,
        );
    }
    for warning in &scan.warnings {
        eprintln!("Warning: {warning}");
    }
}

/// Resolve `--devices` for a report. `None` means this device only (the default).
pub(crate) fn resolve_report_devices(
    cli: &Cli,
    config: &Config,
    source_cmd: SourceCommand,
    source_name: Option<&str>,
    timezone: Timezone,
) -> Option<DeviceMerge> {
    let selection = DeviceSelection::parse(cli.devices.as_deref()?);
    if selection == DeviceSelection::This {
        return None;
    }
    if !matches!(
        source_cmd,
        SourceCommand::Daily
            | SourceCommand::Weekly
            | SourceCommand::Monthly
            | SourceCommand::Today
    ) {
        fail("--devices only supports daily, weekly, monthly, and today");
    }
    let Some(source_name) = source_name else {
        fail("--devices needs a --source when no local source is detected");
    };
    if !source_name.eq_ignore_ascii_case(ALL_SOURCES)
        && get_source(source_name).is_some_and(|source| source.name() == "grok")
    {
        fail("--devices does not support the Grok-only cost report; use --source all");
    }
    if cli.codex_scope != crate::source::CodexScope::All {
        fail(
            "--devices cannot be combined with --codex-scope (synced rows are not split by scope)",
        );
    }

    let sync_dir = require_sync_dir(cli, config);
    let own_id = load_device_id().unwrap_or_else(|error| fail(&error));
    let label = own_label(config, own_id.as_deref());
    let scan = scan_devices(&sync_dir).unwrap_or_else(|error| fail(&error));
    for warning in &scan.warnings {
        eprintln!("Warning: {warning}");
    }
    let merge = DeviceMerge::select(&selection, scan.devices, own_id.as_deref(), &label)
        .unwrap_or_else(|error| fail(&error));

    let offset = utc_offset(timezone);
    let mut names = Vec::new();
    for file in &merge.devices {
        if file.utc_offset != offset {
            eprintln!(
                "Warning: device {} exported days at UTC{}, this report uses UTC{offset}; its rows keep their own day boundaries",
                file.device_label, file.utc_offset
            );
        }
        let unknown: std::collections::BTreeSet<&str> = file
            .rows
            .iter()
            .map(|row| row.source.as_str())
            .filter(|name| get_source(name).is_none_or(|source| source.name() != *name))
            .collect();
        if !unknown.is_empty() {
            eprintln!(
                "Warning: device {} has rows for sources this ccstats does not know ({}); ignored",
                file.device_label,
                unknown.into_iter().collect::<Vec<_>>().join(", ")
            );
        }
        names.push(format!(
            "{} (pushed {})",
            file.device_label,
            file.generated_at
                .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
        ));
    }
    let local = if merge.include_local {
        format!("{label} (live)")
    } else {
        String::new()
    };
    let listed: Vec<String> = std::iter::once(local)
        .filter(|entry| !entry.is_empty())
        .chain(names)
        .collect();
    eprintln!("Devices: {}", listed.join(", "));
    Some(merge)
}
