//! CLI subcommand definitions
//!
//! Defines the available commands for each data source.

use clap::Subcommand;

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
    /// `Codex` CLI usage statistics
    Codex {
        #[command(subcommand)]
        command: Option<CodexCommands>,
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
            Commands::Codex { .. } => SourceCommand::Daily, // Default, handled separately
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
