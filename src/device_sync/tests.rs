use std::collections::HashMap;

use chrono::{TimeZone, Utc};

use super::*;

fn stats(input: i64, output: i64) -> Stats {
    Stats {
        input_tokens: input,
        output_tokens: output,
        count: 1,
        records: 1,
        priced_tokens: crate::core::CostTokens {
            input_tokens: input,
            output_tokens: output,
            count: 1,
            ..Default::default()
        },
        ..Default::default()
    }
}

fn days(date: &str, model: &str, input: i64, output: i64) -> HashMap<String, DayStats> {
    let mut day = DayStats::default();
    day.add_stats(model.to_string(), &stats(input, output));
    HashMap::from([(date.to_string(), day)])
}

fn device(id: &str, label: &str, input: i64) -> DeviceFile {
    let claude = days("2026-10-01", "sonnet-4", input, 10);
    build_device_file(
        id,
        label,
        "+00:00",
        Utc.with_ymd_and_hms(2026, 10, 2, 0, 0, 0).unwrap(),
        [("claude", &claude)],
    )
}

fn temp_sync_dir() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

#[test]
fn round_trip_two_devices_merges_other_device_and_skips_self() {
    let sync = temp_sync_dir();
    let laptop = device("aaaaaaaa11112222", "laptop", 100);
    let desktop = device("bbbbbbbb33334444", "desktop", 200);
    write_device_file(sync.path(), &laptop).unwrap();
    write_device_file(sync.path(), &desktop).unwrap();

    let scan = scan_devices(sync.path()).unwrap();
    assert!(scan.warnings.is_empty(), "{:?}", scan.warnings);
    assert_eq!(scan.devices.len(), 2);

    // Reading as the laptop: its own file is never merged (live data replaces it).
    let merge = DeviceMerge::select(
        &DeviceSelection::All,
        scan.devices,
        Some("aaaaaaaa11112222"),
        "laptop",
    )
    .unwrap();
    assert!(merge.include_local);
    assert_eq!(merge.devices.len(), 1);
    assert_eq!(merge.devices[0].device_label, "desktop");

    let merged = merge.day_stats_for("claude", &DateFilter::new(None, None));
    let day = &merged["2026-10-01"];
    assert_eq!(day.stats.input_tokens, 200);
    assert_eq!(day.models["sonnet-4"].priced_tokens.input_tokens, 200);
    assert!(
        merge
            .day_stats_for("codex", &DateFilter::new(None, None))
            .is_empty()
    );
    let outside = DateFilter::new(NaiveDate::from_ymd_opt(2026, 10, 2), None);
    assert!(merge.day_stats_for("claude", &outside).is_empty());
}

#[test]
fn label_selection_excludes_local_and_unknown_label_errors() {
    let files = vec![
        device("aaaaaaaa11112222", "laptop", 1),
        device("bbbbbbbb33334444", "desktop", 2),
    ];
    let merge = DeviceMerge::select(
        &DeviceSelection::parse("Desktop"),
        files.clone(),
        Some("aaaaaaaa11112222"),
        "laptop",
    )
    .unwrap();
    assert!(!merge.include_local);
    assert_eq!(merge.devices.len(), 1);

    let own = DeviceMerge::select(
        &DeviceSelection::parse("laptop"),
        files.clone(),
        Some("aaaaaaaa11112222"),
        "laptop",
    )
    .unwrap();
    assert!(own.include_local);
    assert!(own.devices.is_empty());

    let error = DeviceMerge::select(
        &DeviceSelection::parse("server"),
        files,
        Some("aaaaaaaa11112222"),
        "laptop",
    )
    .unwrap_err();
    assert!(error.contains("desktop"), "{error}");
}

#[test]
fn corrupted_partial_and_unknown_version_files_are_skipped_with_warnings() {
    let sync = temp_sync_dir();
    write_device_file(sync.path(), &device("aaaaaaaa11112222", "laptop", 1)).unwrap();
    let dir = devices_dir(sync.path());
    fs::write(
        dir.join("partial.json"),
        "{\"schema_version\": 1, \"rows\": [",
    )
    .unwrap();
    fs::write(
        dir.join("future.json"),
        serde_json::json!({"schema_version": 99, "device_id": "cccccccc55556666"}).to_string(),
    )
    .unwrap();
    fs::write(dir.join("noversion.json"), "{}").unwrap();
    let mut bad_date = device("dddddddd77778888", "bad", 1);
    bad_date.rows[0].date = "yesterday".to_string();
    fs::write(
        dir.join("baddate.json"),
        serde_json::to_string(&bad_date).unwrap(),
    )
    .unwrap();
    // Temp files from an interrupted write are not device files.
    fs::write(dir.join(".eeeeeeee.json.1.tmp"), "{").unwrap();

    let scan = scan_devices(sync.path()).unwrap();
    assert_eq!(scan.devices.len(), 1);
    assert_eq!(scan.devices[0].device_label, "laptop");
    assert_eq!(scan.warnings.len(), 4, "{:?}", scan.warnings);
    assert!(scan.warnings.iter().any(|w| w.contains("partial.json")));
    assert!(
        scan.warnings
            .iter()
            .any(|w| w.contains("future.json") && w.contains("schema version 99"))
    );
    assert!(scan.warnings.iter().any(|w| w.contains("noversion.json")));
    assert!(scan.warnings.iter().any(|w| w.contains("baddate.json")));
}

#[test]
fn duplicate_device_files_keep_newest() {
    let sync = temp_sync_dir();
    let old = device("aaaaaaaa11112222", "laptop", 1);
    let mut new = old.clone();
    new.generated_at = Utc.with_ymd_and_hms(2026, 10, 3, 0, 0, 0).unwrap();
    new.rows[0].stats.input_tokens = 5;
    write_device_file(sync.path(), &old).unwrap();
    fs::write(
        devices_dir(sync.path()).join("aaaaaaaa11112222 (conflicted copy).json"),
        serde_json::to_string(&new).unwrap(),
    )
    .unwrap();

    let scan = scan_devices(sync.path()).unwrap();
    assert_eq!(scan.devices.len(), 1);
    assert_eq!(scan.devices[0].rows[0].stats.input_tokens, 5);
    assert_eq!(scan.warnings.len(), 1);
}

#[test]
fn recorded_and_estimated_provenance_survives_round_trip() {
    let sync = temp_sync_dir();
    let mut day = DayStats::default();
    let mut grok = stats(10, 5);
    grok.recorded_cost_usd = 0.25;
    grok.recorded_cost_entries = 1;
    grok.estimated_proxy = crate::core::CostTokens {
        input_tokens: 10,
        count: 1,
        ..Default::default()
    };
    day.add_stats("grok-4".to_string(), &grok);
    let days = HashMap::from([("2026-10-01".to_string(), day)]);
    let file = build_device_file(
        "ffffffff00001111",
        "desktop",
        "+00:00",
        Utc::now(),
        [("grok", &days)],
    );
    write_device_file(sync.path(), &file).unwrap();

    let scan = scan_devices(sync.path()).unwrap();
    let merge = DeviceMerge::select(&DeviceSelection::All, scan.devices, None, "laptop").unwrap();
    let merged = merge.day_stats_for("grok", &DateFilter::new(None, None));
    let model = &merged["2026-10-01"].models["grok-4"];
    assert!((model.recorded_cost_usd - 0.25).abs() < f64::EPSILON);
    assert_eq!(model.recorded_cost_entries, 1);
    assert_eq!(model.estimated_proxy.input_tokens, 10);
    assert_eq!(model.cost_kind(), crate::core::CostKind::EstimatedProxy);
}

#[test]
fn model_paths_are_reduced_to_basename() {
    assert_eq!(
        export_model_name("/Users/alice/models/qwen3.gguf"),
        "qwen3.gguf"
    );
    assert_eq!(export_model_name(r"C:\Users\bob\m.gguf"), "m.gguf");
    assert_eq!(export_model_name("~/m/x.gguf"), "x.gguf");
    assert_eq!(
        export_model_name("anthropic/claude-sonnet-4"),
        "anthropic/claude-sonnet-4"
    );
}

#[test]
fn device_id_is_created_once_and_reused() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ccstats").join(DEVICE_ID_FILE);
    let first = load_or_create_device_id_at(&path).unwrap();
    assert_eq!(first.len(), 32);
    assert!(valid_device_id(&first));
    assert_eq!(load_or_create_device_id_at(&path).unwrap(), first);
    assert_ne!(new_device_id(), new_device_id());
}

#[test]
fn write_requires_existing_sync_dir_and_leaves_no_temp_file() {
    let missing = std::env::temp_dir().join("ccstats-no-such-sync-dir-for-test");
    assert!(write_device_file(&missing, &device("aaaaaaaa11112222", "l", 1)).is_err());

    let sync = temp_sync_dir();
    let path = write_device_file(sync.path(), &device("aaaaaaaa11112222", "l", 1)).unwrap();
    assert!(path.ends_with("ccstats/devices/aaaaaaaa11112222.json"));
    let names: Vec<_> = fs::read_dir(devices_dir(sync.path()))
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(names.len(), 1);
}
