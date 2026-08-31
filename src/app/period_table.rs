use std::collections::HashMap;

use crate::core::LoadResult;
use crate::output::{
    Period, PeriodSummaryFooter, TokenTableOptions, monthly_budget_reports,
    print_monthly_budget_table, print_period_table,
};
use crate::pricing::CostDisplayMode;
use crate::source::{Capabilities, CodexScope, GrokCostReport};

use super::{CommandContext, cost_coverage};

pub(super) fn render(
    result: &LoadResult,
    period: Period,
    caps: &Capabilities,
    codex_scope: Option<CodexScope>,
    grok_reports: Option<&HashMap<String, GrokCostReport>>,
    ctx: &CommandContext<'_>,
    cost_mode: CostDisplayMode,
) {
    if let Some(scope) = codex_scope {
        println!("\n  Codex scope: {}", scope.as_str());
    }

    let selected_grok_report = cost_coverage::selected_grok_report(grok_reports);
    let grok_cost_displays = grok_reports
        .map(|reports| cost_coverage::table_cost_displays(reports, period, ctx.currency));
    print_period_table(
        &result.day_stats,
        period,
        ctx.cli.breakdown,
        PeriodSummaryFooter {
            skipped: result.skipped,
            valid: result.valid,
            elapsed_ms: Some(result.elapsed_ms),
        },
        ctx.pricing_db,
        TokenTableOptions {
            order: ctx.cli.sort_order(),
            use_color: ctx.cli.use_color(),
            compact: ctx.cli.compact,
            show_cost: ctx.cli.show_cost(),
            number_format: ctx.number_format,
            show_reasoning: caps.has_reasoning_tokens,
            show_cache_creation: caps.has_cache_creation,
            supports_cache_read: caps.has_cache_read,
            currency: ctx.currency,
            cost_mode,
            cost_label: if selected_grok_report.is_some() {
                "API Eq. Price"
            } else {
                "Cost"
            },
            cost_display_overrides: grok_cost_displays
                .as_ref()
                .map(|displays| &displays.0),
            total_cost_display_override: grok_cost_displays
                .as_ref()
                .map(|displays| displays.1.as_str()),
            pricing_note_override: selected_grok_report.map(|_| {
                "Pricing source: xAI public API rates; ~ is estimated, N/A has no request coverage."
            }),
        },
    );
    if ctx.cli.show_cost() {
        cost_coverage::print_note(selected_grok_report, ctx.currency);
    }
    if period == Period::Month
        && let Some(budget) = ctx.cli.monthly_budget
    {
        let reports = monthly_budget_reports(
            &result.day_stats,
            ctx.pricing_db,
            ctx.cli.sort_order(),
            budget,
            ctx.budget_as_of,
            ctx.currency,
            cost_mode,
        );
        print_monthly_budget_table(&reports, ctx.cli.use_color(), ctx.currency);
    }
}
