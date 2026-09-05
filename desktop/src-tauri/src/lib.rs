use std::collections::HashMap;
use std::path::Path;
use std::str::FromStr;

use ccstats::{
    CodexWeeklyQuota, CodexWeeklyValueEstimate, CodexWeeklyValueWindow, MultiCostSummary,
    MultiSummaryOptions, ProjectDrilldownSummary, SessionTitle, SourceDescriptor,
    SourceDiagnosticDescriptor, SummaryOptions, TurnToolBreakdown, UsageHistory, UsageRange,
    UsageSource, diagnose_usage_sources, estimate_codex_weekly_value_for_window,
    list_usage_sources, load_codex_weekly_quota, load_session_titles,
    summarize_cost_ranges_with_cli_config, summarize_project_drilldown_with_cli_config,
    turn_tool_breakdown_with_cli_config, usage_history_with_cli_config,
};
use serde::Serialize;
use tauri::Manager;

mod machines;

use machines::{
    MachineRollup, export_bundle_at, import_bundle_at, machine_rollup_at, save_local_snapshot_at,
};

#[derive(Debug, Serialize)]
struct CodexQuotaOverview {
    quota: CodexWeeklyQuota,
    value_estimate: Option<CodexWeeklyValueEstimate>,
    value_estimate_error: Option<String>,
}

#[tauri::command]
fn list_sources() -> Result<Vec<SourceDescriptor>, String> {
    list_usage_sources().map_err(|error| error.to_string())
}

#[tauri::command]
async fn source_diagnostics() -> Result<Vec<SourceDiagnosticDescriptor>, String> {
    tauri::async_runtime::spawn_blocking(|| {
        diagnose_usage_sources().map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("source diagnostics task failed: {error}"))?
}

fn load_codex_quota_overview(codex_home: Option<&Path>) -> Result<CodexQuotaOverview, String> {
    let quota = load_codex_weekly_quota(codex_home).map_err(|error| error.to_string())?;
    let window = CodexWeeklyValueWindow {
        observed_at: quota.observed_at,
        resets_at: quota.resets_at,
        window_minutes: quota.window_minutes,
        used_pct: quota.used_pct,
    };
    let (value_estimate, value_estimate_error) =
        match estimate_codex_weekly_value_for_window(&window, codex_home, true, false) {
            Ok(estimate) => (Some(estimate), None),
            Err(error) => (None, Some(error.to_string())),
        };
    Ok(CodexQuotaOverview {
        quota,
        value_estimate,
        value_estimate_error,
    })
}

#[tauri::command]
async fn codex_quota_overview() -> Result<CodexQuotaOverview, String> {
    tauri::async_runtime::spawn_blocking(|| load_codex_quota_overview(None))
        .await
        .map_err(|error| format!("quota scan task failed: {error}"))?
}

fn summarize_source(source: &str) -> Result<MultiCostSummary, String> {
    let source = UsageSource::from_str(source).map_err(|error| error.to_string())?;
    summarize_cost_ranges_with_cli_config(MultiSummaryOptions {
        source,
        ranges: vec![
            UsageRange::Today,
            UsageRange::ThisWeek,
            UsageRange::ThisMonth,
        ],
        ..MultiSummaryOptions::default()
    })
    .map_err(|error| error.to_string())
}

#[tauri::command]
async fn usage_overview(source: String) -> Result<MultiCostSummary, String> {
    tauri::async_runtime::spawn_blocking(move || summarize_source(&source))
        .await
        .map_err(|error| format!("usage scan task failed: {error}"))?
}

#[tauri::command]
async fn usage_overviews(sources: Vec<String>) -> Result<Vec<MultiCostSummary>, String> {
    if sources.is_empty() {
        return Err("at least one usage source is required".to_string());
    }
    tauri::async_runtime::spawn_blocking(move || {
        sources
            .iter()
            .map(|source| summarize_source(source))
            .collect()
    })
    .await
    .map_err(|error| format!("usage scan task failed: {error}"))?
}

fn parse_range(range: &str) -> Result<UsageRange, String> {
    match range {
        "today" => Ok(UsageRange::Today),
        "this_week" => Ok(UsageRange::ThisWeek),
        "this_month" => Ok(UsageRange::ThisMonth),
        _ => Err(format!("invalid usage range '{range}'")),
    }
}

fn summary_options(source: &str, range: &str) -> Result<SummaryOptions, String> {
    Ok(SummaryOptions {
        source: UsageSource::from_str(source).map_err(|error| error.to_string())?,
        range: parse_range(range)?,
        ..SummaryOptions::default()
    })
}

#[derive(Debug, Serialize)]
struct DesktopProjectDrilldown {
    #[serde(flatten)]
    usage: ProjectDrilldownSummary,
    session_titles: HashMap<String, SessionTitle>,
    session_titles_error: Option<String>,
}

#[tauri::command]
async fn project_drilldown(
    source: String,
    range: String,
) -> Result<DesktopProjectDrilldown, String> {
    let options = summary_options(&source, &range)?;
    tauri::async_runtime::spawn_blocking(move || {
        let usage = summarize_project_drilldown_with_cli_config(options)
            .map_err(|error| error.to_string())?;
        let ids = usage
            .projects
            .iter()
            .flat_map(|project| &project.sessions)
            .map(|session| session.session_id.clone())
            .collect::<Vec<_>>();
        let (session_titles, session_titles_error) = match load_session_titles(usage.source, &ids) {
            Ok(titles) => (titles, None),
            Err(error) => (HashMap::new(), Some(error.to_string())),
        };
        Ok(DesktopProjectDrilldown {
            usage,
            session_titles,
            session_titles_error,
        })
    })
    .await
    .map_err(|error| format!("project scan task failed: {error}"))?
}

#[tauri::command]
async fn usage_history(source: String, range: String) -> Result<UsageHistory, String> {
    let options = summary_options(&source, &range)?;
    tauri::async_runtime::spawn_blocking(move || {
        usage_history_with_cli_config(options).map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("history scan task failed: {error}"))?
}

#[tauri::command]
async fn turn_tool_breakdown(source: String, range: String) -> Result<TurnToolBreakdown, String> {
    let options = summary_options(&source, &range)?;
    tauri::async_runtime::spawn_blocking(move || {
        turn_tool_breakdown_with_cli_config(options).map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("turn and tool scan task failed: {error}"))?
}

#[tauri::command]
async fn export_history(source: String, range: String, format: String) -> Result<String, String> {
    let options = summary_options(&source, &range)?;
    tauri::async_runtime::spawn_blocking(move || {
        let history = usage_history_with_cli_config(options).map_err(|error| error.to_string())?;
        match format.as_str() {
            "csv" => history.to_csv().map_err(|error| error.to_string()),
            "json" => history.to_json().map_err(|error| error.to_string()),
            _ => Err(format!("unsupported history export format '{format}'")),
        }
    })
    .await
    .map_err(|error| format!("history export task failed: {error}"))?
}

fn machine_database_path(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|directory| directory.join("machine-snapshots.sqlite3"))
        .map_err(|error| format!("failed to resolve app data directory: {error}"))
}

#[tauri::command]
async fn machine_rollup(app: tauri::AppHandle) -> Result<MachineRollup, String> {
    let path = machine_database_path(&app)?;
    tauri::async_runtime::spawn_blocking(move || machine_rollup_at(&path))
        .await
        .map_err(|error| format!("machine rollup task failed: {error}"))?
}

#[tauri::command]
async fn save_machine_snapshot(
    app: tauri::AppHandle,
    machine_name: String,
    sources: Vec<MultiCostSummary>,
) -> Result<MachineRollup, String> {
    let path = machine_database_path(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        save_local_snapshot_at(&path, &machine_name, sources)
    })
    .await
    .map_err(|error| format!("machine snapshot task failed: {error}"))?
}

#[tauri::command]
async fn export_machine_bundle(app: tauri::AppHandle) -> Result<String, String> {
    let path = machine_database_path(&app)?;
    tauri::async_runtime::spawn_blocking(move || export_bundle_at(&path))
        .await
        .map_err(|error| format!("machine export task failed: {error}"))?
}

#[tauri::command]
async fn import_machine_bundle(
    app: tauri::AppHandle,
    content: String,
) -> Result<MachineRollup, String> {
    let path = machine_database_path(&app)?;
    tauri::async_runtime::spawn_blocking(move || import_bundle_at(&path, &content))
        .await
        .map_err(|error| format!("machine import task failed: {error}"))?
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default();
    #[cfg(all(feature = "native-e2e", debug_assertions))]
    let builder = builder.plugin(tauri_plugin_wdio_webdriver::init());

    builder
        .invoke_handler(tauri::generate_handler![
            list_sources,
            source_diagnostics,
            codex_quota_overview,
            usage_overview,
            usage_overviews,
            project_drilldown,
            usage_history,
            turn_tool_breakdown,
            export_history,
            machine_rollup,
            save_machine_snapshot,
            export_machine_bundle,
            import_machine_bundle
        ])
        .run(tauri::generate_context!())
        .expect("failed to run ccstats desktop");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_command_projects_the_real_registry() {
        let sources = list_sources().expect("source catalog command");

        assert_eq!(sources.len(), 29);
        assert_eq!(
            sources.first().map(|source| source.name.as_str()),
            Some("claude")
        );
        assert_eq!(
            sources.last().map(|source| source.name.as_str()),
            Some("dsh")
        );
    }

    #[test]
    fn diagnostics_command_projects_the_real_registry() {
        let diagnostics = tauri::async_runtime::block_on(source_diagnostics())
            .expect("source diagnostics command");

        assert_eq!(diagnostics.len(), 29);
        assert_eq!(
            diagnostics.first().map(|row| row.name.as_str()),
            Some("claude")
        );
    }

    #[test]
    fn quota_overview_fails_clearly_when_codex_home_is_missing() {
        let missing_home = std::env::temp_dir().join(format!(
            "ccstats-desktop-missing-quota-home-{}",
            std::process::id()
        ));

        let error = load_codex_quota_overview(Some(&missing_home))
            .expect_err("missing Codex home must fail");

        assert!(error.contains("Codex sessions directory was not found"));
    }

    #[test]
    fn overview_command_rejects_an_unknown_source() {
        let error = tauri::async_runtime::block_on(usage_overview("not-a-source".to_string()))
            .expect_err("unknown source must fail");

        assert!(error.contains("invalid usage source"));
    }

    #[test]
    fn batch_overview_command_rejects_an_empty_source_list() {
        let error = tauri::async_runtime::block_on(usage_overviews(Vec::new()))
            .expect_err("empty source list must fail");

        assert_eq!(error, "at least one usage source is required");
    }

    #[test]
    fn analytics_commands_reject_unknown_ranges_before_scanning() {
        let error = tauri::async_runtime::block_on(usage_history(
            "claude".to_string(),
            "forever".to_string(),
        ))
        .expect_err("unknown ranges must fail");

        assert_eq!(error, "invalid usage range 'forever'");
    }

    #[test]
    fn history_export_rejects_unknown_formats() {
        let error = tauri::async_runtime::block_on(export_history(
            "claude".to_string(),
            "today".to_string(),
            "xml".to_string(),
        ))
        .expect_err("unknown export formats must fail");

        assert_eq!(error, "unsupported history export format 'xml'");
    }
}
