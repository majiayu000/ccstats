mod cli;
mod data;
mod output;
mod pricing;
mod utils;

use chrono::Local;
use clap::Parser;

use cli::{Cli, Commands};
use data::{load_usage_data_quiet, load_usage_data_with_debug};
use output::{
    output_daily_json, output_monthly_json, output_weekly_json, print_daily_table,
    print_monthly_table, print_statusline, print_statusline_json, print_weekly_table,
};
use pricing::PricingDb;
use utils::parse_date;

fn main() {
    let cli = Cli::parse();

    let since = cli.since.as_ref().and_then(|s| parse_date(s));
    let until = cli.until.as_ref().and_then(|s| parse_date(s));

    // For "today" and "statusline" commands, set since/until to today
    let (since, until) = match &cli.command {
        Some(Commands::Today) | Some(Commands::Statusline) => {
            let today = Local::now().date_naive();
            (Some(today), Some(today))
        }
        _ => (since, until),
    };

    // Load pricing database (quiet mode for statusline)
    let is_statusline = matches!(cli.command, Some(Commands::Statusline));
    let pricing_db = if is_statusline {
        PricingDb::load_quiet(cli.offline)
    } else {
        PricingDb::load(cli.offline)
    };

    // Load usage data (quiet mode for statusline)
    let (day_stats, skipped, valid) = if is_statusline {
        load_usage_data_quiet(since, until)
    } else {
        load_usage_data_with_debug(since, until, cli.debug)
    };

    // For statusline, handle empty data gracefully
    if is_statusline {
        if cli.json {
            print_statusline_json(&day_stats, &pricing_db);
        } else {
            print_statusline(&day_stats, &pricing_db);
        }
        return;
    }

    if day_stats.is_empty() {
        println!("No data found for the specified date range.");
        return;
    }

    let use_color = cli.use_color();

    // Determine output format based on command
    match &cli.command {
        Some(Commands::Weekly) => {
            if cli.json {
                output_weekly_json(&day_stats, &pricing_db, cli.order, cli.breakdown);
            } else {
                print_weekly_table(&day_stats, cli.breakdown, skipped, valid, &pricing_db, cli.order, use_color, cli.compact);
            }
        }
        Some(Commands::Monthly) => {
            if cli.json {
                output_monthly_json(&day_stats, &pricing_db, cli.order, cli.breakdown);
            } else {
                print_monthly_table(&day_stats, cli.breakdown, skipped, valid, &pricing_db, cli.order, use_color, cli.compact);
            }
        }
        _ => {
            // Daily is default (including Today which just filters dates)
            if cli.json {
                output_daily_json(&day_stats, &pricing_db, cli.order, cli.breakdown);
            } else {
                print_daily_table(&day_stats, cli.breakdown, skipped, valid, &pricing_db, cli.order, use_color, cli.compact);
            }
        }
    }
}
