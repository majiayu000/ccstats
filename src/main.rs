mod cli;
mod data;
mod output;
mod pricing;
mod utils;

use chrono::Local;
use clap::Parser;

use cli::{Cli, Commands};
use data::load_usage_data;
use output::{
    output_daily_json, output_monthly_json, output_weekly_json, print_daily_table,
    print_monthly_table, print_weekly_table,
};
use pricing::PricingDb;
use utils::parse_date;

fn main() {
    let cli = Cli::parse();

    let since = cli.since.as_ref().and_then(|s| parse_date(s));
    let until = cli.until.as_ref().and_then(|s| parse_date(s));

    // For "today" command, set since/until to today
    let (since, until) = match &cli.command {
        Some(Commands::Today) => {
            let today = Local::now().date_naive();
            (Some(today), Some(today))
        }
        _ => (since, until),
    };

    // Load pricing database
    let pricing_db = PricingDb::load(cli.offline);

    // Load usage data
    let (day_stats, skipped, valid) = load_usage_data(since, until);

    if day_stats.is_empty() {
        println!("No data found for the specified date range.");
        return;
    }

    let use_color = cli.use_color();

    // Determine output format based on command
    match &cli.command {
        Some(Commands::Weekly) => {
            if cli.json {
                output_weekly_json(&day_stats, &pricing_db, cli.order);
            } else {
                print_weekly_table(&day_stats, cli.breakdown, skipped, valid, &pricing_db, cli.order, use_color);
            }
        }
        Some(Commands::Monthly) => {
            if cli.json {
                output_monthly_json(&day_stats, &pricing_db, cli.order);
            } else {
                print_monthly_table(&day_stats, cli.breakdown, skipped, valid, &pricing_db, cli.order, use_color);
            }
        }
        _ => {
            // Daily is default (including Today which just filters dates)
            if cli.json {
                output_daily_json(&day_stats, &pricing_db, cli.order);
            } else {
                print_daily_table(&day_stats, cli.breakdown, skipped, valid, &pricing_db, cli.order, use_color);
            }
        }
    }
}
