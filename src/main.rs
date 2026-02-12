mod app;
mod cli;
mod config;
mod core;
mod error;
mod output;
mod pricing;
mod source;
mod utils;

use chrono::Utc;
use clap::Parser;

use app::{CommandContext, handle_source_command};
use cli::{Cli, parse_command};
use config::Config;
use core::DateFilter;
use output::NumberFormat;
use pricing::PricingDb;
use source::get_source;
use utils::{Timezone, parse_date, set_parse_debug};

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
        PricingDb::load_quiet(cli.offline, cli.strict_pricing)
    } else {
        PricingDb::load(cli.offline, cli.strict_pricing)
    };

    // Get the appropriate data source
    let source_name = if is_codex { "codex" } else { "claude" };
    let source = match get_source(source_name) {
        Some(s) => s,
        None => {
            eprintln!("Error: {} source not found", source_name);
            std::process::exit(1);
        }
    };

    handle_source_command(
        source,
        &source_cmd,
        CommandContext {
            filter: &filter,
            cli: &cli,
            pricing_db: &pricing_db,
            timezone,
            number_format,
            jq_filter,
        },
    );
}
