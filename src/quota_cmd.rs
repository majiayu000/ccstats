use crate::app::{CommandContext, print_json};
use crate::output::{
    OutputFormat, QuotaValueEstimate, output_quota_csv, output_quota_json, print_quota_table,
};
use crate::sdk::{
    CodexWeeklyValueError, CodexWeeklyValueEstimate, estimate_codex_weekly_value_with_pricing,
};
use crate::source::{CodexQuotaError, CodexWeeklyQuota, load_weekly_quota};

pub(crate) struct LoadedQuota {
    pub report: CodexWeeklyQuota,
    pub value_estimate: Option<Result<CodexWeeklyValueEstimate, CodexWeeklyValueError>>,
}

impl LoadedQuota {
    pub(crate) fn rendered(&self) -> QuotaValueEstimate<'_> {
        self.value_estimate.as_ref().map(Result::as_ref)
    }
}

pub(crate) fn load_quota(ctx: &CommandContext<'_>) -> Result<LoadedQuota, CodexQuotaError> {
    let report = load_weekly_quota()?;
    let value_estimate = ctx
        .cli
        .show_cost()
        .then(|| estimate_codex_weekly_value_with_pricing(&report, None, ctx.pricing_db));
    Ok(LoadedQuota {
        report,
        value_estimate,
    })
}

pub(crate) fn handle_quota(ctx: &CommandContext<'_>) {
    let loaded = match load_quota(ctx) {
        Ok(loaded) => loaded,
        Err(error) => {
            eprintln!("Error: {error}");
            std::process::exit(1);
        }
    };
    let rendered_estimate = loaded.rendered();

    match ctx.cli.output_format() {
        OutputFormat::Json => print_json(
            &output_quota_json(&loaded.report, rendered_estimate),
            ctx.jq_filter,
        ),
        OutputFormat::Csv => print!("{}", output_quota_csv(&loaded.report, rendered_estimate)),
        OutputFormat::Table => print_quota_table(
            &loaded.report,
            rendered_estimate,
            ctx.timezone,
            ctx.number_format,
            ctx.cli.use_color(),
        ),
    }
}
