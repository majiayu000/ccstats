use std::collections::HashMap;

use crate::core::{DateFilter, DayStats, LoadResult};
use crate::output::{
    Period, PeriodSummaryFooter, TokenTableOptions, monthly_budget_reports,
    print_monthly_budget_table, print_period_table,
};
use crate::pricing::CostDisplayMode;
use crate::source::{Capabilities, CodexScope, GrokCostReport};

use super::{CommandContext, cost_coverage};
use crate::source::{CodexSource, get_source, load_daily};

#[derive(Clone, Copy)]
pub(super) struct PeriodTableFlags {
    pub(super) cost_mode: CostDisplayMode,
    pub(super) is_today: bool,
    pub(super) source_count: Option<usize>,
    pub(super) source_name: Option<&'static str>,
}

pub(super) fn render(
    result: &LoadResult,
    period: Period,
    caps: &Capabilities,
    codex_scope: Option<CodexScope>,
    grok_reports: Option<&HashMap<String, GrokCostReport>>,
    ctx: &CommandContext<'_>,
    flags: PeriodTableFlags,
) {
    if let Some(scope) = codex_scope {
        println!("\n  Codex scope: {}", scope.as_str());
    }

    let history = if flags.is_today && !ctx.cli.compact && result.parse_errors == 0 {
        prior_days(flags.source_name, ctx)
    } else {
        HashMap::new()
    };
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
            cost_mode: flags.cost_mode,
            cost_label: if selected_grok_report.is_some() {
                "Grok Reported"
            } else {
                "Cost"
            },
            cost_display_overrides: grok_cost_displays
                .as_ref()
                .map(|displays| &displays.provider_rows),
            total_cost_display_override: grok_cost_displays
                .as_ref()
                .map(|displays| displays.provider_total.as_str()),
            secondary_cost_label: selected_grok_report.map(|_| "API Eq. Price"),
            secondary_cost_display_overrides: grok_cost_displays
                .as_ref()
                .map(|displays| &displays.api_rows),
            secondary_total_cost_display_override: grok_cost_displays
                .as_ref()
                .map(|displays| displays.api_total.as_str()),
            pricing_note_override: selected_grok_report.map(|_| {
                "Grok Reported comes from costUsdTicks; API Eq. Price uses xAI public rates. ~ is estimated, ranges mark unknown request boundaries, and ≥ marks partial Grok coverage."
            }),
            comparison_days: Some(&history),
            is_today: flags.is_today,
            source_count: flags.source_count,
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
            flags.cost_mode,
        );
        print_monthly_budget_table(&reports, ctx.cli.use_color(), ctx.currency);
    }
}

fn prior_days(source_name: Option<&str>, ctx: &CommandContext<'_>) -> HashMap<String, DayStats> {
    let today = ctx
        .timezone
        .to_fixed_offset(chrono::Utc::now())
        .date_naive();
    let filter = DateFilter::new(
        today.checked_sub_signed(chrono::Duration::days(7)),
        today.pred_opt(),
    );
    let result = match source_name {
        Some("codex") => load_daily(
            &CodexSource::with_scope(ctx.cli.codex_scope),
            &filter,
            ctx.timezone,
            true,
            ctx.cli.debug,
        ),
        Some(name) => {
            let Some(source) = get_source(name) else {
                return HashMap::new();
            };
            load_daily(source, &filter, ctx.timezone, true, ctx.cli.debug)
        }
        None => {
            let prior_ctx = CommandContext {
                filter: &filter,
                ..*ctx
            };
            super::source_breakdown::load_all_sources(&prior_ctx, true, false).combined
        }
    };
    if result.parse_errors > 0 {
        HashMap::new()
    } else {
        result.day_stats
    }
}
