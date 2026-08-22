use crate::app::{CommandContext, print_json};
use crate::output::{
    OutputFormat, QuotaValueEstimate, output_quota_csv, output_quota_json, print_quota_table,
};
use crate::sdk::estimate_codex_weekly_value_with_pricing;
use crate::source::load_weekly_quota;

pub(crate) fn handle_quota(ctx: &CommandContext<'_>) {
    let report = match load_weekly_quota() {
        Ok(report) => report,
        Err(error) => {
            eprintln!("Error: {error}");
            std::process::exit(1);
        }
    };
    let value_estimate = ctx
        .cli
        .show_cost()
        .then(|| estimate_codex_weekly_value_with_pricing(&report, None, ctx.pricing_db));
    let rendered_estimate: QuotaValueEstimate<'_> = value_estimate.as_ref().map(Result::as_ref);

    match ctx.cli.output_format() {
        OutputFormat::Json => print_json(
            &output_quota_json(&report, rendered_estimate),
            ctx.jq_filter,
        ),
        OutputFormat::Csv => print!("{}", output_quota_csv(&report, rendered_estimate)),
        OutputFormat::Table => print_quota_table(
            &report,
            rendered_estimate,
            ctx.timezone,
            ctx.number_format,
            ctx.cli.use_color(),
        ),
    }
}
