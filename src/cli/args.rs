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
#[allow(clippy::struct_excessive_bools)]
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

    /// Output as CSV
    #[arg(long, global = true)]
    pub(crate) csv: bool,

    /// Use offline cached pricing (skip fetching from `LiteLLM`)
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

    /// Timezone for date display (e.g., `Asia/Shanghai`, `UTC`, `America/New_York`)
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
        if !self.no_cost && self.cost.is_none() && config.no_cost {
            self.no_cost = true;
        }
        if !self.no_color && self.color.is_none() && config.no_color {
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
            self.timezone.clone_from(&config.timezone);
        }
        if self.locale.is_none() {
            self.locale.clone_from(&config.locale);
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
            ColorMode::Auto => {
                // Respect the NO_COLOR convention (https://no-color.org/)
                if std::env::var_os("NO_COLOR").is_some() {
                    return false;
                }
                std::io::stdout().is_terminal()
            }
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

    #[test]
    fn cli_explicit_cost_show_wins_over_config_no_cost() {
        let cli = Cli::parse_from(["ccstats", "daily", "--cost", "show"]);
        let config = Config {
            no_cost: true,
            ..Default::default()
        };
        let merged = cli.with_config(&config);
        assert!(merged.show_cost());
    }

    #[test]
    fn cli_explicit_color_always_wins_over_config_no_color() {
        let cli = Cli::parse_from(["ccstats", "daily", "--color", "always"]);
        let config = Config {
            no_color: true,
            ..Default::default()
        };
        let merged = cli.with_config(&config);
        assert!(merged.use_color());
    }

    // --- Boolean config merging ---

    #[test]
    fn config_offline_applies_when_cli_not_set() {
        let cli = Cli::parse_from(["ccstats", "daily"]);
        let config = Config {
            offline: true,
            ..Default::default()
        };
        let merged = cli.with_config(&config);
        assert!(merged.offline);
    }

    #[test]
    fn config_compact_applies_when_cli_not_set() {
        let cli = Cli::parse_from(["ccstats", "daily"]);
        let config = Config {
            compact: true,
            ..Default::default()
        };
        let merged = cli.with_config(&config);
        assert!(merged.compact);
    }

    #[test]
    fn config_breakdown_applies_when_cli_not_set() {
        let cli = Cli::parse_from(["ccstats", "daily"]);
        let config = Config {
            breakdown: true,
            ..Default::default()
        };
        let merged = cli.with_config(&config);
        assert!(merged.breakdown);
    }

    #[test]
    fn config_debug_applies_when_cli_not_set() {
        let cli = Cli::parse_from(["ccstats", "daily"]);
        let config = Config {
            debug: true,
            ..Default::default()
        };
        let merged = cli.with_config(&config);
        assert!(merged.debug);
    }

    #[test]
    fn config_strict_pricing_applies_when_cli_not_set() {
        let cli = Cli::parse_from(["ccstats", "daily"]);
        let config = Config {
            strict_pricing: true,
            ..Default::default()
        };
        let merged = cli.with_config(&config);
        assert!(merged.strict_pricing);
    }

    #[test]
    fn cli_boolean_flags_override_config_false() {
        // CLI sets --compact, config has compact=false (default)
        let cli = Cli::parse_from(["ccstats", "daily", "--compact"]);
        let config = Config::default();
        let merged = cli.with_config(&config);
        assert!(merged.compact);
    }

    // --- no_cost / no_color shorthand ---

    #[test]
    fn no_cost_flag_hides_cost() {
        let cli = Cli::parse_from(["ccstats", "daily", "--no-cost"]);
        assert!(!cli.show_cost());
    }

    #[test]
    fn config_no_cost_applies_when_cli_not_set() {
        let cli = Cli::parse_from(["ccstats", "daily"]);
        let config = Config {
            no_cost: true,
            ..Default::default()
        };
        let merged = cli.with_config(&config);
        assert!(!merged.show_cost());
    }

    #[test]
    fn cost_hide_enum_hides_cost() {
        let cli = Cli::parse_from(["ccstats", "daily", "--cost", "hide"]);
        assert!(!cli.show_cost());
    }

    #[test]
    fn config_cost_hide_applies_when_cli_not_set() {
        let cli = Cli::parse_from(["ccstats", "daily"]);
        let config = Config {
            cost: Some(ConfigCostMode::Hide),
            ..Default::default()
        };
        let merged = cli.with_config(&config);
        assert!(!merged.show_cost());
    }

    #[test]
    fn no_color_flag_disables_color() {
        let cli = Cli::parse_from(["ccstats", "daily", "--no-color"]);
        assert!(!cli.use_color());
    }

    #[test]
    fn color_never_disables_color() {
        let cli = Cli::parse_from(["ccstats", "daily", "--color", "never"]);
        assert!(!cli.use_color());
    }

    #[test]
    fn config_no_color_applies_when_cli_not_set() {
        let cli = Cli::parse_from(["ccstats", "daily"]);
        let config = Config {
            no_color: true,
            ..Default::default()
        };
        let merged = cli.with_config(&config);
        assert!(!merged.use_color());
    }

    #[test]
    fn config_color_never_applies_when_cli_not_set() {
        let cli = Cli::parse_from(["ccstats", "daily"]);
        let config = Config {
            color: Some(ConfigColorMode::Never),
            ..Default::default()
        };
        let merged = cli.with_config(&config);
        assert!(!merged.use_color());
    }

    // --- Timezone / locale merging ---

    #[test]
    fn config_timezone_applies_when_cli_not_set() {
        let cli = Cli::parse_from(["ccstats", "daily"]);
        let config = Config {
            timezone: Some("Asia/Shanghai".to_string()),
            ..Default::default()
        };
        let merged = cli.with_config(&config);
        assert_eq!(merged.timezone.as_deref(), Some("Asia/Shanghai"));
    }

    #[test]
    fn cli_timezone_wins_over_config() {
        let cli = Cli::parse_from(["ccstats", "daily", "--timezone", "UTC"]);
        let config = Config {
            timezone: Some("Asia/Shanghai".to_string()),
            ..Default::default()
        };
        let merged = cli.with_config(&config);
        assert_eq!(merged.timezone.as_deref(), Some("UTC"));
    }

    #[test]
    fn config_locale_applies_when_cli_not_set() {
        let cli = Cli::parse_from(["ccstats", "daily"]);
        let config = Config {
            locale: Some("zh".to_string()),
            ..Default::default()
        };
        let merged = cli.with_config(&config);
        assert_eq!(merged.locale.as_deref(), Some("zh"));
    }

    #[test]
    fn cli_locale_wins_over_config() {
        let cli = Cli::parse_from(["ccstats", "daily", "--locale", "en"]);
        let config = Config {
            locale: Some("zh".to_string()),
            ..Default::default()
        };
        let merged = cli.with_config(&config);
        assert_eq!(merged.locale.as_deref(), Some("en"));
    }

    // --- Defaults ---

    #[test]
    fn default_sort_order_is_asc() {
        let cli = Cli::parse_from(["ccstats", "daily"]);
        assert_eq!(cli.sort_order(), SortOrder::Asc);
    }

    #[test]
    fn default_show_cost_is_true() {
        let cli = Cli::parse_from(["ccstats", "daily"]);
        assert!(cli.show_cost());
    }

    #[test]
    fn empty_config_changes_nothing() {
        let cli = Cli::parse_from(["ccstats", "daily"]);
        let config = Config::default();
        let merged = cli.with_config(&config);
        assert_eq!(merged.sort_order(), SortOrder::Asc);
        assert!(merged.show_cost());
        assert!(!merged.offline);
        assert!(!merged.compact);
        assert!(!merged.breakdown);
        assert!(!merged.debug);
        assert!(!merged.strict_pricing);
        assert!(merged.timezone.is_none());
        assert!(merged.locale.is_none());
    }

    // --- NO_COLOR env var ---

    #[test]
    fn no_color_env_disables_color_in_auto_mode() {
        // SAFETY: test-only, single-threaded access to env var
        unsafe { std::env::set_var("NO_COLOR", "") };
        let cli = Cli::parse_from(["ccstats", "daily"]);
        let result = cli.use_color();
        unsafe { std::env::remove_var("NO_COLOR") };
        assert!(!result);
    }

    #[test]
    fn color_always_overrides_no_color_env() {
        // SAFETY: test-only, single-threaded access to env var
        unsafe { std::env::set_var("NO_COLOR", "1") };
        let cli = Cli::parse_from(["ccstats", "daily", "--color", "always"]);
        let result = cli.use_color();
        unsafe { std::env::remove_var("NO_COLOR") };
        assert!(result);
    }
}
