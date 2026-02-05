mod cli;
mod config;
mod core;
mod output;
mod pricing;
mod source;
mod utils;

use chrono::Utc;
use clap::Parser;

use cli::{parse_command, Cli, SourceCommand};
use config::Config;
use core::DateFilter;
use output::format::NumberFormat;
use output::{
    output_block_json, output_daily_json, output_monthly_json, output_project_json,
    output_session_json, output_weekly_json, print_block_table, print_daily_table,
    print_monthly_table, print_project_table, print_session_table, print_statusline,
    print_statusline_json, print_weekly_table,
};
use pricing::PricingDb;
use source::{get_source, load_blocks, load_daily, load_projects, load_sessions, Source};
use utils::{filter_json, parse_date, set_parse_debug, Timezone};

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
fn handle_source_command(
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
    let is_statusline = matches!(command, SourceCommand::Statusline);
    let quiet = is_statusline;

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
                let json = output_session_json(&sessions, pricing_db, cli.order, show_cost);
                print_json(&json, jq_filter);
            } else {
                print_session_table(
                    &sessions,
                    pricing_db,
                    cli.order,
                    use_color,
                    cli.compact,
                    show_cost,
                    number_format,
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
                let json = output_project_json(&projects, pricing_db, cli.order, show_cost);
                print_json(&json, jq_filter);
            } else {
                print_project_table(
                    &projects,
                    pricing_db,
                    cli.order,
                    use_color,
                    cli.compact,
                    show_cost,
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
                let json = output_block_json(&blocks, pricing_db, cli.order, show_cost);
                print_json(&json, jq_filter);
            } else {
                print_block_table(
                    &blocks,
                    pricing_db,
                    cli.order,
                    use_color,
                    cli.compact,
                    show_cost,
                    number_format,
                );
            }
        }
        SourceCommand::Statusline => {
            let result = load_daily(source, filter, timezone, true, false);
            if cli.json {
                let json = print_statusline_json(&result.day_stats, pricing_db, number_format);
                print_json(&json, jq_filter);
            } else {
                print_statusline(&result.day_stats, pricing_db, number_format);
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
                    cli.order,
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
                    cli.order,
                    use_color,
                    cli.compact,
                    show_cost,
                    number_format,
                    show_reasoning,
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
                    cli.order,
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
                    cli.order,
                    use_color,
                    cli.compact,
                    show_cost,
                    number_format,
                    show_reasoning,
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
                    cli.order,
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
                    cli.order,
                    use_color,
                    cli.compact,
                    show_cost,
                    number_format,
                    show_reasoning,
                    Some(result.elapsed_ms),
                );
            }
        }
    }
}

fn main() {
    // Parse CLI and extract source command
    let raw_cli = Cli::parse();
    let raw_timezone = raw_cli.timezone.clone();
    let (is_codex, source_cmd) = parse_command(&raw_cli.command);
    let is_statusline = source_cmd.is_statusline();

    // Load config file (quiet for statusline)
    let config = if is_statusline {
        Config::load_quiet()
    } else {
        Config::load()
    };

    // Merge config with CLI (CLI takes precedence)
    let cli = raw_cli.with_config(&config);
    set_parse_debug(cli.debug);

    enum TimezoneSource {
        Cli,
        Config,
    }

    let timezone_source = if raw_timezone.is_some() {
        Some(TimezoneSource::Cli)
    } else if cli.timezone.is_some() {
        Some(TimezoneSource::Config)
    } else {
        None
    };

    let timezone = match Timezone::parse(cli.timezone.as_deref()) {
        Ok(tz) => tz,
        Err(err) => match timezone_source {
            Some(TimezoneSource::Config) => {
                eprintln!("Warning: {}. Falling back to local timezone.", err);
                Timezone::Local
            }
            _ => {
                eprintln!("{}", err);
                std::process::exit(1);
            }
        },
    };

    let number_format = match NumberFormat::from_locale(cli.locale.as_deref()) {
        Ok(format) => format,
        Err(err) => {
            eprintln!("Warning: {}. Using default locale.", err);
            NumberFormat::default()
        }
    };

    let jq_filter = cli.jq.as_deref();

    let since = match cli.since.as_ref() {
        Some(s) => match parse_date(s) {
            Ok(date) => Some(date),
            Err(err) => {
                eprintln!("--since: {}", err);
                std::process::exit(1);
            }
        },
        None => None,
    };
    let until = match cli.until.as_ref() {
        Some(s) => match parse_date(s) {
            Ok(date) => Some(date),
            Err(err) => {
                eprintln!("--until: {}", err);
                std::process::exit(1);
            }
        },
        None => None,
    };

    // For "today" and "statusline" commands, set since/until to today
    let filter = if source_cmd.needs_today_filter() {
        let today = timezone.to_fixed_offset(Utc::now()).date_naive();
        DateFilter::new(Some(today), Some(today))
    } else {
        DateFilter::new(since, until)
    };

    // Load pricing database (quiet mode for statusline)
    let pricing_db = if is_statusline {
        PricingDb::load_quiet(cli.offline)
    } else {
        PricingDb::load(cli.offline)
    };

    // Get the appropriate data source
    let source_name = if is_codex { "codex" } else { "claude" };
    let source = get_source(source_name).expect(&format!("{} source not found", source_name));

    handle_source_command(
        source,
        &source_cmd,
        &filter,
        &cli,
        &pricing_db,
        &timezone,
        number_format,
        jq_filter,
    );
}
