use clap::{Parser, Subcommand, ValueEnum};

#[derive(Debug, Clone, Copy, Default, ValueEnum)]
pub enum SortOrder {
    /// Oldest first (default)
    #[default]
    Asc,
    /// Newest first
    Desc,
}

#[derive(Debug, Clone, Copy, Default, ValueEnum, PartialEq)]
pub enum ColorMode {
    /// Auto-detect based on terminal (default)
    #[default]
    Auto,
    /// Always use colors
    Always,
    /// Never use colors
    Never,
}

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

    /// Sort order for results
    #[arg(short, long, global = true, value_enum, default_value = "asc")]
    pub order: SortOrder,

    /// Color output mode
    #[arg(long, global = true, value_enum, default_value = "auto")]
    pub color: ColorMode,

    /// Disable colored output (shorthand for --color=never)
    #[arg(long, global = true)]
    pub no_color: bool,
}

impl Cli {
    pub fn use_color(&self) -> bool {
        if self.no_color {
            return false;
        }
        match self.color {
            ColorMode::Always => true,
            ColorMode::Never => false,
            ColorMode::Auto => atty::is(atty::Stream::Stdout),
        }
    }
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
