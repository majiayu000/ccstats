//! CLI argument definitions
//!
//! Global CLI options and configuration merging logic.

use std::io::IsTerminal;

use clap::{Parser, ValueEnum};

use crate::config::{Config, ConfigColorMode, ConfigCostMode, ConfigSortOrder};

use super::commands::Commands;

#[derive(Debug, Clone, Copy, Default, ValueEnum, PartialEq, Eq)]
pub(crate) enum SortOrder {
    /// Oldest first (default)
    #[default]
    Asc,
    /// Newest first
    Desc,
}

#[derive(Debug, Clone, Copy, Default, ValueEnum, PartialEq)]
pub(crate) enum ColorMode {
    /// Auto-detect based on terminal (default)
    #[default]
    Auto,
    /// Always use colors
    Always,
    /// Never use colors
    Never,
}

#[derive(Debug, Clone, Copy, Default, ValueEnum, PartialEq)]
pub(crate) enum CostMode {
    /// Show calculated costs (default)
    #[default]
    Show,
    /// Hide cost column entirely
    Hide,
}

#[derive(Parser)]
#[command(name = "ccstats")]
#[command(about = "Fast Claude Code token usage statistics", version)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Option<Commands>,

    /// Filter from date (YYYYMMDD or YYYY-MM-DD)
    #[arg(short, long, global = true)]
    pub(crate) since: Option<String>,

    /// Filter until date (YYYYMMDD or YYYY-MM-DD)
    #[arg(short, long, global = true)]
    pub(crate) until: Option<String>,

    /// Show per-model breakdown
    #[arg(short, long, global = true)]
    pub(crate) breakdown: bool,

    /// Output as JSON
    #[arg(short, long, global = true)]
    pub(crate) json: bool,

    /// Use offline cached pricing (skip fetching from LiteLLM)
    #[arg(short = 'O', long, global = true)]
    pub(crate) offline: bool,

    /// Sort order for results
    #[arg(short, long, global = true, value_enum)]
    pub(crate) order: Option<SortOrder>,

    /// Color output mode
    #[arg(long, global = true, value_enum)]
    pub(crate) color: Option<ColorMode>,

    /// Disable colored output (shorthand for --color=never)
    #[arg(long, global = true)]
    pub(crate) no_color: bool,

    /// Enable debug output (show processing details)
    #[arg(long, global = true)]
    pub(crate) debug: bool,

    /// Treat unknown model pricing as unavailable (show N/A instead of fallback estimate)
    #[arg(long, global = true)]
    pub(crate) strict_pricing: bool,

    /// Compact output (fewer columns, shorter names)
    #[arg(short = 'c', long, global = true)]
    pub(crate) compact: bool,

    /// Cost display mode
    #[arg(long, global = true, value_enum)]
    pub(crate) cost: Option<CostMode>,

    /// Hide cost column (shorthand for --cost=hide)
    #[arg(long, global = true)]
    pub(crate) no_cost: bool,

    /// Filter JSON output with jq expression (requires jq installed)
    #[arg(long, global = true, value_name = "FILTER")]
    pub(crate) jq: Option<String>,

    /// Timezone for date display (e.g., "Asia/Shanghai", "UTC", "America/New_York")
    #[arg(long, global = true, value_name = "TZ")]
    pub(crate) timezone: Option<String>,

    /// Locale for number formatting (e.g., "en", "zh", "de")
    #[arg(long, global = true, value_name = "LOCALE")]
    pub(crate) locale: Option<String>,
}

impl Cli {
    pub(crate) fn sort_order(&self) -> SortOrder {
        self.order.unwrap_or_default()
    }

    fn color_mode(&self) -> ColorMode {
        self.color.unwrap_or_default()
    }

    fn cost_mode(&self) -> CostMode {
        self.cost.unwrap_or_default()
    }

    /// Merge config file values into CLI (CLI args take precedence)
    pub(crate) fn with_config(mut self, config: &Config) -> Self {
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
        if !self.strict_pricing && config.strict_pricing {
            self.strict_pricing = true;
        }

        // For enum values, only apply config when CLI did not set them
        if self.order.is_none() {
            self.order = config.order.map(|order| match order {
                ConfigSortOrder::Asc => SortOrder::Asc,
                ConfigSortOrder::Desc => SortOrder::Desc,
            });
        }

        if self.color.is_none() {
            self.color = config.color.map(|color| match color {
                ConfigColorMode::Auto => ColorMode::Auto,
                ConfigColorMode::Always => ColorMode::Always,
                ConfigColorMode::Never => ColorMode::Never,
            });
        }

        if self.cost.is_none() {
            self.cost = config.cost.map(|cost| match cost {
                ConfigCostMode::Show => CostMode::Show,
                ConfigCostMode::Hide => CostMode::Hide,
            });
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

    pub(crate) fn use_color(&self) -> bool {
        if self.no_color {
            return false;
        }
        match self.color_mode() {
            ColorMode::Always => true,
            ColorMode::Never => false,
            ColorMode::Auto => std::io::stdout().is_terminal(),
        }
    }

    pub(crate) fn show_cost(&self) -> bool {
        if self.no_cost {
            return false;
        }
        self.cost_mode() == CostMode::Show
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    #[test]
    fn cli_explicit_order_wins_over_config() {
        let cli = Cli::parse_from(["ccstats", "daily", "--order", "asc"]);
        let config = Config {
            order: Some(ConfigSortOrder::Desc),
            ..Default::default()
        };
        let merged = cli.with_config(&config);
        assert_eq!(merged.sort_order(), SortOrder::Asc);
    }

    #[test]
    fn config_order_applies_when_cli_not_set() {
        let cli = Cli::parse_from(["ccstats", "daily"]);
        let config = Config {
            order: Some(ConfigSortOrder::Desc),
            ..Default::default()
        };
        let merged = cli.with_config(&config);
        assert_eq!(merged.sort_order(), SortOrder::Desc);
    }
}
