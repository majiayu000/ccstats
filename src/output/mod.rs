mod blocks;
mod budget;
mod csv;
mod format;
mod json;
mod period;
mod project;
mod session;
mod statusline;
mod table;
mod tools;
mod top;

/// Central selector for supported CLI output modes.
///
/// Add a real new format by adding a variant and updating the `render_*`
/// matches. Trait/plugin dispatch is intentionally deferred until a fourth
/// format exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OutputFormat {
    Table,
    Json,
    Csv,
}

pub(crate) use blocks::{BlockTableOptions, output_block_json, print_block_table};
pub(crate) use budget::{
    MonthlyBudgetOptions, add_monthly_budget_to_json, monthly_budget_reports,
    print_monthly_budget_table,
};
pub(crate) use csv::{
    append_data_quality_csv_comment, output_block_csv, output_monthly_budget_csv,
    output_period_csv_with_quality, output_project_csv, output_session_csv,
};
pub(crate) use format::NumberFormat;
pub(crate) use json::output_period_json_with_quality;
pub(crate) use period::Period;
pub(crate) use project::{ProjectTableOptions, output_project_json, print_project_table};
pub(crate) use session::{SessionTableOptions, output_session_json, print_session_table};
pub(crate) use statusline::{print_statusline, print_statusline_json_with_quality};
pub(crate) use table::{PeriodSummaryFooter, TokenTableOptions, print_period_table};
pub(crate) use tools::{output_tools_csv, output_tools_json, print_tools_table};
pub(crate) use top::{
    TopRow, TopTableOptions, output_top_csv, output_top_json, print_top_table, rank_by_model,
    rank_by_project,
};
