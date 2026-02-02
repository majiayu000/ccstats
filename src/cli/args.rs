use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "ccstats")]
#[command(about = "Fast Claude Code token usage statistics", version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Filter from date (YYYYMMDD or YYYY-MM-DD)
    #[arg(short, long, global = true)]
    pub since: Option<String>,

    /// Filter until date (YYYYMMDD or YYYY-MM-DD)
    #[arg(short, long, global = true)]
    pub until: Option<String>,

    /// Show per-model breakdown
    #[arg(short, long, global = true)]
    pub breakdown: bool,

    /// Output as JSON
    #[arg(short, long, global = true)]
    pub json: bool,

    /// Use offline cached pricing (skip fetching from LiteLLM)
    #[arg(short = 'O', long, global = true)]
    pub offline: bool,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Show daily usage (default)
    Daily,
    /// Show weekly usage
    Weekly,
    /// Show monthly usage
    Monthly,
    /// Show today's usage
    Today,
}
