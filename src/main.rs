mod cli;
mod config;
mod data;
mod output;
mod pricing;
mod utils;

use chrono::Utc;
use clap::Parser;

use cli::{Cli, CodexCommands, Commands};
use config::Config;
use data::codex::{load_codex_session_data, load_codex_usage_data};
use data::{
    load_block_data, load_project_data, load_session_data, load_usage_data_quiet,
    load_usage_data_with_debug,
};
use output::format::NumberFormat;
use output::{
    output_block_json, output_daily_json, output_monthly_json, output_project_json,
    output_session_json, output_weekly_json, print_block_table, print_daily_table,
    print_monthly_table, print_project_table, print_session_table, print_statusline,
    print_statusline_json, print_weekly_table,
};
use pricing::PricingDb;
use utils::{filter_json, parse_date, Timezone};

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

/// Check if command is a statusline variant (for quiet mode)
fn is_statusline_command(command: &Option<Commands>) -> bool {
    match command {
        Some(Commands::Statusline) => true,
        Some(Commands::Codex { command }) => matches!(command, Some(CodexCommands::Statusline)),
        _ => false,
    }
}

fn main() {
    // Load config file (quiet for statusline)
    let raw_cli = Cli::parse();
    let raw_timezone = raw_cli.timezone.clone();
    let is_statusline = is_statusline_command(&raw_cli.command);

    let config = if is_statusline {
        Config::load_quiet()
    } else {
        Config::load()
    };

    // Merge config with CLI (CLI takes precedence)
    let cli = raw_cli.with_config(&config);

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

    let since = cli.since.as_ref().and_then(|s| parse_date(s));
    let until = cli.until.as_ref().and_then(|s| parse_date(s));

    // For "today" and "statusline" commands, set since/until to today
    let (since, until) = match &cli.command {
        Some(Commands::Today) | Some(Commands::Statusline) => {
            let today = timezone.to_fixed_offset(Utc::now()).date_naive();
            (Some(today), Some(today))
        }
        Some(Commands::Codex { command }) => match command {
            Some(CodexCommands::Today) | Some(CodexCommands::Statusline) => {
                let today = timezone.to_fixed_offset(Utc::now()).date_naive();
                (Some(today), Some(today))
            }
            _ => (since, until),
        },
        _ => (since, until),
    };

    // Load pricing database (quiet mode for statusline)
    let is_statusline = is_statusline_command(&cli.command);
    let pricing_db = if is_statusline {
        PricingDb::load_quiet(cli.offline)
    } else {
        PricingDb::load(cli.offline)
    };

    // Handle Codex commands
    if let Some(Commands::Codex { command }) = &cli.command {
        handle_codex_command(
            command,
            since,
            until,
            &cli,
            &pricing_db,
            &timezone,
            number_format,
            jq_filter,
        );
        return;
    }

    // Handle session command separately
    if matches!(cli.command, Some(Commands::Session)) {
        let sessions = load_session_data(since, until, false, &timezone);
        if sessions.is_empty() {
            println!("No session data found for the specified date range.");
            return;
        }
        let use_color = cli.use_color();
        let show_cost = cli.show_cost();
        if cli.json {
            let json = output_session_json(&sessions, &pricing_db, cli.order, show_cost);
            print_json(&json, jq_filter);
        } else {
            print_session_table(
                &sessions,
                &pricing_db,
                cli.order,
                use_color,
                cli.compact,
                show_cost,
                number_format,
                &timezone,
            );
        }
        return;
    }

    // Handle project command separately
    if matches!(cli.command, Some(Commands::Project)) {
        let projects = load_project_data(since, until, false, &timezone);
        if projects.is_empty() {
            println!("No project data found for the specified date range.");
            return;
        }
        let use_color = cli.use_color();
        let show_cost = cli.show_cost();
        if cli.json {
            let json = output_project_json(&projects, &pricing_db, cli.order, show_cost);
            print_json(&json, jq_filter);
        } else {
            print_project_table(
                &projects,
                &pricing_db,
                cli.order,
                use_color,
                cli.compact,
                show_cost,
                number_format,
            );
        }
        return;
    }

    // Handle blocks command separately
    if matches!(cli.command, Some(Commands::Blocks)) {
        let blocks = load_block_data(since, until, false, &timezone);
        if blocks.is_empty() {
            println!("No block data found for the specified date range.");
            return;
        }
        let use_color = cli.use_color();
        let show_cost = cli.show_cost();
        if cli.json {
            let json = output_block_json(&blocks, &pricing_db, cli.order, show_cost);
            print_json(&json, jq_filter);
        } else {
            print_block_table(
                &blocks,
                &pricing_db,
                cli.order,
                use_color,
                cli.compact,
                show_cost,
                number_format,
            );
        }
        return;
    }

    // Load usage data (quiet mode for statusline)
    let (day_stats, skipped, valid) = if is_statusline {
        load_usage_data_quiet(since, until, &timezone)
    } else {
        load_usage_data_with_debug(since, until, cli.debug, &timezone)
    };

    // For statusline, handle empty data gracefully
    if is_statusline {
        if cli.json {
            let json = print_statusline_json(&day_stats, &pricing_db, number_format);
            print_json(&json, jq_filter);
        } else {
            print_statusline(&day_stats, &pricing_db, number_format);
        }
        return;
    }

    if day_stats.is_empty() {
        println!("No data found for the specified date range.");
        return;
    }

    let use_color = cli.use_color();
    let show_cost = cli.show_cost();

    // Determine output format based on command
    match &cli.command {
        Some(Commands::Weekly) => {
            if cli.json {
                let json =
                    output_weekly_json(&day_stats, &pricing_db, cli.order, cli.breakdown, show_cost);
                print_json(&json, jq_filter);
            } else {
                print_weekly_table(
                    &day_stats,
                    cli.breakdown,
                    skipped,
                    valid,
                    &pricing_db,
                    cli.order,
                    use_color,
                    cli.compact,
                    show_cost,
                    number_format,
                );
            }
        }
        Some(Commands::Monthly) => {
            if cli.json {
                let json = output_monthly_json(
                    &day_stats,
                    &pricing_db,
                    cli.order,
                    cli.breakdown,
                    show_cost,
                );
                print_json(&json, jq_filter);
            } else {
                print_monthly_table(
                    &day_stats,
                    cli.breakdown,
                    skipped,
                    valid,
                    &pricing_db,
                    cli.order,
                    use_color,
                    cli.compact,
                    show_cost,
                    number_format,
                );
            }
        }
        _ => {
            // Daily is default (including Today which just filters dates)
            if cli.json {
                let json =
                    output_daily_json(&day_stats, &pricing_db, cli.order, cli.breakdown, show_cost);
                print_json(&json, jq_filter);
            } else {
                print_daily_table(
                    &day_stats,
                    cli.breakdown,
                    skipped,
                    valid,
                    &pricing_db,
                    cli.order,
                    use_color,
                    cli.compact,
                    show_cost,
                    number_format,
                );
            }
        }
    }
}

/// Handle Codex subcommands
fn handle_codex_command(
    command: &Option<CodexCommands>,
    since: Option<chrono::NaiveDate>,
    until: Option<chrono::NaiveDate>,
    cli: &Cli,
    pricing_db: &PricingDb,
    timezone: &Timezone,
    number_format: NumberFormat,
    jq_filter: Option<&str>,
) {
    let is_statusline = matches!(command, Some(CodexCommands::Statusline));

    // Handle session command
    if matches!(command, Some(CodexCommands::Session)) {
        let sessions = load_codex_session_data(since, until, false, timezone);
        if sessions.is_empty() {
            println!("No Codex session data found for the specified date range.");
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
        return;
    }

    // Load Codex usage data
    let (day_stats, skipped, valid) = load_codex_usage_data(since, until, is_statusline, timezone);

    // For statusline, handle empty data gracefully
    if is_statusline {
        if cli.json {
            let json = print_statusline_json(&day_stats, pricing_db, number_format);
            print_json(&json, jq_filter);
        } else {
            print_statusline(&day_stats, pricing_db, number_format);
        }
        return;
    }

    if day_stats.is_empty() {
        println!("No Codex data found for the specified date range.");
        return;
    }

    let use_color = cli.use_color();
    let show_cost = cli.show_cost();

    // Determine output format based on command
    match command {
        Some(CodexCommands::Weekly) => {
            if cli.json {
                let json = output_weekly_json(
                    &day_stats,
                    pricing_db,
                    cli.order,
                    cli.breakdown,
                    show_cost,
                );
                print_json(&json, jq_filter);
            } else {
                print_weekly_table(
                    &day_stats,
                    cli.breakdown,
                    skipped,
                    valid,
                    pricing_db,
                    cli.order,
                    use_color,
                    cli.compact,
                    show_cost,
                    number_format,
                );
            }
        }
        Some(CodexCommands::Monthly) => {
            if cli.json {
                let json = output_monthly_json(
                    &day_stats,
                    pricing_db,
                    cli.order,
                    cli.breakdown,
                    show_cost,
                );
                print_json(&json, jq_filter);
            } else {
                print_monthly_table(
                    &day_stats,
                    cli.breakdown,
                    skipped,
                    valid,
                    pricing_db,
                    cli.order,
                    use_color,
                    cli.compact,
                    show_cost,
                    number_format,
                );
            }
        }
        _ => {
            // Daily is default (including Today which just filters dates)
            if cli.json {
                let json = output_daily_json(
                    &day_stats,
                    pricing_db,
                    cli.order,
                    cli.breakdown,
                    show_cost,
                );
                print_json(&json, jq_filter);
            } else {
                print_daily_table(
                    &day_stats,
                    cli.breakdown,
                    skipped,
                    valid,
                    pricing_db,
                    cli.order,
                    use_color,
                    cli.compact,
                    show_cost,
                    number_format,
                );
            }
        }
    }
}
