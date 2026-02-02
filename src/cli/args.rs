use clap::{Parser, Subcommand, ValueEnum};

use crate::config::Config;

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

#[derive(Debug, Clone, Copy, Default, ValueEnum, PartialEq)]
pub enum CostMode {
    /// Show calculated costs (default)
    #[default]
    Show,
    /// Hide cost column entirely
    Hide,
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

    /// Enable debug output (show processing details)
    #[arg(long, global = true)]
    pub debug: bool,

    /// Compact output (fewer columns, shorter names)
    #[arg(short = 'c', long, global = true)]
    pub compact: bool,

    /// Cost display mode
    #[arg(long, global = true, value_enum, default_value = "show")]
    pub cost: CostMode,

    /// Hide cost column (shorthand for --cost=hide)
    #[arg(long, global = true)]
    pub no_cost: bool,

    /// Filter JSON output with jq expression (requires jq installed)
    #[arg(long, global = true, value_name = "FILTER")]
    pub jq: Option<String>,

    /// Timezone for date display (e.g., "Asia/Shanghai", "UTC", "America/New_York")
    #[arg(long, global = true, value_name = "TZ")]
    pub timezone: Option<String>,

    /// Locale for number formatting (e.g., "en", "zh", "de")
    #[arg(long, global = true, value_name = "LOCALE")]
    pub locale: Option<String>,
}

impl Cli {
    /// Merge config file values into CLI (CLI args take precedence)
    pub fn with_config(mut self, config: &Config) -> Self {
        // Only apply config values if CLI didn't explicitly set them
        // For boolean flags, config only applies if CLI is false (default)
        if !self.offline && config.offline {
            self.offline = true;
        }
        if !self.compact && config.compact {
            self.compact = true;
        }
        if !self.no_cost && config.no_cost {
            self.no_cost = true;
        }
        if !self.no_color && config.no_color {
            self.no_color = true;
        }
        if !self.breakdown && config.breakdown {
            self.breakdown = true;
        }
        if !self.debug && config.debug {
            self.debug = true;
        }

        // For enum values, apply config if it's set
        if let Some(ref order) = config.order {
            if matches!(self.order, SortOrder::Asc) {
                // Only override if CLI is at default
                match order.to_lowercase().as_str() {
                    "desc" => self.order = SortOrder::Desc,
                    _ => {}
                }
            }
        }

        if let Some(ref color) = config.color {
            if matches!(self.color, ColorMode::Auto) {
                match color.to_lowercase().as_str() {
                    "always" => self.color = ColorMode::Always,
                    "never" => self.color = ColorMode::Never,
                    _ => {}
                }
            }
        }

        if let Some(ref cost) = config.cost {
            if matches!(self.cost, CostMode::Show) {
                match cost.to_lowercase().as_str() {
                    "hide" => self.cost = CostMode::Hide,
                    _ => {}
                }
            }
        }

        // String options: only apply if CLI didn't set them
        if self.timezone.is_none() {
            self.timezone = config.timezone.clone();
        }
        if self.locale.is_none() {
            self.locale = config.locale.clone();
        }

        self
    }

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

    pub fn show_cost(&self) -> bool {
        if self.no_cost {
            return false;
        }
        self.cost == CostMode::Show
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
    /// Show usage by session
    Session,
    /// Show usage by project
    Project,
    /// Show usage by 5-hour billing blocks
    Blocks,
    /// Output single line for statusline/tmux integration
    Statusline,
}
