// Enable pedantic lints for new code quality; allow domain-inherent cast warnings
// (token counts are i64, display needs f64/u32 — precision loss is acceptable)
#![warn(clippy::pedantic)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]

mod app;
mod cli;
mod config;
mod consts;
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

enum TimezoneSource {
    Cli,
    Config,
}

fn main() {
    // Parse CLI and extract source command
    let raw_cli = Cli::parse();
    let raw_timezone = raw_cli.timezone.clone();
    let (is_codex, source_cmd) = parse_command(raw_cli.command.as_ref());
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

    let timezone_source = if raw_timezone.is_some() {
        Some(TimezoneSource::Cli)
    } else if cli.timezone.is_some() {
        Some(TimezoneSource::Config)
    } else {
        None
    };

    let timezone = match Timezone::parse(cli.timezone.as_deref()) {
        Ok(tz) => tz,
        Err(err) => {
            if let Some(TimezoneSource::Config) = timezone_source {
                eprintln!("Warning: {err}. Falling back to local timezone.");
                Timezone::Local
            } else {
                eprintln!("{err}");
                std::process::exit(1);
            }
        }
    };

    let number_format = match NumberFormat::from_locale(cli.locale.as_deref()) {
        Ok(format) => format,
        Err(err) => {
            eprintln!("Warning: {err}. Using default locale.");
            NumberFormat::default()
        }
    };

    let jq_filter = cli.jq.as_deref();

    let since = match cli.since.as_ref() {
        Some(s) => match parse_date(s) {
            Ok(date) => Some(date),
            Err(err) => {
                eprintln!("--since: {err}");
                std::process::exit(1);
            }
        },
        None => None,
    };
    let until = match cli.until.as_ref() {
        Some(s) => match parse_date(s) {
            Ok(date) => Some(date),
            Err(err) => {
                eprintln!("--until: {err}");
                std::process::exit(1);
            }
        },
        None => None,
    };

    // Validate date range
    if let (Some(s), Some(u)) = (since, until)
        && s > u
    {
        eprintln!("Error: --since ({s}) is after --until ({u})");
        std::process::exit(1);
    }

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
    let Some(source) = get_source(source_name) else {
        eprintln!("Error: {source_name} source not found");
        std::process::exit(1);
    };

    handle_source_command(
        source,
        source_cmd,
        &CommandContext {
            filter: &filter,
            cli: &cli,
            pricing_db: &pricing_db,
            timezone,
            number_format,
            jq_filter,
        },
    );
}
