//! `ccstats` is a local-first library and CLI for token and cost analytics from
//! Claude Code, `OpenAI` Codex, and Cursor session logs.
//!
//! The public SDK entry points are [`summarize_cost`] for explicit options and
//! [`summarize_cost_with_cli_config`] for CLI-aligned config defaults. The binary
//! target calls [`run_cli`] to preserve the existing command-line behavior.

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
mod sdk;
mod source;
mod utils;

pub use sdk::{
    CostSummary, ModelCostSummary, SdkError, SummaryOptions, TokenBreakdown, UsageRange,
    UsageSource, summarize_cost, summarize_cost_with_cli_config,
};

use chrono::Utc;
use clap::Parser;

use app::{CommandContext, handle_all_sources_command, handle_source_command};
use cli::{Cli, SourceCommand, parse_command};
use config::Config;
use core::DateFilter;
use output::NumberFormat;
use pricing::{CurrencyConverter, PricingDb};
use source::{ALL_SOURCES, get_source, source_choices, suggest_source};
use utils::{Timezone, parse_date};

enum TimezoneSource {
    Cli,
    Config,
}

/// Run the `ccstats` CLI using process arguments.
///
/// This is intended for the binary target. SDK consumers should prefer
/// [`summarize_cost`] or [`summarize_cost_with_cli_config`] so they receive
/// structured data instead of rendered CLI output.
#[allow(clippy::too_many_lines)] // CLI setup intentionally stays in one dispatch function.
pub fn run_cli() {
    let raw_cli = Cli::parse();
    let raw_timezone = raw_cli.timezone.clone();
    let parsed_command = parse_command(raw_cli.command.as_ref());
    let source_cmd = parsed_command.command;
    let is_statusline = source_cmd.is_statusline();

    let config = if is_statusline {
        Config::load_quiet()
    } else {
        Config::load()
    };

    let cli = raw_cli.with_config(&config);

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

    let parse_date_flag = |value: &Option<String>, flag: &str| {
        value.as_ref().map(|s| match parse_date(s) {
            Ok(date) => date,
            Err(err) => {
                eprintln!("{flag}: {err}");
                std::process::exit(1);
            }
        })
    };
    let since = parse_date_flag(&cli.since, "--since");
    let until = parse_date_flag(&cli.until, "--until");

    if let (Some(s), Some(u)) = (since, until)
        && s > u
    {
        eprintln!("Error: --since ({s}) is after --until ({u})");
        std::process::exit(1);
    }

    if let Some(monthly_budget) = cli.monthly_budget {
        if !monthly_budget.is_finite() || monthly_budget <= 0.0 {
            eprintln!("Error: --monthly-budget must be a positive number");
            std::process::exit(1);
        }
        if source_cmd != SourceCommand::Monthly {
            eprintln!("Error: --monthly-budget only supports the monthly command");
            std::process::exit(1);
        }
        if !cli.show_cost() {
            eprintln!(
                "Error: --monthly-budget requires cost display; remove --no-cost or --cost hide"
            );
            std::process::exit(1);
        }
    }

    let today = timezone.to_fixed_offset(Utc::now()).date_naive();
    let budget_as_of = until.map_or(today, |end| end.min(today));

    let filter = if source_cmd.needs_today_filter() {
        DateFilter::new(Some(today), Some(today))
    } else {
        DateFilter::new(since, until)
    };

    let show_cost = cli.show_cost();
    let needs_pricing = is_statusline || show_cost;

    let pricing_db = if !needs_pricing {
        PricingDb::default()
    } else if is_statusline {
        PricingDb::load_quiet(cli.offline, cli.strict_pricing)
    } else {
        PricingDb::load(cli.offline, cli.strict_pricing)
    };

    let unknown_source_message = |input: &str| {
        let available = source_choices().join(", ");
        if let Some(suggested) = suggest_source(input) {
            format!(
                "Error: unknown source '{input}'. Did you mean '{suggested}'? Available: {available}"
            )
        } else {
            format!("Error: unknown source '{input}'. Available: {available}")
        }
    };

    let source_name = if source_cmd == SourceCommand::Sources {
        "claude"
    } else {
        match (parsed_command.source_hint, cli.source.as_deref()) {
            (Some(hint), Some(override_name)) => {
                if override_name.eq_ignore_ascii_case(ALL_SOURCES) {
                    eprintln!("Error: command source '{hint}' conflicts with --source all");
                    std::process::exit(1);
                }
                let Some(override_source) = get_source(override_name) else {
                    eprintln!("{}", unknown_source_message(override_name));
                    std::process::exit(1);
                };
                if override_source.name() != hint {
                    eprintln!(
                        "Error: command source '{hint}' conflicts with --source '{}'",
                        override_source.name()
                    );
                    std::process::exit(1);
                }
                override_source.name()
            }
            (Some(hint), None) => hint,
            (None, Some(name)) => name,
            (None, None) => "claude",
        }
    };

    let currency_converter = if show_cost {
        cli.currency.as_ref().map(|code| {
            let conv = if let Some(conv) = CurrencyConverter::load(code, cli.offline) {
                conv
            } else {
                if !is_statusline {
                    eprintln!(
                        "Warning: failed to load exchange rate for '{code}', showing USD costs."
                    );
                }
                let Some(conv) = CurrencyConverter::load("USD", true) else {
                    eprintln!("Error: failed to initialize USD currency converter");
                    std::process::exit(1);
                };
                conv
            };
            if !is_statusline && conv.currency_code() != "USD" {
                eprintln!(
                    "Converting costs to {} (rate: displayed as {})",
                    conv.currency_code(),
                    conv.format(1.0)
                );
            }
            conv
        })
    } else {
        None
    };

    if source_name.eq_ignore_ascii_case(ALL_SOURCES) {
        return handle_all_sources_command(
            source_cmd,
            &CommandContext {
                filter: &filter,
                cli: &cli,
                pricing_db: &pricing_db,
                timezone,
                number_format,
                jq_filter,
                currency: currency_converter.as_ref(),
                budget_as_of,
            },
        );
    }

    let Some(source) = get_source(source_name) else {
        eprintln!("{}", unknown_source_message(source_name));
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
            currency: currency_converter.as_ref(),
            budget_as_of,
        },
    );
}
