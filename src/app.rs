use crate::cli::{Cli, SourceCommand};
use crate::core::DateFilter;
use crate::output::NumberFormat;
use crate::output::{
    output_block_json, output_daily_json, output_monthly_json, output_project_json,
    output_session_json, output_weekly_json, print_block_table, print_daily_table,
    print_monthly_table, print_project_table, print_session_table, print_statusline,
    print_statusline_json, print_weekly_table,
};
use crate::pricing::PricingDb;
use crate::source::{load_blocks, load_daily, load_projects, load_sessions, Source};
use crate::utils::{filter_json, Timezone};

/// Print JSON output, optionally filtering through jq
fn print_json(json: &str, jq_filter: Option<&str>) {
    match jq_filter {
        Some(filter) => match filter_json(json, filter) {
            Ok(filtered) => print!("{}", filtered),
            Err(e) => {
                eprintln!("{}", e);
                std::process::exit(1);
            }
        },
        None => println!("{}", json),
    }
}

/// Handle commands for a specific data source
pub(crate) fn handle_source_command(
    source: &dyn Source,
    command: &SourceCommand,
    filter: &DateFilter,
    cli: &Cli,
    pricing_db: &PricingDb,
    timezone: &Timezone,
    number_format: NumberFormat,
    jq_filter: Option<&str>,
) {
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
                    order,
                    use_color,
                    cli.compact,
                    show_cost,
                    number_format,
                    source.display_name(),
                    timezone,
                );
            }
        }
        SourceCommand::Project => {
            if !caps.has_projects {
                println!("{} does not support project aggregation.", source.display_name());
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
                    order,
                    use_color,
                    cli.compact,
                    show_cost,
                    source.display_name(),
                    number_format,
                );
            }
        }
        SourceCommand::Blocks => {
            if !caps.has_billing_blocks {
                println!("{} does not support billing block aggregation.", source.display_name());
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
                    order,
                    use_color,
                    cli.compact,
                    show_cost,
                    source.display_name(),
                    number_format,
                );
            }
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
        }
        SourceCommand::Daily | SourceCommand::Today => {
            let result = load_daily(source, filter, timezone, quiet, cli.debug);
            if result.day_stats.is_empty() {
                println!("No {} data found.", source.display_name());
                return;
            }
            let use_color = cli.use_color();
            let show_cost = cli.show_cost();
            if cli.json {
                let json = output_daily_json(
                    &result.day_stats,
                    pricing_db,
                    order,
                    cli.breakdown,
                    show_cost,
                );
                print_json(&json, jq_filter);
            } else {
                print_daily_table(
                    &result.day_stats,
                    cli.breakdown,
                    result.skipped,
                    result.valid,
                    pricing_db,
                    order,
                    use_color,
                    cli.compact,
                    show_cost,
                    number_format,
                    show_reasoning,
                    show_cache_creation,
                    Some(result.elapsed_ms),
                );
            }
        }
        SourceCommand::Weekly => {
            let result = load_daily(source, filter, timezone, quiet, cli.debug);
            if result.day_stats.is_empty() {
                println!("No {} data found.", source.display_name());
                return;
            }
            let use_color = cli.use_color();
            let show_cost = cli.show_cost();
            if cli.json {
                let json = output_weekly_json(
                    &result.day_stats,
                    pricing_db,
                    order,
                    cli.breakdown,
                    show_cost,
                );
                print_json(&json, jq_filter);
            } else {
                print_weekly_table(
                    &result.day_stats,
                    cli.breakdown,
                    result.skipped,
                    result.valid,
                    pricing_db,
                    order,
                    use_color,
                    cli.compact,
                    show_cost,
                    number_format,
                    show_reasoning,
                    show_cache_creation,
                    Some(result.elapsed_ms),
                );
            }
        }
        SourceCommand::Monthly => {
            let result = load_daily(source, filter, timezone, quiet, cli.debug);
            if result.day_stats.is_empty() {
                println!("No {} data found.", source.display_name());
                return;
            }
            let use_color = cli.use_color();
            let show_cost = cli.show_cost();
            if cli.json {
                let json = output_monthly_json(
                    &result.day_stats,
                    pricing_db,
                    order,
                    cli.breakdown,
                    show_cost,
                );
                print_json(&json, jq_filter);
            } else {
                print_monthly_table(
                    &result.day_stats,
                    cli.breakdown,
                    result.skipped,
                    result.valid,
                    pricing_db,
                    order,
                    use_color,
                    cli.compact,
                    show_cost,
                    number_format,
                    show_reasoning,
                    show_cache_creation,
                    Some(result.elapsed_ms),
                );
            }
        }
    }
}
