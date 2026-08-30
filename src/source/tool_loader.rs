use rayon::prelude::*;

use crate::core::{DateFilter, ToolCall};
use crate::source::Source;
use crate::utils::Timezone;

/// Load tool calls for a source with tool-call support.
pub(crate) fn load_tool_calls(
    source: &dyn Source,
    filter: &DateFilter,
    timezone: Timezone,
) -> Vec<ToolCall> {
    if !source.capabilities().has_tool_calls {
        return Vec::new();
    }

    let files = source.find_tool_call_files();
    if files.is_empty() {
        return Vec::new();
    }

    eprintln!("Scanning {} files for tool usage...", files.len());

    let all_calls = files
        .par_iter()
        .flat_map(|path| {
            source
                .parse_tool_call_file(path, timezone)
                .into_iter()
                .filter(|call| {
                    chrono::NaiveDate::parse_from_str(&call.date_str, crate::consts::DATE_FORMAT)
                        .is_ok_and(|date| filter.contains(date))
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<ToolCall>>();

    eprintln!("Found {} tool calls", all_calls.len());
    all_calls
}

#[cfg(test)]
#[path = "loader_tool_tests.rs"]
mod tests;
