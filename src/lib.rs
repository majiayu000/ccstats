//! `ccstats` is a local-first library and CLI for token and cost analytics from
//! Claude Code, `OpenAI` Codex, Cursor, Grok, and Kimi Code session logs.
//!
//! The public SDK entry points are [`summarize_cost`] and
//! [`summarize_cost_ranges`] for explicit options, plus
//! [`summarize_cost_with_cli_config`] and
//! [`summarize_cost_ranges_with_cli_config`] for CLI-aligned config defaults.
//! The binary target calls [`run_cli`] to preserve the existing command-line
//! behavior.

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
mod endpoints_cmd;
mod error;
mod output;
mod pricing;
mod sdk;
mod source;
mod sources_cmd;
mod utils;

pub use sdk::{
    CostSummary, ModelCostSummary, MultiCostSummary, MultiSummaryOptions, SdkError, SummaryOptions,
    TokenBreakdown, UsageRange, UsageSource, summarize_cost, summarize_cost_ranges,
    summarize_cost_ranges_with_cli_config, summarize_cost_with_cli_config,
};

use chrono::{NaiveDate, Utc};
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

fn load_config(is_statusline: bool) -> Config {
    let config_result = if is_statusline {
        Config::try_load_quiet()
    } else {
        Config::try_load()
    };
    match config_result {
        Ok(config) => config,
        Err(err) => {
            eprintln!("Error: {err}");
            std::process::exit(1);
        }
    }
}

fn resolve_timezone(timezone: Option<&str>, cli_timezone_was_set: bool) -> Timezone {
    let timezone_source = if cli_timezone_was_set {
        Some(TimezoneSource::Cli)
    } else if timezone.is_some() {
        Some(TimezoneSource::Config)
    } else {
        None
    };

    match Timezone::parse(timezone) {
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
    }
}

fn resolve_number_format(locale: Option<&str>) -> NumberFormat {
    match NumberFormat::from_locale(locale) {
        Ok(format) => format,
        Err(err) => {
            eprintln!("Warning: {err}. Using default locale.");
            NumberFormat::default()
        }
    }
}

fn parse_date_flag(value: Option<&str>, flag: &str) -> Option<NaiveDate> {
    value.map(|s| match parse_date(s) {
        Ok(date) => date,
        Err(err) => {
            eprintln!("{flag}: {err}");
            std::process::exit(1);
        }
    })
}

fn validate_date_range(since: Option<NaiveDate>, until: Option<NaiveDate>) {
    if let (Some(s), Some(u)) = (since, until)
        && s > u
    {
        eprintln!("Error: --since ({s}) is after --until ({u})");
        std::process::exit(1);
    }
}

fn validate_monthly_budget(cli: &Cli, source_cmd: SourceCommand) {
    let Some(monthly_budget) = cli.monthly_budget else {
        return;
    };
    if !monthly_budget.is_finite() || monthly_budget <= 0.0 {
        eprintln!("Error: --monthly-budget must be a positive number");
        std::process::exit(1);
    }
    if source_cmd != SourceCommand::Monthly {
        eprintln!("Error: --monthly-budget only supports the monthly command");
        std::process::exit(1);
    }
    if !cli.show_cost() {
        eprintln!("Error: --monthly-budget requires cost display; remove --no-cost or --cost hide");
        std::process::exit(1);
    }
}

fn build_date_filter(
    source_cmd: SourceCommand,
    today: NaiveDate,
    since: Option<NaiveDate>,
    until: Option<NaiveDate>,
) -> DateFilter {
    if source_cmd.needs_today_filter() {
        DateFilter::new(Some(today), Some(today))
    } else {
        DateFilter::new(since, until)
    }
}

fn load_pricing_db(cli: &Cli, needs_pricing: bool, is_statusline: bool) -> PricingDb {
    if !needs_pricing {
        PricingDb::default()
    } else if is_statusline {
        PricingDb::load_quiet(cli.offline, cli.strict_pricing)
    } else {
        PricingDb::load(cli.offline, cli.strict_pricing)
    }
}

fn unknown_source_message(input: &str) -> String {
    let available = source_choices().join(", ");
    if let Some(suggested) = suggest_source(input) {
        format!(
            "Error: unknown source '{input}'. Did you mean '{suggested}'? Available: {available}"
        )
    } else {
        format!("Error: unknown source '{input}'. Available: {available}")
    }
}

fn resolve_source_name<'a>(
    source_hint: Option<&'static str>,
    source_override: Option<&'a str>,
    source_cmd: SourceCommand,
) -> &'a str {
    if source_cmd == SourceCommand::Sources {
        return "claude";
    }

    match (source_hint, source_override) {
        (Some(hint), Some(override_name)) => resolve_overridden_command_source(hint, override_name),
        (Some(hint), None) => hint,
        (None, Some(name)) => name,
        (None, None) => "claude",
    }
}

fn resolve_overridden_command_source(hint: &'static str, override_name: &str) -> &'static str {
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

fn load_currency_converter(
    cli: &Cli,
    needs_pricing: bool,
    is_statusline: bool,
) -> Option<CurrencyConverter> {
    if !needs_pricing {
        return None;
    }

    cli.currency.as_ref().map(|code| {
        let Some(converter) = CurrencyConverter::load(code, cli.offline) else {
            eprintln!(
                "Error: failed to load exchange rate for '{code}'. Use a supported currency with cached rates, refresh rates without --offline, or omit --currency."
            );
            std::process::exit(1);
        };
        if !is_statusline && converter.currency_code() != "USD" {
            eprintln!(
                "Converting costs to {} (rate: displayed as {})",
                converter.currency_code(),
                converter.format(1.0)
            );
        }
        converter
    })
}

fn dispatch_command(source_name: &str, source_cmd: SourceCommand, context: &CommandContext<'_>) {
    if source_name.eq_ignore_ascii_case(ALL_SOURCES) {
        return handle_all_sources_command(source_cmd, context);
    }

    let Some(source) = get_source(source_name) else {
        eprintln!("{}", unknown_source_message(source_name));
        std::process::exit(1);
    };

    handle_source_command(source, source_cmd, context);
}

/// Run the `ccstats` CLI using process arguments.
///
/// This is intended for the binary target. SDK consumers should prefer
/// [`summarize_cost`], [`summarize_cost_ranges`], or their CLI-config variants
/// so they receive structured data instead of rendered CLI output.
pub fn run_cli() {
    let raw_cli = Cli::parse();
    let cli_timezone_was_set = raw_cli.timezone.is_some();
    let parsed_command = parse_command(raw_cli.command.as_ref());
    let source_cmd = parsed_command.command;
    let is_statusline = source_cmd.is_statusline();

    let config = load_config(is_statusline);
    let cli = raw_cli.with_config(&config);
    let timezone = resolve_timezone(cli.timezone.as_deref(), cli_timezone_was_set);
    let number_format = resolve_number_format(cli.locale.as_deref());

    let jq_filter = cli.jq.as_deref();
    let since = parse_date_flag(cli.since.as_deref(), "--since");
    let until = parse_date_flag(cli.until.as_deref(), "--until");
    validate_date_range(since, until);
    validate_monthly_budget(&cli, source_cmd);

    let today = timezone.to_fixed_offset(Utc::now()).date_naive();
    let budget_as_of = until.map_or(today, |end| end.min(today));
    let filter = build_date_filter(source_cmd, today, since, until);
    let show_cost = cli.show_cost();
    let needs_pricing = is_statusline || show_cost;
    let pricing_db = load_pricing_db(&cli, needs_pricing, is_statusline);
    let source_name = resolve_source_name(
        parsed_command.source_hint,
        cli.source.as_deref(),
        source_cmd,
    );
    let currency_converter = load_currency_converter(&cli, needs_pricing, is_statusline);

    dispatch_command(
        source_name,
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

    if cli.debug && needs_pricing {
        for diagnostic in pricing_db.pricing_diagnostics() {
            eprintln!("{diagnostic}");
        }
    }
}
