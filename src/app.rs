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

/// Handle commands for a specific data source
pub(crate) fn handle_source_command(
    source: &dyn Source,
    command: SourceCommand,
    ctx: CommandContext<'_>,
) {
    let filter = ctx.filter;
    let cli = ctx.cli;
    let pricing_db = ctx.pricing_db;
    let timezone = ctx.timezone;
    let number_format = ctx.number_format;
    let jq_filter = ctx.jq_filter;

    let caps = source.capabilities();
    let show_reasoning = caps.has_reasoning_tokens;
    let show_cache_creation = caps.has_cache_creation;
    let is_statusline = matches!(command, SourceCommand::Statusline);
    let quiet = is_statusline;
    let order = cli.sort_order();

    match command {
        SourceCommand::Session => {
            let sessions = load_sessions(source, filter, timezone, quiet);
            if sessions.is_empty() {
                println!("No {} session data found.", source.display_name());
                return;
            }
            let use_color = cli.use_color();
            let show_cost = cli.show_cost();
            if cli.json {
                let json = output_session_json(&sessions, pricing_db, order, show_cost);
                print_json(&json, jq_filter);
            } else {
                print_session_table(
                    &sessions,
                    pricing_db,
                    SessionTableOptions {
                        order,
                        use_color,
                        compact: cli.compact,
                        show_cost,
                        number_format,
                        source_label: source.display_name(),
                        timezone,
                    },
                );
            }
            return;
        }
        SourceCommand::Project => {
            if !caps.has_projects {
                println!(
                    "{} does not support project aggregation.",
                    source.display_name()
                );
                return;
            }
            let projects = load_projects(source, filter, timezone, quiet);
            if projects.is_empty() {
                println!("No {} project data found.", source.display_name());
                return;
            }
            let use_color = cli.use_color();
            let show_cost = cli.show_cost();
            if cli.json {
                let json = output_project_json(&projects, pricing_db, order, show_cost);
                print_json(&json, jq_filter);
            } else {
                print_project_table(
                    &projects,
                    pricing_db,
                    ProjectTableOptions {
                        order,
                        use_color,
                        compact: cli.compact,
                        show_cost,
                        source_label: source.display_name(),
                        number_format,
                    },
                );
            }
            return;
        }
        SourceCommand::Blocks => {
            if !caps.has_billing_blocks {
                println!(
                    "{} does not support billing block aggregation.",
                    source.display_name()
                );
                return;
            }
            let blocks = load_blocks(source, filter, timezone, quiet);
            if blocks.is_empty() {
                println!("No {} block data found.", source.display_name());
                return;
            }
            let use_color = cli.use_color();
            let show_cost = cli.show_cost();
            if cli.json {
                let json = output_block_json(&blocks, pricing_db, order, show_cost);
                print_json(&json, jq_filter);
            } else {
                print_block_table(
                    &blocks,
                    pricing_db,
                    BlockTableOptions {
                        order,
                        use_color,
                        compact: cli.compact,
                        show_cost,
                        source_label: source.display_name(),
                        number_format,
                    },
                );
            }
            return;
        }
        SourceCommand::Statusline => {
            let result = load_daily(source, filter, timezone, true, false);
            if cli.json {
                let json = print_statusline_json(
                    &result.day_stats,
                    pricing_db,
                    source.display_name(),
                    number_format,
                );
                print_json(&json, jq_filter);
            } else {
                print_statusline(
                    &result.day_stats,
                    pricing_db,
                    source.display_name(),
                    number_format,
                );
            }
            return;
        }
        _ => {}
    }

    // Period-based commands: Daily/Today/Weekly/Monthly
    let period = match command {
        SourceCommand::Daily | SourceCommand::Today => Period::Day,
        SourceCommand::Weekly => Period::Week,
        SourceCommand::Monthly => Period::Month,
        _ => unreachable!(),
    };

    let result = load_daily(source, filter, timezone, quiet, cli.debug);
    if result.day_stats.is_empty() {
        println!("No {} data found.", source.display_name());
        return;
    }
    let use_color = cli.use_color();
    let show_cost = cli.show_cost();
    if cli.json {
        let json = output_period_json(
            &result.day_stats,
            period,
            pricing_db,
            order,
            cli.breakdown,
            show_cost,
        );
        print_json(&json, jq_filter);
    } else {
        print_period_table(
            &result.day_stats,
            period,
            cli.breakdown,
            SummaryOptions {
                skipped: result.skipped,
                valid: result.valid,
                elapsed_ms: Some(result.elapsed_ms),
            },
            pricing_db,
            TokenTableOptions {
                order,
                use_color,
                compact: cli.compact,
                show_cost,
                number_format,
                show_reasoning,
                show_cache_creation,
            },
        );
    }
}
