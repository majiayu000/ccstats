mod codex_scope;
mod cost_coverage;

use std::time::Instant;

use crate::cli::{Cli, SourceCommand, TopDimension};
use crate::core::{
    BlockStats, DateFilter, LoadResult, ProjectStats, SessionStats, ToolSummary, aggregate_tools,
    merge_day_stats,
};
use crate::output::NumberFormat;
use crate::output::{
    BlockTableOptions, MonthlyBudgetOptions, OutputFormat, Period, PeriodSummaryFooter,
    ProjectTableOptions, SessionTableOptions, TokenTableOptions, TopRow, TopTableOptions,
    add_monthly_budget_to_json, append_data_quality_csv_comment, monthly_budget_reports,
    output_block_csv, output_block_json, output_monthly_budget_csv, output_period_csv_with_quality,
    output_period_json_with_quality, output_project_csv, output_project_json, output_session_csv,
    output_session_json, output_tools_csv, output_tools_json, output_top_csv, output_top_json,
    print_block_table, print_monthly_budget_table, print_period_table, print_project_table,
    print_session_table, print_statusline, print_statusline_json_with_quality, print_tools_table,
    print_top_table, rank_by_model, rank_by_model_with_cost_mode, rank_by_project,
};
use crate::pricing::{CostDisplayMode, PricingDb};
use crate::source::{
    Capabilities, CodexScope, CostCoverage, Source, all_capabilities, all_sources, load_blocks,
    load_daily, load_projects, load_sessions, load_tool_calls,
};
use crate::utils::{Timezone, filter_json};

/// Print JSON output, optionally filtering through jq
pub(crate) fn print_json(json: &str, jq_filter: Option<&str>) {
    match jq_filter {
        Some(filter) => match filter_json(json, filter) {
            Ok(filtered) => print!("{filtered}"),
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        },
        None => println!("{json}"),
    }
}

pub(crate) struct CommandContext<'a> {
    pub(crate) filter: &'a DateFilter,
    pub(crate) cli: &'a Cli,
    pub(crate) pricing_db: &'a PricingDb,
    pub(crate) timezone: Timezone,
    pub(crate) number_format: NumberFormat,
    pub(crate) jq_filter: Option<&'a str>,
    pub(crate) currency: Option<&'a crate::pricing::CurrencyConverter>,
    pub(crate) budget_as_of: chrono::NaiveDate,
}

pub(crate) fn print_no_data_hint(source_name: &str, category: &str) {
    println!(
        "No {source_name} {category} data found in the selected date range.\nHint: widen --since/--until, try `today`, or run `ccstats sources` to pick a different --source."
    );
}

fn codex_scope_for_source(source: &dyn Source, ctx: &CommandContext<'_>) -> Option<CodexScope> {
    (source.name() == "codex" && ctx.cli.codex_scope != CodexScope::All)
        .then_some(ctx.cli.codex_scope)
}

fn source_label(source: &dyn Source, ctx: &CommandContext<'_>) -> String {
    match codex_scope_for_source(source, ctx) {
        Some(scope) => format!("{} ({})", source.display_name(), scope.label()),
        None => source.display_name().to_string(),
    }
}

fn print_codex_scope_note(scope: Option<CodexScope>) {
    if let Some(scope) = scope {
        println!("\n  Codex scope: {}", scope.as_str());
    }
}

fn should_render_empty_structured_result(result: &LoadResult, ctx: &CommandContext<'_>) -> bool {
    result.day_stats.is_empty()
        && result.data_quality().has_warnings()
        && matches!(
            ctx.cli.output_format(),
            OutputFormat::Json | OutputFormat::Csv
        )
}

fn handle_session(source: &dyn Source, ctx: &CommandContext<'_>) {
    let sessions = load_sessions(source, ctx.filter, ctx.timezone, false);
    if sessions.is_empty() {
        print_no_data_hint(&source_label(source, ctx), "session");
        return;
    }

    render_session(&sessions, source, ctx);
}

fn render_session(sessions: &[SessionStats], source: &dyn Source, ctx: &CommandContext<'_>) {
    let scope = codex_scope_for_source(source, ctx);
    match ctx.cli.output_format() {
        OutputFormat::Csv => {
            let csv = output_session_csv(
                sessions,
                ctx.pricing_db,
                ctx.cli.sort_order(),
                ctx.cli.show_cost(),
                source.capabilities().has_cache_read,
                ctx.currency,
            );
            print!("{}", codex_scope::annotate_csv(csv, scope));
        }
        OutputFormat::Json => {
            let json = output_session_json(
                sessions,
                ctx.pricing_db,
                ctx.cli.sort_order(),
                ctx.cli.show_cost(),
                source.capabilities().has_cache_read,
                ctx.currency,
            );
            let json = codex_scope::annotate_json(&json, scope);
            print_json(&json, ctx.jq_filter);
        }
        OutputFormat::Table => {
            let label = source_label(source, ctx);
            print_session_table(
                sessions,
                ctx.pricing_db,
                SessionTableOptions {
                    order: ctx.cli.sort_order(),
                    use_color: ctx.cli.use_color(),
                    compact: ctx.cli.compact,
                    show_cost: ctx.cli.show_cost(),
                    supports_cache_read: source.capabilities().has_cache_read,
                    number_format: ctx.number_format,
                    source_label: &label,
                    timezone: ctx.timezone,
                    currency: ctx.currency,
                },
            );
        }
    }
}

fn handle_project(source: &dyn Source, ctx: &CommandContext<'_>) {
    let projects = load_projects(source, ctx.filter, ctx.timezone, false);
    if projects.is_empty() {
        print_no_data_hint(source.display_name(), "project");
        return;
    }

    render_project(&projects, source, ctx);
}

fn render_project(projects: &[ProjectStats], source: &dyn Source, ctx: &CommandContext<'_>) {
    match ctx.cli.output_format() {
        OutputFormat::Csv => {
            let csv = output_project_csv(
                projects,
                ctx.pricing_db,
                ctx.cli.sort_order(),
                ctx.cli.show_cost(),
                source.capabilities().has_cache_read,
                ctx.currency,
            );
            print!("{csv}");
        }
        OutputFormat::Json => {
            let json = output_project_json(
                projects,
                ctx.pricing_db,
                ctx.cli.sort_order(),
                ctx.cli.show_cost(),
                source.capabilities().has_cache_read,
                ctx.currency,
            );
            print_json(&json, ctx.jq_filter);
        }
        OutputFormat::Table => print_project_table(
            projects,
            ctx.pricing_db,
            ProjectTableOptions {
                order: ctx.cli.sort_order(),
                use_color: ctx.cli.use_color(),
                compact: ctx.cli.compact,
                show_cost: ctx.cli.show_cost(),
                supports_cache_read: source.capabilities().has_cache_read,
                source_label: source.display_name(),
                number_format: ctx.number_format,
                currency: ctx.currency,
            },
        ),
    }
}

fn handle_blocks(source: &dyn Source, ctx: &CommandContext<'_>) {
    let blocks = load_blocks(source, ctx.filter, ctx.timezone, false);
    if blocks.is_empty() {
        print_no_data_hint(source.display_name(), "billing block");
        return;
    }

    render_blocks(&blocks, source, ctx);
}

fn render_blocks(blocks: &[BlockStats], source: &dyn Source, ctx: &CommandContext<'_>) {
    match ctx.cli.output_format() {
        OutputFormat::Csv => {
            let csv = output_block_csv(
                blocks,
                ctx.pricing_db,
                ctx.cli.sort_order(),
                ctx.cli.show_cost(),
                source.capabilities().has_cache_read,
                ctx.currency,
            );
            print!("{csv}");
        }
        OutputFormat::Json => {
            let json = output_block_json(
                blocks,
                ctx.pricing_db,
                ctx.cli.sort_order(),
                ctx.cli.show_cost(),
                source.capabilities().has_cache_read,
                ctx.currency,
            );
            print_json(&json, ctx.jq_filter);
        }
        OutputFormat::Table => print_block_table(
            blocks,
            ctx.pricing_db,
            BlockTableOptions {
                order: ctx.cli.sort_order(),
                use_color: ctx.cli.use_color(),
                compact: ctx.cli.compact,
                show_cost: ctx.cli.show_cost(),
                supports_cache_read: source.capabilities().has_cache_read,
                source_label: source.display_name(),
                number_format: ctx.number_format,
                currency: ctx.currency,
            },
        ),
    }
}

#[derive(Debug, Clone, Copy)]
struct TopRenderOptions<'a> {
    source_label: &'a str,
    supports_cache_read: bool,
    codex_scope: Option<CodexScope>,
    cost_mode: CostDisplayMode,
}

fn handle_top(
    rows: &[TopRow],
    dim: TopDimension,
    limit: usize,
    options: TopRenderOptions<'_>,
    ctx: &CommandContext<'_>,
) {
    if rows.is_empty() {
        print_no_data_hint(options.source_label, "usage");
        return;
    }

    match ctx.cli.output_format() {
        OutputFormat::Csv => {
            let csv = output_top_csv(
                rows,
                dim,
                limit,
                ctx.cli.show_cost(),
                options.supports_cache_read,
                ctx.currency,
            );
            print!("{}", codex_scope::annotate_csv(csv, options.codex_scope));
        }
        OutputFormat::Json => {
            let json = output_top_json(
                rows,
                dim,
                limit,
                ctx.cli.show_cost(),
                options.supports_cache_read,
                ctx.currency,
            );
            let json = codex_scope::annotate_json(&json, options.codex_scope);
            print_json(&json, ctx.jq_filter);
        }
        OutputFormat::Table => print_top_table(
            rows,
            TopTableOptions {
                use_color: ctx.cli.use_color(),
                compact: ctx.cli.compact,
                show_cost: ctx.cli.show_cost(),
                supports_cache_read: options.supports_cache_read,
                source_label: options.source_label,
                number_format: ctx.number_format,
                currency: ctx.currency,
                dim,
                limit,
                cost_mode: options.cost_mode,
            },
        ),
    }
}

fn handle_top_for_source(
    source: &dyn Source,
    dim: TopDimension,
    limit: usize,
    ctx: &CommandContext<'_>,
) {
    let scope = codex_scope_for_source(source, ctx);
    let label = source_label(source, ctx);
    match dim {
        TopDimension::Model => {
            let result = load_daily(source, ctx.filter, ctx.timezone, false, ctx.cli.debug);
            let rows = rank_by_model(&result.day_stats, ctx.pricing_db);
            handle_top(
                &rows,
                dim,
                limit,
                TopRenderOptions {
                    source_label: &label,
                    supports_cache_read: source.capabilities().has_cache_read,
                    codex_scope: scope,
                    cost_mode: CostDisplayMode::Total,
                },
                ctx,
            );
        }
        TopDimension::Project => {
            if !source.capabilities().has_projects {
                println!(
                    "{} does not support project ranking.\nHint: try `--dim model`, or run `ccstats sources` to inspect capabilities.",
                    source.display_name()
                );
                return;
            }
            let projects = load_projects(source, ctx.filter, ctx.timezone, false);
            let rows = rank_by_project(&projects, ctx.pricing_db);
            handle_top(
                &rows,
                dim,
                limit,
                TopRenderOptions {
                    source_label: &label,
                    supports_cache_read: source.capabilities().has_cache_read,
                    codex_scope: scope,
                    cost_mode: CostDisplayMode::Total,
                },
                ctx,
            );
        }
    }
}

fn validate_top_limit(limit: usize) -> Result<usize, String> {
    if limit == 0 {
        Err("--limit must be at least 1".to_string())
    } else if limit > 1000 {
        Err("--limit must be at most 1000".to_string())
    } else {
        Ok(limit)
    }
}

fn handle_tools(source: &dyn Source, ctx: &CommandContext<'_>) {
    let calls = load_tool_calls(source, ctx.filter, ctx.timezone);
    let summary = aggregate_tools(&calls);

    render_tools(&summary, ctx);
}

fn render_tools(summary: &ToolSummary, ctx: &CommandContext<'_>) {
    match ctx.cli.output_format() {
        OutputFormat::Csv => {
            let csv = output_tools_csv(summary);
            print!("{csv}");
        }
        OutputFormat::Json => {
            let json = output_tools_json(summary);
            print_json(&json, ctx.jq_filter);
        }
        OutputFormat::Table => print_tools_table(summary, ctx.cli.use_color()),
    }
}

/// Statusline keeps its compact single-line semantics outside generic output dispatch.
fn handle_statusline(source: &dyn Source, ctx: &CommandContext<'_>) {
    let scope = codex_scope_for_source(source, ctx);
    let label = source_label(source, ctx);
    let result = load_daily(source, ctx.filter, ctx.timezone, true, false);
    if ctx.cli.json {
        let json = print_statusline_json_with_quality(
            &result.day_stats,
            ctx.pricing_db,
            &label,
            ctx.number_format,
            ctx.currency,
            source.capabilities().has_cache_read,
            Some(result.data_quality()),
            CostDisplayMode::Total,
        );
        let json = codex_scope::annotate_json(&json, scope);
        print_json(&json, ctx.jq_filter);
    } else {
        print_statusline(
            &result.day_stats,
            ctx.pricing_db,
            &label,
            ctx.number_format,
            ctx.currency,
            source.capabilities().has_cache_read,
            CostDisplayMode::Total,
        );
    }
}

#[allow(clippy::too_many_lines)]
fn render_period_result(
    result: &LoadResult,
    period: Period,
    caps: &Capabilities,
    codex_scope: Option<CodexScope>,
    cost_coverage: Option<CostCoverage>,
    ctx: &CommandContext<'_>,
    cost_mode: CostDisplayMode,
) {
    let monthly_budget = (period == Period::Month)
        .then_some(ctx.cli.monthly_budget)
        .flatten();

    match ctx.cli.output_format() {
        OutputFormat::Csv => {
            let csv = if let Some(budget) = monthly_budget {
                let mut csv = output_monthly_budget_csv(
                    &result.day_stats,
                    ctx.pricing_db,
                    MonthlyBudgetOptions {
                        order: ctx.cli.sort_order(),
                        breakdown: ctx.cli.breakdown,
                        show_cost: ctx.cli.show_cost(),
                        supports_cache_read: caps.has_cache_read,
                        limit: budget,
                        as_of: ctx.budget_as_of,
                        currency: ctx.currency,
                        cost_mode,
                    },
                );
                append_data_quality_csv_comment(&mut csv, Some(result.data_quality()));
                csv
            } else {
                output_period_csv_with_quality(
                    &result.day_stats,
                    period,
                    ctx.pricing_db,
                    ctx.cli.sort_order(),
                    ctx.cli.breakdown,
                    ctx.cli.show_cost(),
                    caps.has_cache_read,
                    ctx.currency,
                    Some(result.data_quality()),
                    cost_mode,
                )
            };
            let csv = cost_coverage::annotate_csv(csv, cost_coverage);
            print!("{}", codex_scope::annotate_csv(csv, codex_scope));
        }
        OutputFormat::Json => {
            let mut json = output_period_json_with_quality(
                &result.day_stats,
                period,
                ctx.pricing_db,
                ctx.cli.sort_order(),
                ctx.cli.breakdown,
                ctx.cli.show_cost(),
                caps.has_cache_read,
                ctx.currency,
                Some(result.data_quality()),
                cost_mode,
            );
            if let Some(budget) = monthly_budget {
                let reports = monthly_budget_reports(
                    &result.day_stats,
                    ctx.pricing_db,
                    ctx.cli.sort_order(),
                    budget,
                    ctx.budget_as_of,
                    ctx.currency,
                    cost_mode,
                );
                json = add_monthly_budget_to_json(&json, &reports);
            }
            json = cost_coverage::annotate_json(&json, cost_coverage);
            json = codex_scope::annotate_json(&json, codex_scope);
            print_json(&json, ctx.jq_filter);
        }
        OutputFormat::Table => {
            print_codex_scope_note(codex_scope);
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
                },
            );
            if ctx.cli.show_cost() {
                cost_coverage::print_note(cost_coverage);
            }
            if let Some(budget) = monthly_budget {
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
    }
}

fn handle_period(
    source: &dyn Source,
    command: SourceCommand,
    caps: &Capabilities,
    ctx: &CommandContext<'_>,
) {
    let period = match command {
        SourceCommand::Daily | SourceCommand::Today => Period::Day,
        SourceCommand::Weekly => Period::Week,
        SourceCommand::Monthly => Period::Month,
        _ => return,
    };

    let result = load_daily(source, ctx.filter, ctx.timezone, false, ctx.cli.debug);
    if result.day_stats.is_empty() && !should_render_empty_structured_result(&result, ctx) {
        print_no_data_hint(&source_label(source, ctx), "usage");
        return;
    }
    render_period_result(
        &result,
        period,
        caps,
        codex_scope_for_source(source, ctx),
        source.cost_coverage(ctx.filter, ctx.timezone),
        ctx,
        CostDisplayMode::Total,
    );
}

/// Handle commands for a specific data source
pub(crate) fn handle_source_command(
    source: &dyn Source,
    command: SourceCommand,
    ctx: &CommandContext<'_>,
) {
    let caps = source.capabilities();

    match command {
        SourceCommand::Sources => return crate::sources_cmd::handle_sources(ctx),
        SourceCommand::Quota => return crate::quota_cmd::handle_quota(ctx),
        SourceCommand::Session => return handle_session(source, ctx),
        SourceCommand::Project => {
            if !caps.has_projects {
                println!(
                    "{} does not support project aggregation.\nHint: try `session`/`daily`, or run `ccstats sources` to inspect capabilities.",
                    source.display_name()
                );
                return;
            }
            return handle_project(source, ctx);
        }
        SourceCommand::Blocks => {
            if !caps.has_billing_blocks {
                println!(
                    "{} does not support billing block aggregation.\nHint: try `daily`/`session`, or run `ccstats sources` to inspect capabilities.",
                    source.display_name()
                );
                return;
            }
            return handle_blocks(source, ctx);
        }
        SourceCommand::Endpoints => return crate::endpoints_cmd::handle_endpoints(source, ctx),
        SourceCommand::Statusline => return handle_statusline(source, ctx),
        SourceCommand::Tools => {
            if !caps.has_tool_calls {
                println!(
                    "Tool usage analysis is only supported for Claude source.\nHint: switch with `--source claude` (or alias `--source cc`)."
                );
                return;
            }
            return handle_tools(source, ctx);
        }
        SourceCommand::Top { dim, limit } => {
            let limit = match validate_top_limit(limit) {
                Ok(l) => l,
                Err(msg) => {
                    eprintln!("Error: {msg}");
                    std::process::exit(1);
                }
            };
            return handle_top_for_source(source, dim, limit, ctx);
        }
        SourceCommand::Daily
        | SourceCommand::Today
        | SourceCommand::Weekly
        | SourceCommand::Monthly => {}
    }

    // Period-based commands: Daily/Today/Weekly/Monthly
    handle_period(source, command, &caps, ctx);
}

fn load_all_daily(ctx: &CommandContext<'_>, quiet: bool) -> (LoadResult, Capabilities) {
    let start = Instant::now();
    let mut combined = LoadResult::default();
    let caps = all_capabilities();

    for source in all_sources() {
        let result = load_daily(source, ctx.filter, ctx.timezone, quiet, ctx.cli.debug);
        combined.skipped += result.skipped;
        combined.valid += result.valid;
        combined.parse_errors += result.parse_errors;
        merge_day_stats(&mut combined.day_stats, result.day_stats);
    }

    combined.elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
    (combined, caps)
}

fn handle_all_period(command: SourceCommand, ctx: &CommandContext<'_>) {
    let period = match command {
        SourceCommand::Daily | SourceCommand::Today => Period::Day,
        SourceCommand::Weekly => Period::Week,
        SourceCommand::Monthly => Period::Month,
        _ => return,
    };

    let (result, caps) = load_all_daily(ctx, false);
    if result.day_stats.is_empty() && !should_render_empty_structured_result(&result, ctx) {
        print_no_data_hint("All Sources", "usage");
        return;
    }
    let cost_coverage = all_sources()
        .filter_map(|source| source.cost_coverage(ctx.filter, ctx.timezone))
        .reduce(|left, right| CostCoverage {
            total_tokens: left.total_tokens.saturating_add(right.total_tokens),
            priced_tokens: left.priced_tokens.saturating_add(right.priced_tokens),
        });
    render_period_result(
        &result,
        period,
        &caps,
        None,
        cost_coverage,
        ctx,
        CostDisplayMode::RealOnly,
    );
}

/// Handle aggregate commands across every registered data source.
pub(crate) fn handle_all_sources_command(command: SourceCommand, ctx: &CommandContext<'_>) {
    match command {
        SourceCommand::Sources => return crate::sources_cmd::handle_sources(ctx),
        SourceCommand::Quota => {
            eprintln!("Error: quota analysis only supports the Codex source");
            std::process::exit(1);
        }
        SourceCommand::Statusline => {
            let (result, caps) = load_all_daily(ctx, true);
            if ctx.cli.json {
                let json = print_statusline_json_with_quality(
                    &result.day_stats,
                    ctx.pricing_db,
                    "All Sources",
                    ctx.number_format,
                    ctx.currency,
                    caps.has_cache_read,
                    Some(result.data_quality()),
                    CostDisplayMode::RealOnly,
                );
                print_json(&json, ctx.jq_filter);
            } else {
                print_statusline(
                    &result.day_stats,
                    ctx.pricing_db,
                    "All Sources",
                    ctx.number_format,
                    ctx.currency,
                    caps.has_cache_read,
                    CostDisplayMode::RealOnly,
                );
            }
            return;
        }
        SourceCommand::Top { dim, limit } => {
            let limit = match validate_top_limit(limit) {
                Ok(l) => l,
                Err(msg) => {
                    eprintln!("Error: {msg}");
                    std::process::exit(1);
                }
            };
            // Project ranking with --source all would require a unified
            // project view across sources, which we do not aggregate today.
            // Fall back to model ranking which works on the merged daily map.
            if dim == TopDimension::Project {
                println!(
                    "`--source all` does not support `top --dim project`.\nHint: pick a specific --source (e.g. claude) for project ranking."
                );
                return;
            }
            let (result, caps) = load_all_daily(ctx, false);
            let rows = rank_by_model_with_cost_mode(
                &result.day_stats,
                ctx.pricing_db,
                CostDisplayMode::RealOnly,
            );
            handle_top(
                &rows,
                dim,
                limit,
                TopRenderOptions {
                    source_label: "All Sources",
                    supports_cache_read: caps.has_cache_read,
                    codex_scope: None,
                    cost_mode: CostDisplayMode::RealOnly,
                },
                ctx,
            );
            return;
        }
        SourceCommand::Session
        | SourceCommand::Project
        | SourceCommand::Blocks
        | SourceCommand::Endpoints
        | SourceCommand::Tools => {
            println!(
                "`--source all` supports daily, weekly, monthly, today, statusline, and top views.\nHint: use a specific --source for {command:?}."
            );
            return;
        }
        SourceCommand::Daily
        | SourceCommand::Today
        | SourceCommand::Weekly
        | SourceCommand::Monthly => {}
    }

    handle_all_period(command, ctx);
}
