//! CLI subcommand definitions
//!
//! Defines the available commands for each data source.

use clap::Subcommand;

/// Main CLI commands
#[derive(Subcommand)]
pub(crate) enum Commands {
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
    /// OpenAI Codex CLI usage statistics
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
    Daily,
    Weekly,
    Monthly,
    Today,
    Session,
    Project,
    Blocks,
    Statusline,
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
    fn from(cmd: &Commands) -> Self {
        match cmd {
            Commands::Daily => SourceCommand::Daily,
            Commands::Weekly => SourceCommand::Weekly,
            Commands::Monthly => SourceCommand::Monthly,
            Commands::Today => SourceCommand::Today,
            Commands::Session => SourceCommand::Session,
            Commands::Project => SourceCommand::Project,
            Commands::Blocks => SourceCommand::Blocks,
            Commands::Statusline => SourceCommand::Statusline,
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

/// Parse CLI command into (`is_codex`, `SourceCommand`)
pub(crate) fn parse_command(cmd: &Option<Commands>) -> (bool, SourceCommand) {
    match cmd {
        Some(Commands::Codex { command }) => (true, SourceCommand::from(command)),
        Some(cmd) => (false, SourceCommand::from(cmd)),
        None => (false, SourceCommand::Daily),
    }
}
