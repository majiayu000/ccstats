# Source registry

Canonical `--source` names come from `src/source/inventory.rs` (`define_sources!`).
Run `ccstats sources` for aliases and capabilities. The [core sources](../README.md#core-sources)
table in the README is the recommended first-run set.

## Complete registry (29)

| Source | Directory | Override | Features |
|--------|-----------|----------|----------|
| Claude Code | `~/.claude/projects/` | `CLAUDE_CONFIG_DIR` | Projects, estimated session windows, deduplication |
| OpenAI Codex | `~/.codex/sessions/` and `archived_sessions/` | `CODEX_HOME` | Reasoning tokens, cumulative-event deduplication |
| All sources | Multiple | Source-specific env vars | Combined daily/weekly/monthly/today/statusline summaries |
| Cursor | Cursor usage API | `CURSOR_API_KEY` / `CURSOR_SESSION_TOKEN` | Per-event tokens, cache tokens, recorded `chargedCents` |
| Grok | `~/.grok/sessions/`, `~/.grok/logs/unified.jsonl` | `GROK_HOME` | Complete turn tokens, per-inference API pricing, coverage metadata. See [grok.md](sources/grok.md) |
| Kimi Code | `~/.kimi-code/sessions/` | `KIMI_CODE_HOME` | Per-turn usage records, projects, cache tokens. See [kimi.md](sources/kimi.md) |
| Gemini CLI | `~/.gemini/tmp/` | `GEMINI_CLI_HOME` | Chat/headless formats, reasoning and cache tokens |
| Amp | `~/.local/share/amp/threads/` | `XDG_DATA_HOME` | Ledger/message reconciliation, cache tokens |
| Qwen Code | `~/.qwen/usage/token-usage-*.jsonl` | `QWEN_RUNTIME_DIR`, `QWEN_HOME` | Native usage ledger, reasoning and cache tokens |
| Cline | `~/.cline/data/sessions/` and VS Code global storage | `CLINE_SESSION_DATA_DIR`, `CLINE_DATA_DIR`, `CLINE_DIR` | CLI and extension sessions, projects, cache tokens |
| Roo Code | VS Code global storage | — | Extension task usage, cache tokens |
| Kilo Code | VS Code global storage | — | Extension task usage, cache tokens |
| OpenCode | Platform data directory under `opencode/opencode*.db` | `OPENCODE_DB`, `XDG_DATA_HOME` | Projects, reasoning/cache tokens, recorded cost, cross-schema deduplication |
| MiMo Code | `~/.local/share/mimocode/mimocode*.db` | `MIMOCODE_DB`, `MIMOCODE_HOME`, `XDG_DATA_HOME` | Projects, reasoning/cache tokens, recorded cost, fork-copy timestamp reconciliation |
| Kilo CLI | `~/.local/share/kilo/kilo*.db` | `KILO_DB`, `XDG_DATA_HOME` | Current + legacy message schemas, recorded cost, fork-copy timestamp reconciliation |
| Pi | `~/.pi/agent/sessions/**/*.jsonl` | `PI_CODING_AGENT_SESSION_DIR`, `PI_CODING_AGENT_DIR` | Projects, assistant + summary usage, cache tokens, branch-copy deduplication |
| Senpi | `~/.senpi/agent/sessions/**/*.jsonl` | `SENPI_CODING_AGENT_SESSION_DIR`, `SENPI_CODING_AGENT_DIR` | Assistant, compaction, branch summary, and tool-result usage with branch-copy deduplication |
| Kimchi | `~/.config/kimchi/harness/sessions/**/*.jsonl` | — | Child transcripts plus remote/missing-child `details.tokenUsage` fallback without rollup double counting |
| Gajae Code | `~/.gjc/agent/sessions/**/*.jsonl` or XDG data root | `GJC_CODING_AGENT_DIR`, `GJC_CONFIG_DIR`, `XDG_DATA_HOME` | v5 patch replay, reasoning/cache tokens, fork deduplication, partial task-rollup residuals |
| Prime Agent | `~/.prime/agent/sessions/**/*.jsonl` | `PRIME_AGENT_SESSION_DIR`, `PRIME_AGENT_CODING_AGENT_DIR` | Project/global session settings, child-attribution reconstruction, recursive transcript and fork deduplication |
| Oh My Pi | Active default/named profile sessions | `PI_CODING_AGENT_SESSION_DIR`, `OMP_PROFILE`, `PI_PROFILE`, `PI_CODING_AGENT_DIR`, `PI_CONFIG_DIR`, `XDG_DATA_HOME` | Profile-aware discovery, orchestration/reasoning/cache tokens, recursive task transcript and fork deduplication |
| GitHub Copilot CLI | `~/.copilot/otel/**/*.jsonl` | `COPILOT_OTEL_FILE_EXPORTER_PATH` | Per-request `chat` spans, reasoning/cache normalization, cross-file deduplication |
| Goose | `~/.local/share/goose/sessions/sessions.db` | `GOOSE_PATH_ROOT`, `XDG_DATA_HOME` | Per-call ledger, projects, cache tokens, provider-reported cost provenance |
| OpenClaw | Agent JSONL/zstd archives and configured/default SQLite stores | `OPENCLAW_STATE_DIR`, `OPENCLAW_CONFIG_PATH`, `OPENCLAW_HOME` | v3 assistant usage, projects, cache TTL tokens, copied-entry deduplication, isolated archive errors, provider-billed cost provenance |
| Xum | `~/.xum/sessions/*/session-usage.json` | `XUM_ROOT` | Five independent token/cost buckets, reasoning/cache tokens, cycle-safe child roll-up reconciliation |
| Hermes Agent | `~/.hermes/state.db` | `HERMES_HOME` | Current per-model/task ledger plus session residual, projects, exact API call counts, reasoning/cache tokens, actual/included cost provenance |
| Reasonix | `~/.reasonix/stats/YYYY-MM-DD.jsonl` | `REASONIX_STATE_HOME`, `REASONIX_HOME` | Per-call/request aggregates, reasoning/cache normalization, occurrence-time complete USD valuations, isolated parse errors |
| Vercel Fx | `~/.fx/usage.jsonl` plus canonical-session-validated recovery backlog | `HOME` | Profile-wide generation IDs, exact timestamps and costs, cache/reasoning normalization, duplicate/conflict detection, fail-closed sidecar recovery |
| Unsloth Studio | `~/.unsloth/studio/studio.db` | `UNSLOTH_STUDIO_HOME`, `STUDIO_HOME` | Chat and API receipts, fork-copy reconciliation, project attribution, response-model precedence, independent reported totals |
| DeepSeek Harness | `~/.dsh/sessions/` session JSONL / zstd | `DSH_HOME` | Projects, retry-aware call accounting, cache/reasoning tokens, compaction calls, fork ownership, concatenated-zstd recovery |

`ccstats daily --source gemini` (and the same pattern for `amp`, `qwen`,
`cline`, `roocode`, `kilocode`, `opencode`, `mimocode`, `kilo`, `pi`, `senpi`,
`kimchi`, `gjc`, `prime`, `omp`, `copilot`, `goose`, `dsh`, …) selects one
source. `--source all` merges every registered source.

## Configuration

ccstats reads an optional TOML config file before command execution. CLI flags
override config values.

Search order:

1. `~/.config/ccstats/config.toml`
2. Platform config directory (for example
   `~/Library/Application Support/ccstats/config.toml` on macOS)
3. `~/.ccstats.toml`

The first existing config file wins. If that file exists but cannot be read,
has invalid TOML, or has a wrong field type, ccstats exits with an error. It
does not fall back to defaults or lower-priority config paths. If no config
file exists, defaults are used.

Example `config.toml`:

```toml
source = "codex"
timezone = "Asia/Shanghai"
locale = "en"
currency = "USD"
offline = true
strict_pricing = true
compact = true
breakdown = false
order = "desc"
color = "auto"
cost = "show"
```

Supported keys:

| Key | Type | Values |
|-----|------|--------|
| `offline` | boolean | `true` or `false` |
| `compact` | boolean | `true` or `false` |
| `no_cost` | boolean | `true` or `false` |
| `no_color` | boolean | `true` or `false` |
| `breakdown` | boolean | `true` or `false` |
| `debug` | boolean | `true` or `false` |
| `strict_pricing` | boolean | `true` or `false` |
| `order` | string | `asc`, `desc` |
| `color` | string | `auto`, `always`, `never` |
| `cost` | string | `show`, `hide` |
| `timezone` | string | IANA timezone such as `UTC` or `Asia/Shanghai` |
| `locale` | string | Locale used for number formatting, such as `en` or `de` |
| `currency` | string | Currency code such as `USD`, `CNY`, or `EUR` |
| `source` | string | Source name or alias such as `claude`, `codex`, or `all` |

## Source-root environment overrides

| Source | Env var | Value | Default when unset |
|--------|---------|-------|--------------------|
| Claude Code | `CLAUDE_CONFIG_DIR` | Claude config root containing `projects/` | `~/.claude` |
| OpenAI Codex | `CODEX_HOME` | Codex root containing `sessions/` and `archived_sessions/` | `~/.codex` |
| Cursor | `CURSOR_API_KEY` or `CURSOR_SESSION_TOKEN` | Admin API key or dashboard session cookie | No default; optional `CURSOR_USAGE_FILE` replay |
| Grok | `GROK_HOME` | Grok root containing `sessions/` | `~/.grok` |
| Kimi Code | `KIMI_CODE_HOME` | Kimi Code root containing `sessions/` | `~/.kimi-code` |
| Gemini CLI | `GEMINI_CLI_HOME` | Gemini CLI root containing `tmp/` | `~/.gemini` |
| Amp | `XDG_DATA_HOME` | User data root containing `amp/threads/` | `~/.local/share` |
| Qwen Code | `QWEN_RUNTIME_DIR`, then `QWEN_HOME` | Qwen root containing `usage/` | `~/.qwen` |
| Cline CLI | `CLINE_SESSION_DATA_DIR` | Cline session directory | `~/.cline/data/sessions` |
| OpenCode | `OPENCODE_DB`; data root follows `XDG_DATA_HOME` | Exact database path, or relative name inside the OpenCode data directory | Platform data directory under `opencode/opencode*.db` |
| MiMo Code | `MIMOCODE_DB`; `MIMOCODE_HOME`; data root follows `XDG_DATA_HOME` | Exact database path, or MiMo home containing `data/` | `~/.local/share/mimocode/mimocode*.db` |
| Kilo CLI | `KILO_DB`; data root follows `XDG_DATA_HOME` | Exact database path, or relative name inside the Kilo data directory | `~/.local/share/kilo/kilo*.db` plus legacy channel databases |
| Pi | `PI_CODING_AGENT_SESSION_DIR`, then `PI_CODING_AGENT_DIR` | Exact sessions directory, or agent directory containing `sessions/` | `~/.pi/agent/sessions` |
| Senpi | `SENPI_CODING_AGENT_SESSION_DIR`, then `SENPI_CODING_AGENT_DIR` | Exact sessions directory, or agent directory containing `sessions/`; `~` is expanded | Nearest project `.senpi/agent/sessions`, then `~/.senpi/agent/sessions` |
| Kimchi | — | Fixed by the Kimchi launcher | `~/.config/kimchi/harness/sessions` |
| Gajae Code | `GJC_CODING_AGENT_DIR`; `GJC_CONFIG_DIR`; data root follows `XDG_DATA_HOME` | Agent directory containing `sessions/`, or config directory name | `~/.gjc/agent/sessions` or migrated `$XDG_DATA_HOME/gjc/sessions` |
| Prime Agent | `PRIME_AGENT_SESSION_DIR`, then `PRIME_AGENT_CODING_AGENT_DIR`; current project/global `settings.json` is also read | Exact sessions directory, or agent directory containing `sessions/` | `~/.prime/agent/sessions` |
| Oh My Pi | `PI_CODING_AGENT_SESSION_DIR`; `OMP_PROFILE`, then `PI_PROFILE`; `PI_CODING_AGENT_DIR`; `PI_CONFIG_DIR`; data root follows `XDG_DATA_HOME` | Exact sessions directory, active profile, or non-profile agent directory | `~/.omp/agent/sessions` or the active profile/XDG equivalent |
| GitHub Copilot CLI | `COPILOT_OTEL_FILE_EXPORTER_PATH` | Exact OTel JSONL exporter file | Also scans `~/.copilot/otel/**/*.jsonl` |
| Goose | `GOOSE_PATH_ROOT`; data root follows `XDG_DATA_HOME` | Absolute Goose path root containing `data/sessions/sessions.db` | `~/.local/share/goose/sessions/sessions.db` |
| OpenClaw | `OPENCLAW_STATE_DIR`, `OPENCLAW_CONFIG_PATH`; effective home follows `OPENCLAW_HOME` | State/config roots containing standard or configured agent transcripts/stores; `~` uses effective home | `~/.openclaw` |
| Xum | `XUM_ROOT` | Current Xum root containing `sessions/` | `~/.xum` |
| Hermes Agent | `HERMES_HOME` | Hermes home containing `state.db` | `~/.hermes/state.db` |
| Reasonix | `REASONIX_STATE_HOME`, then `REASONIX_HOME` | Reasonix state root containing `stats/` | `~/.reasonix` |
| Vercel Fx | `HOME` | Home containing the `.fx` profile ledger and recovery registry | `~/.fx` |
| DeepSeek Harness | `DSH_HOME` | DSH root containing `sessions/`; relative paths resolve from the current directory and `~` is expanded | `~/.dsh` |

Cline also recognizes `CLINE_DATA_DIR` and `CLINE_DIR`. Roo Code and Kilo Code
currently use their standard local directories. Senpi `settings.json` /
`settings.jsonc` `sessionDir` is discovered automatically. If it was launched
with the one-off `--session-dir` flag, set `SENPI_CODING_AGENT_SESSION_DIR` to
that same directory for ccstats.

## Cursor notes

Cursor usage comes from Cursor's usage API, not from local `state.vscdb`
files. Enterprise teams should set `CURSOR_API_KEY`. Individual and self-serve
plans should set `CURSOR_SESSION_TOKEN` to the `WorkosCursorSessionToken`
cookie from [cursor.com/dashboard/usage](https://cursor.com/dashboard/usage).
`ccstats login cursor` stores credentials locally; never pass secrets on argv.

- ccstats does not read local Cursor SQLite auth tokens.
- Project aggregation and estimated session windows (`blocks`) are not supported.
- Dashboard session cookies expire; refresh `CURSOR_SESSION_TOKEN` when requests start failing.
- Self-serve plans may return token counts with `$0` event costs. ccstats records that billed amount instead of estimating Cursor subscription cost from LiteLLM prices.

## Windows directory overrides

Windows uses the native user profile and application directories by default.
An explicit nonempty absolute `HOME` overrides the profile root for source logs
and the `~/.config` / `~/.ccstats.toml` search paths. It does not remap config,
data, or cache directories to `$HOME/.config`, `$HOME/.local/share`, or
`$HOME/.cache`. Native known folders stay on the search path.

`XDG_CONFIG_HOME`, `XDG_DATA_HOME`, and `XDG_CACHE_HOME` override those
respective directories when set to a nonempty absolute path. Source-specific
overrides such as `CODEX_HOME` still take precedence for source logs.

In PowerShell, set a source root with `$env:CODEX_HOME = 'C:\path\to\.codex'`.
