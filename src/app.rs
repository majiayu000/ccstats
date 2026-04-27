use std::collections::HashMap;
use std::time::Instant;

use crate::cli::{Cli, SourceCommand};
use crate::core::{DateFilter, DayStats, LoadResult, aggregate_tools};
use crate::output::NumberFormat;
use crate::output::{
    BlockTableOptions, MonthlyBudgetOptions, Period, ProjectTableOptions, SessionTableOptions,
    SummaryOptions, TokenTableOptions, add_monthly_budget_to_json, monthly_budget_reports,
    output_block_csv, output_block_json, output_monthly_budget_csv, output_period_csv,
    output_period_json, output_project_csv, output_project_json, output_session_csv,
    output_session_json, output_tools_csv, output_tools_json, print_block_table,
    print_monthly_budget_table, print_period_table, print_project_table, print_session_table,
    print_statusline, print_statusline_json, print_tools_table,
};
use crate::pricing::PricingDb;
use crate::source::{
    ALL_SOURCES, Capabilities, Source, all_sources, load_blocks, load_daily, load_projects,
    load_sessions, load_tool_calls,
};
use crate::utils::{Timezone, filter_json};
use serde_json::json;

/// Print JSON output, optionally filtering through jq
fn print_json(json: &str, jq_filter: Option<&str>) {
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

fn print_no_data_hint(source_name: &str, category: &str) {
    println!(
        "No {source_name} {category} data found in the selected date range.\nHint: widen --since/--until, try `today`, or run `ccstats sources` to pick a different --source."
    );
}

fn handle_session(source: &dyn Source, ctx: &CommandContext<'_>) {
    let sessions = load_sessions(source, ctx.filter, ctx.timezone, false);
    if sessions.is_empty() {
        print_no_data_hint(source.display_name(), "session");
        return;
    }
    if ctx.cli.csv {
        let csv = output_session_csv(
            &sessions,
            ctx.pricing_db,
            ctx.cli.sort_order(),
            ctx.cli.show_cost(),
        );
        print!("{csv}");
    } else if ctx.cli.json {
        let json = output_session_json(
            &sessions,
            ctx.pricing_db,
            ctx.cli.sort_order(),
            ctx.cli.show_cost(),
            ctx.currency,
        );
        print_json(&json, ctx.jq_filter);
    } else {
        print_session_table(
            &sessions,
            ctx.pricing_db,
            SessionTableOptions {
                order: ctx.cli.sort_order(),
                use_color: ctx.cli.use_color(),
                compact: ctx.cli.compact,
                show_cost: ctx.cli.show_cost(),
                number_format: ctx.number_format,
                source_label: source.display_name(),
                timezone: ctx.timezone,
                currency: ctx.currency,
            },
        );
    }
}

fn handle_project(source: &dyn Source, ctx: &CommandContext<'_>) {
    let projects = load_projects(source, ctx.filter, ctx.timezone, false);
    if projects.is_empty() {
        print_no_data_hint(source.display_name(), "project");
        return;
    }
    if ctx.cli.csv {
        let csv = output_project_csv(
            &projects,
            ctx.pricing_db,
            ctx.cli.sort_order(),
            ctx.cli.show_cost(),
        );
        print!("{csv}");
    } else if ctx.cli.json {
        let json = output_project_json(
            &projects,
            ctx.pricing_db,
            ctx.cli.sort_order(),
            ctx.cli.show_cost(),
            ctx.currency,
        );
        print_json(&json, ctx.jq_filter);
    } else {
        print_project_table(
            &projects,
            ctx.pricing_db,
            ProjectTableOptions {
                order: ctx.cli.sort_order(),
                use_color: ctx.cli.use_color(),
                compact: ctx.cli.compact,
                show_cost: ctx.cli.show_cost(),
                source_label: source.display_name(),
                number_format: ctx.number_format,
                currency: ctx.currency,
            },
        );
    }
}

fn handle_blocks(source: &dyn Source, ctx: &CommandContext<'_>) {
    let blocks = load_blocks(source, ctx.filter, ctx.timezone, false);
    if blocks.is_empty() {
        print_no_data_hint(source.display_name(), "billing block");
        return;
    }
    if ctx.cli.csv {
        let csv = output_block_csv(
            &blocks,
            ctx.pricing_db,
            ctx.cli.sort_order(),
            ctx.cli.show_cost(),
        );
        print!("{csv}");
    } else if ctx.cli.json {
        let json = output_block_json(
            &blocks,
            ctx.pricing_db,
            ctx.cli.sort_order(),
            ctx.cli.show_cost(),
            ctx.currency,
        );
        print_json(&json, ctx.jq_filter);
    } else {
        print_block_table(
            &blocks,
            ctx.pricing_db,
            BlockTableOptions {
                order: ctx.cli.sort_order(),
                use_color: ctx.cli.use_color(),
                compact: ctx.cli.compact,
                show_cost: ctx.cli.show_cost(),
                source_label: source.display_name(),
                number_format: ctx.number_format,
                currency: ctx.currency,
            },
        );
    }
}

fn handle_tools(ctx: &CommandContext<'_>) {
    let calls = load_tool_calls(ctx.filter, ctx.timezone);
    let summary = aggregate_tools(&calls);
    if ctx.cli.csv {
        let csv = output_tools_csv(&summary);
        print!("{csv}");
    } else if ctx.cli.json {
        let json = output_tools_json(&summary);
        print_json(&json, ctx.jq_filter);
    } else {
        print_tools_table(&summary, ctx.cli.use_color());
    }
}

fn handle_sources(ctx: &CommandContext<'_>) {
    let sources: Vec<&dyn Source> = all_sources().collect();
    let mut all_caps = all_sources_capabilities();
    all_caps.has_projects = false;
    all_caps.has_billing_blocks = false;

    if ctx.cli.csv {
        println!("name,display_name,aliases,has_projects,has_billing_blocks,has_reasoning_tokens");
        println!(
            "{},All Sources,,{},{},{}",
            ALL_SOURCES,
            all_caps.has_projects,
            all_caps.has_billing_blocks,
            all_caps.has_reasoning_tokens
        );
        for source in sources {
            let caps = source.capabilities();
            let aliases = source.aliases().join("|");
            println!(
                "{},{},{},{},{},{}",
                source.name(),
                source.display_name(),
                aliases,
                caps.has_projects,
                caps.has_billing_blocks,
                caps.has_reasoning_tokens
            );
        }
        return;
    }

    if ctx.cli.json {
        let mut payload = vec![json!({
            "name": ALL_SOURCES,
            "display_name": "All Sources",
            "aliases": [],
            "capabilities": {
                "has_projects": all_caps.has_projects,
                "has_billing_blocks": all_caps.has_billing_blocks,
                "has_reasoning_tokens": all_caps.has_reasoning_tokens,
                "has_cache_creation": all_caps.has_cache_creation,
                "needs_dedup": all_caps.needs_dedup
            }
        })];
        payload.extend(sources.iter().map(|source| {
            let caps = source.capabilities();
            json!({
                "name": source.name(),
                "display_name": source.display_name(),
                "aliases": source.aliases(),
                "capabilities": {
                    "has_projects": caps.has_projects,
                    "has_billing_blocks": caps.has_billing_blocks,
                    "has_reasoning_tokens": caps.has_reasoning_tokens,
                    "has_cache_creation": caps.has_cache_creation,
                    "needs_dedup": caps.needs_dedup
                }
            })
        }));
        let json = serde_json::to_string(&payload).unwrap_or_else(|_| "[]".to_string());
        print_json(&json, ctx.jq_filter);
        return;
    }

    println!("Available sources:");
    println!(
        "- {} (All Sources) aliases: - | projects={} blocks={} reasoning={}",
        ALL_SOURCES,
        all_caps.has_projects,
        all_caps.has_billing_blocks,
        all_caps.has_reasoning_tokens
    );
    for source in sources {
        let caps = source.capabilities();
        let aliases = if source.aliases().is_empty() {
            "-".to_string()
        } else {
            source.aliases().join(", ")
        };
        println!(
            "- {} ({}) aliases: {} | projects={} blocks={} reasoning={}",
            source.name(),
            source.display_name(),
            aliases,
            caps.has_projects,
            caps.has_billing_blocks,
            caps.has_reasoning_tokens
        );
    }
    println!("Hint: use `--source <name|alias>` (e.g. `--source codex` or `--source cx`).");
}

fn handle_statusline(source: &dyn Source, ctx: &CommandContext<'_>) {
    let result = load_daily(source, ctx.filter, ctx.timezone, true, false);
    if ctx.cli.json {
        let json = print_statusline_json(
            &result.day_stats,
            ctx.pricing_db,
            source.display_name(),
            ctx.number_format,
        );
        print_json(&json, ctx.jq_filter);
    } else {
        print_statusline(
            &result.day_stats,
            ctx.pricing_db,
            source.display_name(),
            ctx.number_format,
        );
    }
}

fn render_period_result(
    result: &LoadResult,
    period: Period,
    caps: &Capabilities,
    ctx: &CommandContext<'_>,
) {
    let monthly_budget = (period == Period::Month)
        .then_some(ctx.cli.monthly_budget)
        .flatten();

    if ctx.cli.csv {
        let csv = if let Some(budget) = monthly_budget {
            output_monthly_budget_csv(
                &result.day_stats,
                ctx.pricing_db,
                MonthlyBudgetOptions {
                    order: ctx.cli.sort_order(),
                    breakdown: ctx.cli.breakdown,
                    show_cost: ctx.cli.show_cost(),
                    limit: budget,
                    as_of: ctx.budget_as_of,
                    currency: ctx.currency,
                },
            )
        } else {
            output_period_csv(
                &result.day_stats,
                period,
                ctx.pricing_db,
                ctx.cli.sort_order(),
                ctx.cli.breakdown,
                ctx.cli.show_cost(),
            )
        };
        print!("{csv}");
    } else if ctx.cli.json {
        let mut json = output_period_json(
            &result.day_stats,
            period,
            ctx.pricing_db,
            ctx.cli.sort_order(),
            ctx.cli.breakdown,
            ctx.cli.show_cost(),
            ctx.currency,
        );
        if let Some(budget) = monthly_budget {
            let reports = monthly_budget_reports(
                &result.day_stats,
                ctx.pricing_db,
                ctx.cli.sort_order(),
                budget,
                ctx.budget_as_of,
                ctx.currency,
            );
            json = add_monthly_budget_to_json(&json, &reports);
        }
        print_json(&json, ctx.jq_filter);
    } else {
        print_period_table(
            &result.day_stats,
            period,
            ctx.cli.breakdown,
            SummaryOptions {
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
                currency: ctx.currency,
            },
        );
        if let Some(budget) = monthly_budget {
            let reports = monthly_budget_reports(
                &result.day_stats,
                ctx.pricing_db,
                ctx.cli.sort_order(),
                budget,
                ctx.budget_as_of,
                ctx.currency,
            );
            print_monthly_budget_table(&reports, ctx.cli.use_color(), ctx.currency);
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
    if result.day_stats.is_empty() {
        print_no_data_hint(source.display_name(), "usage");
        return;
    }
    render_period_result(&result, period, caps, ctx);
}

/// Handle commands for a specific data source
pub(crate) fn handle_source_command(
    source: &dyn Source,
    command: SourceCommand,
    ctx: &CommandContext<'_>,
) {
    let caps = source.capabilities();

    // Non-period commands: dispatch and return early
    match command {
        SourceCommand::Sources => return handle_sources(ctx),
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
        SourceCommand::Statusline => return handle_statusline(source, ctx),
        SourceCommand::Tools => {
            if source.name() != "claude" {
                println!(
                    "Tool usage analysis is only supported for Claude source.\nHint: switch with `--source claude` (or alias `--source cc`)."
                );
                return;
            }
            return handle_tools(ctx);
        }
        SourceCommand::Daily
        | SourceCommand::Today
        | SourceCommand::Weekly
        | SourceCommand::Monthly => {}
    }

    // Period-based commands: Daily/Today/Weekly/Monthly
    handle_period(source, command, &caps, ctx);
}

fn all_sources_capabilities() -> Capabilities {
    let mut combined = Capabilities::default();
    for source in all_sources() {
        let caps = source.capabilities();
        combined.has_projects |= caps.has_projects;
        combined.has_billing_blocks |= caps.has_billing_blocks;
        combined.has_reasoning_tokens |= caps.has_reasoning_tokens;
        combined.has_cache_creation |= caps.has_cache_creation;
        combined.needs_dedup |= caps.needs_dedup;
    }
    combined
}

fn merge_day_stats(target: &mut HashMap<String, DayStats>, source: HashMap<String, DayStats>) {
    for (date, stats) in source {
        let day = target.entry(date).or_default();
        day.stats.add(&stats.stats);
        for (model, model_stats) in stats.models {
            day.models.entry(model).or_default().add(&model_stats);
        }
    }
}

fn load_all_daily(ctx: &CommandContext<'_>, quiet: bool) -> (LoadResult, Capabilities) {
    let start = Instant::now();
    let mut combined = LoadResult::default();
    let mut caps = Capabilities::default();

    for source in all_sources() {
        let source_caps = source.capabilities();
        caps.has_projects |= source_caps.has_projects;
        caps.has_billing_blocks |= source_caps.has_billing_blocks;
        caps.has_reasoning_tokens |= source_caps.has_reasoning_tokens;
        caps.has_cache_creation |= source_caps.has_cache_creation;
        caps.needs_dedup |= source_caps.needs_dedup;

        let result = load_daily(source, ctx.filter, ctx.timezone, quiet, ctx.cli.debug);
        combined.skipped += result.skipped;
        combined.valid += result.valid;
        merge_day_stats(&mut combined.day_stats, result.day_stats);
    }

    combined.elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
    (combined, caps)
}

/// Handle aggregate commands across every registered data source.
pub(crate) fn handle_all_sources_command(command: SourceCommand, ctx: &CommandContext<'_>) {
    match command {
        SourceCommand::Sources => return handle_sources(ctx),
        SourceCommand::Statusline => {
            let (result, _) = load_all_daily(ctx, true);
            if ctx.cli.json {
                let json = print_statusline_json(
                    &result.day_stats,
                    ctx.pricing_db,
                    "All Sources",
                    ctx.number_format,
                );
                print_json(&json, ctx.jq_filter);
            } else {
                print_statusline(
                    &result.day_stats,
                    ctx.pricing_db,
                    "All Sources",
                    ctx.number_format,
                );
            }
            return;
        }
        SourceCommand::Session
        | SourceCommand::Project
        | SourceCommand::Blocks
        | SourceCommand::Tools => {
            println!(
                "`--source all` supports daily, weekly, monthly, today, and statusline views.\nHint: use a specific --source for {command:?}."
            );
            return;
        }
        SourceCommand::Daily
        | SourceCommand::Today
        | SourceCommand::Weekly
        | SourceCommand::Monthly => {}
    }

    let period = match command {
        SourceCommand::Daily | SourceCommand::Today => Period::Day,
        SourceCommand::Weekly => Period::Week,
        SourceCommand::Monthly => Period::Month,
        _ => return,
    };

    let (result, caps) = load_all_daily(ctx, false);
    if result.day_stats.is_empty() {
        print_no_data_hint("All Sources", "usage");
        return;
    }
    render_period_result(&result, period, &caps, ctx);
}
