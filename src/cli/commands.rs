//! CLI subcommand definitions
//!
//! Defines the available commands for each data source.

use clap::{Subcommand, ValueEnum};

/// Dimension to rank in the `top` command
#[derive(Debug, Clone, Copy, Default, ValueEnum, PartialEq, Eq)]
pub(crate) enum TopDimension {
    /// Rank by model (default)
    #[default]
    Model,
    /// Rank by project (requires source with project capability)
    Project,
}

/// Main CLI commands
#[derive(Subcommand)]
pub(crate) enum Commands {
    /// List available data sources and aliases
    Sources,
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
    /// Show tool usage statistics (Read, Bash, Edit, etc.)
    Tools,
    /// Show top N consumers ranked by cost (or tokens when cost is unknown)
    Top {
        /// Dimension to rank by
        #[arg(long, value_enum, default_value_t = TopDimension::Model)]
        dim: TopDimension,
        /// Maximum number of rows to display (1..=1000)
        #[arg(long, default_value_t = 10)]
        limit: usize,
    },
    /// `Codex` CLI usage statistics
    Codex {
        #[command(subcommand)]
        command: Option<CodexCommands>,
    },
    /// Grok CLI local context-token statistics
    Grok {
        #[command(subcommand)]
        command: Option<GrokCommands>,
    },
}

/// Codex-specific subcommands
#[derive(Subcommand)]
pub(crate) enum CodexCommands {
    /// Show daily Codex usage (default)
    Daily,
    /// Show weekly Codex usage
    Weekly,
    /// Show monthly Codex usage
    Monthly,
    /// Show today's Codex usage
    Today,
    /// Show Codex usage by session
    Session,
    /// Output single line for statusline/tmux integration
    Statusline,
}

/// Grok-specific subcommands
#[derive(Subcommand)]
pub(crate) enum GrokCommands {
    /// Show daily Grok local context-token stats (default)
    Daily,
    /// Show weekly Grok local context-token stats
    Weekly,
    /// Show monthly Grok local context-token stats
    Monthly,
    /// Show today's Grok local context-token stats
    Today,
    /// Show Grok local context-token stats by session
    Session,
    /// Show Grok local context-token stats by project
    Project,
    /// Output single line for statusline/tmux integration
    Statusline,
}

/// Normalized command that works across all sources
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SourceCommand {
    Sources,
    Daily,
    Weekly,
    Monthly,
    Today,
    Session,
    Project,
    Blocks,
    Statusline,
    Tools,
    Top { dim: TopDimension, limit: usize },
}

impl SourceCommand {
    /// Check if this is a statusline command (requires quiet mode)
    pub(crate) fn is_statusline(self) -> bool {
        matches!(self, SourceCommand::Statusline)
    }

    /// Check if this command needs today's date filter
    pub(crate) fn needs_today_filter(self) -> bool {
        matches!(self, SourceCommand::Today | SourceCommand::Statusline)
    }
}

impl From<&Commands> for SourceCommand {
    #[allow(clippy::match_same_arms)] // Codex default intentionally maps to Daily
    fn from(cmd: &Commands) -> Self {
        match cmd {
            Commands::Sources => SourceCommand::Sources,
            Commands::Daily => SourceCommand::Daily,
            Commands::Weekly => SourceCommand::Weekly,
            Commands::Monthly => SourceCommand::Monthly,
            Commands::Today => SourceCommand::Today,
            Commands::Session => SourceCommand::Session,
            Commands::Project => SourceCommand::Project,
            Commands::Blocks => SourceCommand::Blocks,
            Commands::Statusline => SourceCommand::Statusline,
            Commands::Tools => SourceCommand::Tools,
            Commands::Top { dim, limit } => SourceCommand::Top {
                dim: *dim,
                limit: *limit,
            },
            Commands::Codex { .. } | Commands::Grok { .. } => SourceCommand::Daily, // Default, handled separately
        }
    }
}

impl From<&Option<CodexCommands>> for SourceCommand {
    fn from(cmd: &Option<CodexCommands>) -> Self {
        match cmd {
            Some(CodexCommands::Daily) | None => SourceCommand::Daily,
            Some(CodexCommands::Weekly) => SourceCommand::Weekly,
            Some(CodexCommands::Monthly) => SourceCommand::Monthly,
            Some(CodexCommands::Today) => SourceCommand::Today,
            Some(CodexCommands::Session) => SourceCommand::Session,
            Some(CodexCommands::Statusline) => SourceCommand::Statusline,
        }
    }
}

impl From<&Option<GrokCommands>> for SourceCommand {
    fn from(cmd: &Option<GrokCommands>) -> Self {
        match cmd {
            Some(GrokCommands::Daily) | None => SourceCommand::Daily,
            Some(GrokCommands::Weekly) => SourceCommand::Weekly,
            Some(GrokCommands::Monthly) => SourceCommand::Monthly,
            Some(GrokCommands::Today) => SourceCommand::Today,
            Some(GrokCommands::Session) => SourceCommand::Session,
            Some(GrokCommands::Project) => SourceCommand::Project,
            Some(GrokCommands::Statusline) => SourceCommand::Statusline,
        }
    }
}

/// Parsed command with optional source hint from subcommand routing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ParsedCommand {
    pub(crate) source_hint: Option<&'static str>,
    pub(crate) command: SourceCommand,
}

/// Parse CLI command into normalized command plus source hint.
pub(crate) fn parse_command(cmd: Option<&Commands>) -> ParsedCommand {
    match cmd {
        Some(Commands::Codex { command }) => ParsedCommand {
            source_hint: Some("codex"),
            command: SourceCommand::from(command),
        },
        Some(Commands::Grok { command }) => ParsedCommand {
            source_hint: Some("grok"),
            command: SourceCommand::from(command),
        },
        Some(cmd) => ParsedCommand {
            source_hint: None,
            command: SourceCommand::from(cmd),
        },
        None => ParsedCommand {
            source_hint: None,
            command: SourceCommand::Daily,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_command_defaults_to_daily_without_source_hint() {
        let parsed = parse_command(None);
        assert_eq!(parsed.command, SourceCommand::Daily);
        assert_eq!(parsed.source_hint, None);
    }

    #[test]
    fn parse_command_codex_sets_source_hint() {
        let parsed = parse_command(Some(&Commands::Codex {
            command: Some(CodexCommands::Session),
        }));
        assert_eq!(parsed.command, SourceCommand::Session);
        assert_eq!(parsed.source_hint, Some("codex"));
    }

    #[test]
    fn parse_command_grok_sets_source_hint() {
        let parsed = parse_command(Some(&Commands::Grok {
            command: Some(GrokCommands::Project),
        }));
        assert_eq!(parsed.command, SourceCommand::Project);
        assert_eq!(parsed.source_hint, Some("grok"));
    }

    #[test]
    fn parse_command_regular_keeps_no_source_hint() {
        let parsed = parse_command(Some(&Commands::Weekly));
        assert_eq!(parsed.command, SourceCommand::Weekly);
        assert_eq!(parsed.source_hint, None);
    }

    #[test]
    fn parse_command_sources_has_no_source_hint() {
        let parsed = parse_command(Some(&Commands::Sources));
        assert_eq!(parsed.command, SourceCommand::Sources);
        assert_eq!(parsed.source_hint, None);
    }
}
