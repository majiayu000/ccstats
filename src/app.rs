use crate::cli::{Cli, SourceCommand};
use crate::core::DateFilter;
use crate::output::NumberFormat;
use crate::output::{
    BlockTableOptions, Period, ProjectTableOptions, SessionTableOptions, SummaryOptions,
    TokenTableOptions, output_block_json, output_period_json, output_project_json,
    output_session_json, print_block_table, print_period_table, print_project_table,
    print_session_table, print_statusline, print_statusline_json,
};
use crate::pricing::PricingDb;
use crate::source::{Source, load_blocks, load_daily, load_projects, load_sessions};
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
}

fn handle_session(source: &dyn Source, ctx: &CommandContext<'_>) {
    let sessions = load_sessions(source, ctx.filter, ctx.timezone, false);
    if sessions.is_empty() {
        println!("No {} session data found.", source.display_name());
        return;
    }
    if ctx.cli.json {
        let json = output_session_json(
            &sessions,
            ctx.pricing_db,
            ctx.cli.sort_order(),
            ctx.cli.show_cost(),
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
            },
        );
    }
}

fn handle_project(source: &dyn Source, ctx: &CommandContext<'_>) {
    let projects = load_projects(source, ctx.filter, ctx.timezone, false);
    if projects.is_empty() {
        println!("No {} project data found.", source.display_name());
        return;
    }
    if ctx.cli.json {
        let json = output_project_json(
            &projects,
            ctx.pricing_db,
            ctx.cli.sort_order(),
            ctx.cli.show_cost(),
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
            },
        );
    }
}

fn handle_blocks(source: &dyn Source, ctx: &CommandContext<'_>) {
    let blocks = load_blocks(source, ctx.filter, ctx.timezone, false);
    if blocks.is_empty() {
        println!("No {} block data found.", source.display_name());
        return;
    }
    if ctx.cli.json {
        let json = output_block_json(
            &blocks,
            ctx.pricing_db,
            ctx.cli.sort_order(),
            ctx.cli.show_cost(),
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
            },
        );
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
        _ => {}
    }

    // Period-based commands: Daily/Today/Weekly/Monthly
    let period = match command {
        SourceCommand::Daily | SourceCommand::Today => Period::Day,
        SourceCommand::Weekly => Period::Week,
        SourceCommand::Monthly => Period::Month,
        _ => unreachable!(),
    };

    let result = load_daily(source, ctx.filter, ctx.timezone, false, ctx.cli.debug);
    if result.day_stats.is_empty() {
        println!("No {} data found.", source.display_name());
        return;
    }
    if ctx.cli.json {
        let json = output_period_json(
            &result.day_stats,
            period,
            ctx.pricing_db,
            ctx.cli.sort_order(),
            ctx.cli.breakdown,
            ctx.cli.show_cost(),
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
            },
        );
    }
}
