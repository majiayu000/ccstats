use std::fs;

use ccstats::{ApiEquivalentCostCoverage, CostSummary, TokenBreakdown, UsageSource};

use super::*;

fn database(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "ccstats-machines-{name}-{}-{}.sqlite3",
        std::process::id(),
        current_time_ms().expect("timestamp")
    ))
}

fn sample_source(today: i64, week: i64, month: i64) -> MultiCostSummary {
    let windows = current_usage_windows_with_cli_config().expect("configured usage windows");
    let summary = |range: UsageRange, tokens, cost| CostSummary {
        source: UsageSource::Claude,
        source_name: "claude".to_string(),
        display_name: "Claude Code".to_string(),
        range: range.clone(),
        since: Some(window_for(&windows, &range).expect("range bounds").since),
        until: Some(window_for(&windows, &range).expect("range bounds").until),
        currency: "USD".to_string(),
        cost: Some(cost),
        cost_usd: Some(cost),
        estimated_cost: None,
        estimated_cost_usd: None,
        cost_kind: "real".to_string(),
        pricing_source: "recorded".to_string(),
        api_equivalent_cost_coverage: None,
        grok_api_equivalent_cost: None,
        tokens: TokenBreakdown {
            input_tokens: tokens,
            total_tokens: tokens,
            reported_total_adjustment: 0,
            ..TokenBreakdown::default()
        },
        models: Vec::new(),
        valid_entries: 1,
        skipped_entries: 0,
        parse_error_entries: 0,
        elapsed_ms: 1.0,
    };
    MultiCostSummary {
        source: UsageSource::Claude,
        source_name: "claude".to_string(),
        display_name: "Claude Code".to_string(),
        currency: "USD".to_string(),
        generated_at: "2026-09-02T00:00:00Z".to_string(),
        summaries: vec![
            summary(UsageRange::Today, today, 1.0),
            summary(UsageRange::ThisWeek, week, 2.0),
            summary(UsageRange::ThisMonth, month, 3.0),
        ],
        elapsed_ms: 3.0,
    }
}

#[test]
fn local_snapshot_persists_across_reopens() {
    let path = database("persist");
    let saved = save_local_snapshot_at(&path, "Studio", vec![sample_source(100, 400, 900)])
        .expect("save snapshot");
    let reopened = machine_rollup_at(&path).expect("reopen rollup");

    assert_eq!(saved.local_machine_id, reopened.local_machine_id);
    assert_eq!(reopened.local_machine_name.as_deref(), Some("Studio"));
    assert_eq!(reopened.totals.month_tokens, 900);
    fs::remove_file(path).expect("remove test database");
}

#[test]
fn exported_snapshots_include_absolute_usage_windows() {
    let path = database("absolute-windows");
    save_local_snapshot_at(&path, "Studio", vec![sample_source(100, 400, 900)])
        .expect("save snapshot");

    let bundle: serde_json::Value =
        serde_json::from_str(&export_bundle_at(&path).expect("export snapshot bundle"))
            .expect("parse snapshot bundle");
    let windows = bundle["machines"][0]["windows"]
        .as_array()
        .expect("snapshot must include usage windows");

    assert_eq!(windows.len(), 3);
    assert!(windows[0]["since_utc_ms"].is_i64());
    assert!(windows[0]["until_exclusive_utc_ms"].is_i64());
    fs::remove_file(path).expect("remove test database");
}

#[test]
fn imported_older_snapshot_does_not_replace_newer_data() {
    let origin = database("origin");
    let target = database("target");
    save_local_snapshot_at(&origin, "Laptop", vec![sample_source(100, 400, 900)])
        .expect("save origin");
    let current = export_bundle_at(&origin).expect("export origin");
    import_bundle_at(&target, &current).expect("import origin");

    let mut older: SnapshotBundle = serde_json::from_str(&current).expect("parse bundle");
    older.machines[0].machine_name = "Stale laptop".to_string();
    older.machines[0].captured_at_ms -= 1;
    older.machines[0].sources = vec![sample_source(999, 999, 999)];
    import_bundle_at(
        &target,
        &serde_json::to_string(&older).expect("serialize older"),
    )
    .expect("import older");

    let rollup = machine_rollup_at(&target).expect("target rollup");
    assert_eq!(rollup.machines[0].machine_name, "Laptop");
    assert_eq!(rollup.totals.month_tokens, 900);
    fs::remove_file(origin).expect("remove origin");
    fs::remove_file(target).expect("remove target");
}

#[test]
fn import_rejects_invalid_currency() {
    let path = database("currency");
    let mut source = sample_source(1, 2, 3);
    source.currency = "INVALID".to_string();
    let bundle = SnapshotBundle {
        schema_version: SCHEMA_VERSION,
        machines: vec![StoredMachineSnapshot {
            machine_id: "remote".to_string(),
            machine_name: "Remote".to_string(),
            captured_at_ms: 1,
            windows: current_usage_windows_with_cli_config().expect("usage windows"),
            sources: vec![source],
        }],
    };
    let error = import_bundle_at(&path, &serde_json::to_string(&bundle).expect("bundle"))
        .expect_err("invalid currency must fail");

    assert!(error.contains("invalid currency"));
    fs::remove_file(path).expect("remove test database");
}

#[test]
fn freshness_and_rollup_are_independent_for_each_range() {
    let path = database("partial-freshness");
    save_local_snapshot_at(&path, "Local", vec![sample_source(100, 400, 900)])
        .expect("save current USD snapshot");
    let mut source = sample_source(100, 400, 900);
    source.currency = "EUR".to_string();
    source
        .summaries
        .iter_mut()
        .for_each(|row| row.currency = "EUR".to_string());
    let mut windows = current_usage_windows_with_cli_config().expect("usage windows");
    windows[0].since = windows[0].since.pred_opt().expect("previous day");
    windows[0].until = windows[0].until.pred_opt().expect("previous day");
    windows[0].since_utc_ms -= 24 * 60 * 60 * 1_000;
    windows[0].until_exclusive_utc_ms -= 24 * 60 * 60 * 1_000;
    source.summaries[0].since = Some(windows[0].since);
    source.summaries[0].until = Some(windows[0].until);
    let bundle = SnapshotBundle {
        schema_version: SCHEMA_VERSION,
        machines: vec![StoredMachineSnapshot {
            machine_id: "remote".to_string(),
            machine_name: "Remote".to_string(),
            captured_at_ms: 1,
            windows,
            sources: vec![source],
        }],
    };

    let rollup = import_bundle_at(&path, &serde_json::to_string(&bundle).expect("bundle"))
        .expect("import partially fresh snapshot");

    assert_eq!(rollup.today_current_machines, 1);
    assert_eq!(rollup.week_current_machines, 2);
    assert_eq!(rollup.totals.today_cost, Some(1.0));
    assert_eq!(rollup.totals.week_cost, Some(4.0));
    fs::remove_file(path).expect("remove test database");
}

#[test]
fn equal_date_labels_with_different_utc_boundaries_are_not_current() {
    let current = current_usage_windows_with_cli_config().expect("usage windows");
    let mut shifted = current.clone();
    for window in &mut shifted {
        window.since_utc_ms -= 8 * 60 * 60 * 1_000;
        window.until_exclusive_utc_ms -= 8 * 60 * 60 * 1_000;
    }
    let snapshot = StoredMachineSnapshot {
        machine_id: "remote".to_string(),
        machine_name: "Remote".to_string(),
        captured_at_ms: 1,
        windows: shifted,
        sources: vec![sample_source(100, 400, 900)],
    };

    validate_snapshot(&snapshot).expect("same date labels are internally valid");
    assert_eq!(
        snapshot_freshness_against(&snapshot, &current).expect("freshness"),
        [false, false, false]
    );
}

#[test]
fn bundles_round_trip_without_replacing_the_local_machine() {
    let studio = database("round-trip-studio");
    let laptop = database("round-trip-laptop");
    save_local_snapshot_at(&studio, "Studio", vec![sample_source(100, 400, 900)])
        .expect("save studio snapshot");
    save_local_snapshot_at(&laptop, "Laptop", vec![sample_source(50, 200, 600)])
        .expect("save laptop snapshot");

    let studio_bundle = export_bundle_at(&studio).expect("export studio");
    import_bundle_at(&laptop, &studio_bundle).expect("import studio on laptop");
    let laptop_bundle = export_bundle_at(&laptop).expect("export laptop and studio");
    let rollup = import_bundle_at(&studio, &laptop_bundle).expect("round-trip to studio");

    assert_eq!(rollup.machines.len(), 2);
    assert_eq!(rollup.local_machine_name.as_deref(), Some("Studio"));
    assert_eq!(rollup.totals.month_tokens, 1_500);
    fs::remove_file(studio).expect("remove studio database");
    fs::remove_file(laptop).expect("remove laptop database");
}

#[test]
fn lower_bound_cost_is_not_presented_as_a_complete_machine_total() {
    let path = database("lower-bound");
    let mut source = sample_source(100, 400, 900);
    source.summaries[2].api_equivalent_cost_coverage = Some(ApiEquivalentCostCoverage {
        total_tokens: 900,
        priced_tokens: 800,
        percent: 800.0 / 900.0 * 100.0,
        complete: false,
        cost_is_lower_bound: true,
    });

    let rollup = save_local_snapshot_at(&path, "Studio", vec![source]).expect("save snapshot");

    assert_eq!(rollup.totals.month_cost, None);
    fs::remove_file(path).expect("remove test database");
}
