use std::str::FromStr;

use ccstats::{
    MultiCostSummary, MultiSummaryOptions, SourceDescriptor, UsageRange, UsageSource,
    list_usage_sources, summarize_cost_ranges_with_cli_config,
};

#[tauri::command]
fn list_sources() -> Result<Vec<SourceDescriptor>, String> {
    list_usage_sources().map_err(|error| error.to_string())
}

#[tauri::command]
async fn usage_overview(source: String) -> Result<MultiCostSummary, String> {
    let source = UsageSource::from_str(&source).map_err(|error| error.to_string())?;
    tauri::async_runtime::spawn_blocking(move || {
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
    })
    .await
    .map_err(|error| format!("usage scan task failed: {error}"))?
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default();
    #[cfg(all(feature = "native-e2e", debug_assertions))]
    let builder = builder.plugin(tauri_plugin_wdio_webdriver::init());

    builder
        .invoke_handler(tauri::generate_handler![list_sources, usage_overview])
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
    fn overview_command_rejects_an_unknown_source() {
        let error = tauri::async_runtime::block_on(usage_overview("not-a-source".to_string()))
            .expect_err("unknown source must fail");

        assert!(error.contains("invalid usage source"));
    }
}
