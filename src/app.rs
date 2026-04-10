use crate::cli::{Cli, SourceCommand};
use crate::core::{DateFilter, aggregate_tools};
use crate::output::NumberFormat;
use crate::output::{
    BlockTableOptions, Period, ProjectTableOptions, SessionTableOptions, SummaryOptions,
    TokenTableOptions, output_block_csv, output_block_json, output_period_csv, output_period_json,
    output_project_csv, output_project_json, output_session_csv, output_session_json,
    output_tools_csv, output_tools_json, print_block_table, print_period_table,
    print_project_table, print_session_table, print_statusline, print_statusline_json,
    print_tools_table,
};
use crate::pricing::PricingDb;
use crate::source::{
    Source, load_blocks, load_daily, load_projects, load_sessions, load_tool_calls,
};
use crate::utils::{Timezone, filter_json};

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
}

fn print_no_data_hint(source_name: &str, category: &str) {
    println!(
        "No {source_name} {category} data found in the selected date range.\nHint: widen --since/--until, try `today`, or switch --source (claude/codex)."
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
    let summary = aggregate_tools(calls);
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

/// Handle commands for a specific data source
pub(crate) fn handle_source_command(
    source: &dyn Source,
    command: SourceCommand,
    ctx: &CommandContext<'_>,
) {
    let caps = source.capabilities();

    // Non-period commands: dispatch and return early
    match command {
        SourceCommand::Session => return handle_session(source, ctx),
        SourceCommand::Project => {
            if !caps.has_projects {
                println!(
                    "{} does not support project aggregation.",
                    source.display_name()
                );
                return;
            }
            return handle_project(source, ctx);
        }
        SourceCommand::Blocks => {
            if !caps.has_billing_blocks {
                println!(
                    "{} does not support billing block aggregation.",
                    source.display_name()
                );
                return;
            }
            return handle_blocks(source, ctx);
        }
        SourceCommand::Statusline => return handle_statusline(source, ctx),
        SourceCommand::Tools => {
            if source.name() != "claude" {
                println!("Tool usage analysis is only supported for Claude source.");
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
    let period = match command {
        SourceCommand::Daily | SourceCommand::Today => Period::Day,
        SourceCommand::Weekly => Period::Week,
        SourceCommand::Monthly => Period::Month,
        // All non-period variants handled above and returned early
        _ => return,
    };

    let result = load_daily(source, ctx.filter, ctx.timezone, false, ctx.cli.debug);
    if result.day_stats.is_empty() {
        print_no_data_hint(source.display_name(), "usage");
        return;
    }
    if ctx.cli.csv {
        let csv = output_period_csv(
            &result.day_stats,
            period,
            ctx.pricing_db,
            ctx.cli.sort_order(),
            ctx.cli.breakdown,
            ctx.cli.show_cost(),
        );
        print!("{csv}");
    } else if ctx.cli.json {
        let json = output_period_json(
            &result.day_stats,
            period,
            ctx.pricing_db,
            ctx.cli.sort_order(),
            ctx.cli.breakdown,
            ctx.cli.show_cost(),
            ctx.currency,
        );
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
    }
}
