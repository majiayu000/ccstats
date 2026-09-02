//! Per-source subtotals for `--source all --source-breakdown`.

use std::time::Instant;

use serde_json::{Value, json};

use super::{
    CommandContext, cost_coverage, period_table, print_json, print_no_data_hint,
    should_render_empty_structured_result,
};
use crate::core::{LoadResult, apply_real_token_totals_for_all_source, merge_day_stats};
use crate::output::{
    OutputFormat, Period, PeriodSummaryFooter, TokenTableOptions, add_monthly_budget_to_json,
    append_data_quality_csv_comment, csv_escape, monthly_budget_reports,
    output_period_csv_with_quality, output_period_json_with_quality, print_period_table,
};
use crate::pricing::CostDisplayMode;
use crate::source::{Capabilities, CostCoverage, all_capabilities, all_sources, load_daily};

pub(super) struct SourceSection {
    name: &'static str,
    display_name: &'static str,
    result: LoadResult,
}

pub(super) struct AllSourceLoad {
    pub(super) combined: LoadResult,
    pub(super) caps: Capabilities,
    sections: Vec<SourceSection>,
}

pub(super) fn load_all_sources(
    ctx: &CommandContext<'_>,
    quiet: bool,
    keep_sections: bool,
) -> AllSourceLoad {
    let start = Instant::now();
    let mut combined = LoadResult::default();
    let caps = all_capabilities();
    let mut sections = Vec::new();

    for source in all_sources() {
        let mut result = load_daily(source, ctx.filter, ctx.timezone, quiet, ctx.cli.debug);
        if keep_sections {
            apply_real_token_totals_for_all_source(&mut result.day_stats);
        }
        combined.skipped += result.skipped;
        combined.valid += result.valid;
        combined.parse_errors += result.parse_errors;
        if keep_sections && !result.day_stats.is_empty() {
            merge_day_stats(&mut combined.day_stats, result.day_stats.clone());
            sections.push(SourceSection {
                name: source.name(),
                display_name: source.display_name(),
                result,
            });
        } else {
            merge_day_stats(&mut combined.day_stats, result.day_stats);
        }
    }

    if !keep_sections {
        // Default all-source token columns are CostKind::Real only. Keep
        // estimated_proxy populated so estimated_cost still reports proxy rows
        // (e.g. Grok context snapshots). Display buckets are already real, so
        // callers must use CostDisplayMode::Total — RealOnly would subtract
        // estimated_proxy a second time.
        apply_real_token_totals_for_all_source(&mut combined.day_stats);
    }

    combined.elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
    AllSourceLoad {
        combined,
        caps,
        sections,
    }
}

pub(super) fn render(period: Period, ctx: &CommandContext<'_>) {
    let loaded = load_all_sources(ctx, false, true);
    if loaded.combined.day_stats.is_empty()
        && !should_render_empty_structured_result(&loaded.combined, ctx)
    {
        print_no_data_hint("All Sources", "usage");
        return;
    }

    match ctx.cli.output_format() {
        OutputFormat::Csv => render_csv(&loaded, period, ctx),
        OutputFormat::Json => render_json(&loaded, period, ctx),
        OutputFormat::Table => render_table(&loaded, period, ctx),
    }
}

fn period_json(
    result: &LoadResult,
    period: Period,
    caps: &Capabilities,
    ctx: &CommandContext<'_>,
) -> String {
    output_period_json_with_quality(
        &result.day_stats,
        period,
        ctx.pricing_db,
        ctx.cli.sort_order(),
        ctx.cli.breakdown,
        ctx.cli.show_cost(),
        caps.has_cache_read,
        ctx.currency,
        Some(result.data_quality()),
        CostDisplayMode::Total,
    )
}

fn period_csv(
    result: &LoadResult,
    period: Period,
    caps: &Capabilities,
    ctx: &CommandContext<'_>,
) -> String {
    output_period_csv_with_quality(
        &result.day_stats,
        period,
        ctx.pricing_db,
        ctx.cli.sort_order(),
        ctx.cli.breakdown,
        ctx.cli.show_cost(),
        caps.has_cache_read,
        ctx.currency,
        None,
        CostDisplayMode::Total,
    )
}

fn parse_json_value(json: &str) -> Value {
    serde_json::from_str(json).unwrap_or_else(|_| json!([]))
}

fn render_json(loaded: &AllSourceLoad, period: Period, ctx: &CommandContext<'_>) {
    let monthly_budget = (period == Period::Month)
        .then_some(ctx.cli.monthly_budget)
        .flatten();

    let sources: Vec<Value> = loaded
        .sections
        .iter()
        .map(|section| {
            let json = period_json(&section.result, period, &loaded.caps, ctx);
            let json = cost_coverage::annotate_json(&json, &section.result.day_stats, period, None);
            json!({
                "source": section.name,
                "rows": parse_json_value(&json),
            })
        })
        .collect();

    let mut total = period_json(&loaded.combined, period, &loaded.caps, ctx);
    if let Some(budget) = monthly_budget {
        let reports = monthly_budget_reports(
            &loaded.combined.day_stats,
            ctx.pricing_db,
            ctx.cli.sort_order(),
            budget,
            ctx.budget_as_of,
            ctx.currency,
            CostDisplayMode::Total,
        );
        total = add_monthly_budget_to_json(&total, &reports);
    }
    total = cost_coverage::annotate_json(&total, &loaded.combined.day_stats, period, None);

    let wrapped = json!({
        "sources": sources,
        "total": parse_json_value(&total),
    });
    let json = serde_json::to_string(&wrapped).unwrap_or_else(|e| {
        eprintln!("Failed to serialize JSON output: {e}");
        "{}".to_string()
    });
    print_json(&json, ctx.jq_filter);
}

fn with_source_column(csv: &str, source: &str) -> String {
    let mut out = String::new();
    let mut header_written = false;
    for line in csv.lines() {
        if line.starts_with('#') || line.is_empty() {
            out.push_str(line);
            out.push('\n');
            continue;
        }
        if header_written {
            out.push_str(&csv_escape(source));
            out.push(',');
            out.push_str(line);
            out.push('\n');
        } else {
            out.push_str("source,");
            out.push_str(line);
            out.push('\n');
            header_written = true;
        }
    }
    out
}

fn render_csv(loaded: &AllSourceLoad, period: Period, ctx: &CommandContext<'_>) {
    let mut csv = String::new();
    for (index, section) in loaded.sections.iter().enumerate() {
        if index > 0 {
            csv.push('\n');
        }
        csv.push_str(&with_source_column(
            &period_csv(&section.result, period, &loaded.caps, ctx),
            section.name,
        ));
    }
    if !loaded.sections.is_empty() {
        csv.push('\n');
    }
    csv.push_str(&with_source_column(
        &period_csv(&loaded.combined, period, &loaded.caps, ctx),
        "total",
    ));
    append_data_quality_csv_comment(&mut csv, Some(loaded.combined.data_quality()));
    let coverage =
        CostCoverage::from_stats(loaded.combined.day_stats.values().map(|day| &day.stats));
    let csv = cost_coverage::annotate_csv(csv, coverage, None);
    print!("{csv}");
}

fn table_options<'a>(caps: &Capabilities, ctx: &'a CommandContext<'a>) -> TokenTableOptions<'a> {
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
        cost_mode: CostDisplayMode::Total,
        cost_label: "Cost",
        cost_display_overrides: None,
        total_cost_display_override: None,
        secondary_cost_label: None,
        secondary_cost_display_overrides: None,
        secondary_total_cost_display_override: None,
        pricing_note_override: None,
    }
}

fn print_section_table(
    result: &LoadResult,
    period: Period,
    caps: &Capabilities,
    ctx: &CommandContext<'_>,
    heading: &str,
) {
    println!("\n  {heading}");
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
        table_options(caps, ctx),
    );
}

fn render_table(loaded: &AllSourceLoad, period: Period, ctx: &CommandContext<'_>) {
    for section in &loaded.sections {
        print_section_table(
            &section.result,
            period,
            &loaded.caps,
            ctx,
            section.display_name,
        );
    }
    println!("\n  All Sources");
    period_table::render(
        &loaded.combined,
        period,
        &loaded.caps,
        None,
        None,
        ctx,
        CostDisplayMode::Total,
    );
}
