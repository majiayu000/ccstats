use crate::app::{CommandContext, print_json};
use crate::output::{OutputFormat, output_quota_csv, output_quota_json, print_quota_table};
use crate::source::load_weekly_quota;

pub(crate) fn handle_quota(ctx: &CommandContext<'_>) {
    let report = match load_weekly_quota() {
        Ok(report) => report,
        Err(error) => {
            eprintln!("Error: {error}");
            std::process::exit(1);
        }
    };

    match ctx.cli.output_format() {
        OutputFormat::Json => print_json(&output_quota_json(&report), ctx.jq_filter),
        OutputFormat::Csv => print!("{}", output_quota_csv(&report)),
        OutputFormat::Table => print_quota_table(&report, ctx.timezone, ctx.cli.use_color()),
    }
}
