//! Single inventory of registered usage sources.
//!
//! Adding a source is one row in the [`define_sources!`] invocation plus a
//! parser under `src/source/<name>/`. This module expands [`UsageSource`],
//! canonical name tables, and boxed constructors used by the registry.

/// Declare every registered usage source.
///
/// Each row is `Variant => "canonical_name", Constructor::new()`. Optional
/// attributes (`///` docs, `#[serde(rename = "...")]`) apply to the
/// [`UsageSource`] variant. Aliases stay on [`Source::aliases`](super::Source::aliases).
macro_rules! define_sources {
    (
        $(
            $(#[$attr:meta])*
            $variant:ident => $name:literal, $ctor:expr
        ),+ $(,)?
    ) => {
        /// Supported local usage sources.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
        #[non_exhaustive]
        #[serde(rename_all = "snake_case")]
        pub enum UsageSource {
            $(
                $(#[$attr])*
                $variant,
            )+
        }

        impl UsageSource {
            #[cfg(test)]
            pub(crate) const VARIANTS: &[Self] = &[$(Self::$variant,)+];

            #[must_use]
            pub fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $name,)+
                }
            }

            pub(crate) fn from_name(name: &str) -> Option<Self> {
                match name {
                    $($name => Some(Self::$variant),)+
                    _ => None,
                }
            }
        }

        pub(crate) fn boxed_sources() -> Vec<super::BoxedSource> {
            vec![
                $(Box::new($ctor),)+
            ]
        }
    };
}

// Add-source checklist: one row here. Parsers stay in `src/source/<name>/`.
define_sources! {
    /// Claude Code logs under `~/.claude/projects`.
    Claude => "claude", super::claude::ClaudeSource::new(),
    /// `OpenAI` Codex active and archived logs under `~/.codex`, or `CODEX_HOME`.
    Codex => "codex", super::codex::CodexSource::new(),
    /// Cursor composer usage data.
    Cursor => "cursor", super::cursor::CursorSource::new(),
    /// Grok inference usage under `~/.grok/logs/unified.jsonl`, or `GROK_HOME`.
    Grok => "grok", super::grok::GrokSource::new(),
    /// Kimi Code wire logs under `~/.kimi-code/sessions`, or `KIMI_CODE_HOME`.
    Kimi => "kimi", super::kimi::KimiSource::new(),
    /// Gemini CLI chats under `~/.gemini/tmp`, or `GEMINI_CLI_HOME`.
    Gemini => "gemini", super::gemini::GeminiSource::new(),
    /// Amp thread logs under the XDG data directory.
    Amp => "amp", super::amp::AmpSource::new(),
    /// Qwen Code usage ledger under `~/.qwen/usage`, or its root env overrides.
    Qwen => "qwen", super::qwen::QwenSource::new(),
    /// Cline CLI sessions and VS Code extension task logs.
    Cline => "cline", super::cline::ClineSource::new(),
    /// Roo Code VS Code extension task logs.
    #[serde(rename = "roocode")]
    RooCode => "roocode", super::cline_extension::RooCodeSource::new(),
    /// Kilo Code VS Code extension task logs.
    #[serde(rename = "kilocode")]
    KiloCode => "kilocode", super::cline_extension::KiloCodeSource::new(),
    /// `OpenCode` messages in its local `SQLite` database.
    OpenCode => "opencode", super::opencode::OpenCodeSource::new(),
    /// `MiMo` Code messages in its local `SQLite` database.
    MiMoCode => "mimocode", super::opencode::MiMoCodeSource::new(),
    /// Kilo CLI messages in its local `SQLite` database.
    Kilo => "kilo", super::opencode::KiloCliSource::new(),
    /// Pi coding-agent JSONL sessions.
    Pi => "pi", super::pi::PiSource::new(),
    /// Senpi coding-agent JSONL sessions.
    Senpi => "senpi", super::pi::SenpiSource::new(),
    /// Kimchi harness JSONL sessions.
    Kimchi => "kimchi", super::pi::KimchiSource::new(),
    /// Gajae Code v5 JSONL sessions.
    Gjc => "gjc", super::pi_forks::GjcSource::new(),
    /// Prime Agent JSONL sessions.
    Prime => "prime", super::pi_forks::PrimeSource::new(),
    /// Oh My Pi profile-aware JSONL sessions.
    Omp => "omp", super::pi_forks::OmpSource::new(),
    /// GitHub Copilot CLI `OpenTelemetry` JSONL spans.
    Copilot => "copilot", super::copilot::CopilotSource::new(),
    /// Goose per-call usage ledger in its local `SQLite` database.
    Goose => "goose", super::goose::GooseSource::new(),
    /// `OpenClaw` v3 assistant transcript usage.
    OpenClaw => "openclaw", super::openclaw::OpenClawSource::new(),
    /// Xum cumulative per-workspace usage snapshots.
    Xum => "xum", super::xum::XumSource::new(),
    /// Hermes Agent current per-model/task usage ledger.
    Hermes => "hermes", super::hermes::HermesSource::new(),
    /// Reasonix append-only provider usage ledger.
    Reasonix => "reasonix", super::reasonix::ReasonixSource::new(),
    /// Vercel Fx profile-wide generation ledger.
    Fx => "fx", super::fx::FxSource::new(),
    /// Unsloth Studio chat and authenticated API inference receipts.
    Unsloth => "unsloth", super::unsloth::UnslothSource::new(),
    /// `DeepSeek` Harness durable session calls and compaction summaries.
    Dsh => "dsh", super::dsh::DshSource::new(),
}
