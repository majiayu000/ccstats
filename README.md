# ccstats

[![CI](https://github.com/majiayu000/ccstats/actions/workflows/ci.yml/badge.svg)](https://github.com/majiayu000/ccstats/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/ccstats.svg)](https://crates.io/crates/ccstats)
[![Crates.io Downloads](https://img.shields.io/crates/d/ccstats.svg)](https://crates.io/crates/ccstats)
[![GitHub Release](https://img.shields.io/github/v/release/majiayu000/ccstats)](https://github.com/majiayu000/ccstats/releases)
[![docs.rs](https://img.shields.io/docsrs/ccstats)](https://docs.rs/ccstats/latest/ccstats/)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://github.com/majiayu000/ccstats/blob/main/LICENSE)

![ccstats token and cost analytics card](docs/branding/readme-card.png)

`ccstats` is a fast, local-first CLI, Rust SDK, and desktop workbench for token
and cost analytics across 29 AI coding-agent data sources.

One binary turns the usage metadata your coding agents already produce into
terminal reports and structured JSON/CSV. No ccstats account or telemetry is
required.

## 30-second start

```bash
brew install majiayu000/tap/ccstats
ccstats doctor
ccstats daily --source all
```

`doctor` is read-only and never contacts remote providers. It shows which
registered sources are detected or configured, plus a practical hint for
missing sources.

## Why ccstats

- **Fast and portable** — a small Rust binary with no Node.js or Python runtime.
- **Automation-ready** — table, JSON, CSV, jq filtering, statusline output, and
  a reusable Rust SDK share the same accounting logic.
- **Accuracy over guesswork** — source-aware deduplication, cache/reasoning token
  handling, parse-quality metadata, strict pricing mode, and visible pricing
  provenance.
- **Useful beyond spend** — daily/weekly/monthly trends, projects, sessions,
  top consumers, Claude tool usage, and Codex quota pace.
- **Local-first** — local session data stays on the machine; network access is
  bounded by feature and pricing refreshes can be disabled with `--offline`.

ccstats keeps its CLI and Rust SDK as the accounting engine. The desktop app is
a local investigation surface over the same source registry and summary APIs;
it does not upload transcripts or substitute mock data when a source fails.

## Desktop application development

The desktop app requires Node.js 20.19+ (or 22.12+), Rust 1.88+, and the
platform prerequisites for [Tauri 2](https://v2.tauri.app/start/prerequisites/).
From the repository root:

```bash
cd desktop
npm install
npm run tauri -- dev
```

The app discovers detected or configured sources at startup and opens the first
ready ledger. If none are ready, it opens Diagnostics instead of presenting an
empty default source. “All Sources” scans ready ledgers only; all 29 registered
sources remain available for explicit inspection and setup.

The workspace is organized by the questions a usage investigation asks:

- **Observe** — totals, live 15-second monitoring, top consumers, and spikes.
- **Explain** — model turns, tool calls, projects, sessions, and daily history.
- **Trust** — pricing provenance, API-equivalent coverage, Codex quota, budget,
  and source readiness.
- **Devices** — explicit JSON snapshot exchange and a local SQLite rollup.

Unknown, partial, fallback-priced, malformed, and provider-adjusted values stay
visible as evidence states. Live, History, Limits, and Machines treat only real
usage backed by recorded, live, or fresh cached pricing with complete coverage
as exact cost. Machine totals use canonical USD and evaluate Today, This week,
and This month freshness independently using the configured CLI timezone.

Production installers are built from `desktop/` by the tag-triggered Release
workflow. Local packaging:

```bash
cd desktop
npm ci
npm run tauri -- build
```

macOS produces a DMG, Windows an MSI, and Linux an AppImage. See
[docs/RELEASING.md](docs/RELEASING.md) for GitHub Release artifacts.

Focused desktop checks:

```bash
cd desktop
npm run build
npm run test:e2e
cargo test --manifest-path src-tauri/Cargo.toml
npm run test:e2e:native
```

Playwright exercises the renderer contract by injecting an explicit window bridge
before the page loads. Rust tests cover the command boundary, and the native test
launches a debug app through an embedded WebDriver before crossing Tauri IPC into
the real ccstats SDK. Production builds call those commands directly and have no
sample-data fallback.

Usage files and transcripts remain local; pricing refreshes and sources configured
with remote APIs may still make their documented network requests.

## Start with common sources

| Source | Usage input | Start here |
|--------|-------------|------------|
| Claude Code | `~/.claude/projects/` | `ccstats today` |
| OpenAI Codex | `~/.codex/sessions/` and local quota snapshots | `ccstats codex today` / `ccstats quota` |
| Cursor | Official usage API or an explicit replay file | `ccstats today --source cursor` |
| Grok | `~/.grok/logs/unified.jsonl` with session fallback | `ccstats grok today` |
| Kimi Code | `~/.kimi-code/sessions/` wire logs | `ccstats kimi today` |

The complete registry contains 29 sources, including DeepSeek Harness. See the
full [source table](#supported-data-sources) below. Run `ccstats sources` for
aliases and per-source capabilities, or `ccstats doctor --json` for
machine-readable setup diagnostics.

## Privacy and network access

ccstats extracts usage metadata from known source locations. It does not store
or upload prompt text, responses, or source-code content. Local parsing requires
no ccstats account.

Network access occurs only when a selected feature needs it:

- pricing refresh downloads the public LiteLLM pricing catalog;
- non-USD output downloads exchange rates;
- Cursor usage calls Cursor's API with credentials supplied by the user.

Use `--offline` to prevent pricing and exchange-rate refreshes. See the full
[privacy and data-access reference](docs/PRIVACY.md) for files read, caches
written, and endpoints contacted.

## Installation

### Homebrew (macOS/Linux)

```bash
brew install majiayu000/tap/ccstats
```

### Cargo binstall (prebuilt binary)

```bash
cargo binstall ccstats
```

### Cargo install (from source)

```bash
cargo install ccstats
```

### Shell script

```bash
curl -fsSL https://raw.githubusercontent.com/majiayu000/ccstats/main/install.sh | sh

# Install a specific version
curl -fsSL https://raw.githubusercontent.com/majiayu000/ccstats/main/install.sh | VERSION=v0.5.0 sh
```

### Desktop app

Download installers and SHA-256 checksums from [GitHub Releases](https://github.com/majiayu000/ccstats/releases):

- macOS Apple Silicon: `ccstats-desktop-aarch64-apple-darwin.dmg`
- macOS Intel: `ccstats-desktop-x86_64-apple-darwin.dmg`
- Windows: `ccstats-desktop-x86_64-pc-windows-msvc.msi`
- Linux x64: `ccstats-desktop-x86_64-unknown-linux-gnu.AppImage`
- Linux ARM64: `ccstats-desktop-aarch64-unknown-linux-gnu.AppImage`

The desktop app is a local investigation surface over the same accounting
engine as the CLI. It does not create a ccstats account or upload transcripts.

### Manual download

Download prebuilt CLI archives and SHA-256 checksums from [GitHub Releases](https://github.com/majiayu000/ccstats/releases).

### Upgrade and uninstall

```bash
# Homebrew
brew upgrade majiayu000/tap/ccstats

# Cargo
cargo install ccstats --locked --force

# Uninstall Homebrew or Cargo installs
brew uninstall ccstats
cargo uninstall ccstats
```

## Quick Start (Codex)

```bash
# Install
brew install majiayu000/tap/ccstats

# Today
ccstats codex today

# Daily trend
ccstats codex daily

# Weekly quota pace, remaining percentage, and reset time
ccstats quota

# Same result via unified source flag
ccstats daily --source codex
```

## Quick Start (Cursor)

Cursor usage comes from Cursor's usage API, not from local `state.vscdb` files. Enterprise teams should set `CURSOR_API_KEY`. Individual and self-serve plans should set `CURSOR_SESSION_TOKEN` to the `WorkosCursorSessionToken` cookie from [cursor.com/dashboard/usage](https://cursor.com/dashboard/usage).

```bash
# Install
brew install majiayu000/tap/ccstats

# Today
ccstats today --source cursor

# Daily trend
ccstats daily --source cursor

# Same source via alias
ccstats daily --source cur
```

## Quick Start (Grok)

Grok support reports complete, globally deduplicated token totals from session `turn_completed.usage` records. It separately prices each observed `shell.turn.inference_done` request using xAI's public short- or long-context API rate. Because Grok trims `unified.jsonl`, ccstats shows the exact observed API equivalent, a coverage-adjusted estimate, and a short/long-context range. It also displays `costUsdTicks` as a separate provider metric. Neither value is labeled as the user's actual subscription charge, which is unavailable in the local logs.

```bash
# Install
brew install majiayu000/tap/ccstats

# Today's Grok usage
ccstats grok today

# Daily Grok usage
ccstats grok

# Same source via alias
ccstats daily --source gx
```

## Quick Start (Kimi Code)

Kimi Code support reads per-turn `usage.record` entries from wire logs under `~/.kimi-code/sessions/`, including sub-agent usage, and reports actual input/output/cache token usage per turn.

```bash
# Install
brew install majiayu000/tap/ccstats

# Today's usage and cost
ccstats kimi today

# Daily breakdown
ccstats kimi

# Same source via alias
ccstats daily --source km
```

## Quick Start (Additional Sources)

```bash
ccstats daily --source gemini
ccstats daily --source amp
ccstats daily --source qwen
ccstats daily --source cline
ccstats daily --source roocode
ccstats daily --source kilocode
ccstats daily --source opencode
ccstats daily --source mimocode
ccstats daily --source kilo
ccstats daily --source pi
ccstats daily --source senpi
ccstats daily --source kimchi
ccstats daily --source gjc
ccstats daily --source prime
ccstats daily --source omp
ccstats daily --source copilot
ccstats daily --source goose
ccstats daily --source dsh
```

Gemini reads both chat JSON and headless JSONL usage. Amp reconciles its usage
ledger with assistant-message usage without double counting. Qwen reads its
native usage ledger and separates cached input from uncached input. Cline reads
both CLI sessions and its VS Code extension task logs; Roo Code and Kilo Code
use the same extension-log algorithm. The OpenCode family reconciles dual
schemas and fork-copied history. The Pi-derived sources use separate rules for
GJC task residuals, Prime child attribution, and OMP task rollups so parent and
child usage is counted exactly once. Source-recorded OpenCode/Pi-family costs
and provider-reported Goose ledger costs retain their provenance. Copilot's
documented monetary field has no published currency code, so it remains
separate from ccstats' USD estimate instead of being mislabeled.
DeepSeek Harness reads the durable session ledger rather than message text. It
reconciles streamed usage with final responses and retries, excludes forked
history through `seedLength`, counts compaction calls separately, and validates
plain or concatenated zstd persistence before accepting usage.

## Crate Documentation

- docs.rs: <https://docs.rs/ccstats/latest/ccstats/>
- crates.io: <https://crates.io/crates/ccstats>
- The crate-level Rustdoc in `src/lib.rs` explains the SDK entry points and CLI runtime.

## Rust SDK

`ccstats` can be used as a Rust library when another app needs structured local usage and cost data without spawning the CLI.

```rust
use ccstats::{SummaryOptions, UsageRange, UsageSource, summarize_cost_with_cli_config};

let summary = summarize_cost_with_cli_config(SummaryOptions {
    source: UsageSource::Codex,
    range: UsageRange::Today,
    ..SummaryOptions::default()
})?;

println!("today: ${:.2}", summary.cost_usd.unwrap_or(0.0));
```

The SDK uses the same source registry, parsers, aggregation logic, pricing cache, and fallback pricing as the CLI. Use `summarize_cost_with_cli_config` when SDK output should follow the same persisted CLI defaults for timezone, offline pricing, strict pricing, and currency. Use `summarize_cost` when the caller wants fully explicit options. Returned summaries include total tokens, cache read/create tokens, cache hit rate, reasoning tokens, per-model breakdowns, `cost_usd`, and an optional converted `cost` when `SummaryOptions::currency` is set.

Use `UsageRange::TimestampRange` with inclusive UTC `DateTime<Utc>` bounds when an app must align usage to an exact provider quota window instead of whole local dates.

Codex weekly quota pace is also available as structured SDK data without
spawning the CLI:

```rust
use ccstats::load_codex_weekly_quota;

let quota = load_codex_weekly_quota(None)?;
println!("weekly used: {:.1}%", quota.used_pct);
println!("projected at reset: {:.1}%", quota.projected_pct_at_reset);
```

Pass `Some(codex_home)` to read an explicit Codex home without modifying
process environment variables. The explicit path is authoritative and never
falls back to `CODEX_HOME` or `~/.codex`. Missing, stale, malformed, and
unreadable snapshots return typed `CodexQuotaError` values.

Apps that need several windows at once can use the batch API so source logs,
pricing, and currency are loaded once for the request:

```rust
use ccstats::{MultiSummaryOptions, UsageRange, UsageSource, summarize_cost_ranges};

let overview = summarize_cost_ranges(MultiSummaryOptions {
    source: UsageSource::Claude,
    ranges: vec![
        UsageRange::Today,
        UsageRange::ThisWeek,
        UsageRange::ThisMonth,
    ],
    timezone: None,
    offline: true,
    strict_pricing: false,
    currency: Some("USD".to_string()),
})?;

for summary in overview.summaries {
    println!("{:?}: ${:.2}", summary.range, summary.cost_usd.unwrap_or(0.0));
}
```

`UsageRange::ThisWeek` and `ThisMonth` are current-period date filters
(Monday–today / month-start–today in the selected timezone). They are not the
CLI `weekly` / `monthly` commands, which group already-filtered history by
period instead of loading only the current week or month.

## Usage

### Claude Code

```bash
# Today's usage
ccstats today

# Daily breakdown
ccstats daily

# Weekly grouping of history (not "this week")
ccstats weekly
ccstats weekly --since 20260901 --until 20260915

# Monthly grouping of history (not "this month")
ccstats monthly

# By project
ccstats project

# By session
ccstats session

# Estimated 5-hour session windows (not an official Anthropic billing reset)
ccstats blocks

# Top-N leaderboard (ranks by cost, falls back to tokens when costs unknown)
ccstats top                          # top 10 models by cost
ccstats top --dim project --limit 5  # top 5 projects

# With model breakdown
ccstats today -b

# JSON output
ccstats today -j

# Debug mode (timing info)
ccstats today --debug

# Debug model pricing resolution (written to stderr)
# Example: Pricing: glm-5.2 -> glm-5.2 (live)
# Unknown models are reported as: Pricing: <model> -> no match (unknown)
ccstats today --breakdown --strict-pricing --debug
```

`weekly` and `monthly` are aggregation grains: they group already-filtered
history by week (Monday start) or calendar month. They do not default to the
current period. Bound dates with `--since` / `--until`. There is no `--current`
flag. SDK `UsageRange::ThisWeek` / `ThisMonth` are a different concept: those
filter to the current period in the selected timezone (Monday–today /
month-start–today).

By default, ccstats checks Claude Code logs under `~/.claude/projects/`.
If Claude Code uses a moved config directory, set `CLAUDE_CONFIG_DIR` to the
Claude config root:

```bash
CLAUDE_CONFIG_DIR="/path/to/claude-config" ccstats daily --source claude
```

### OpenAI Codex

```bash
# Codex subcommand mode
ccstats codex daily

# Or use unified source flag
ccstats daily --source codex

# Today's Codex usage
ccstats codex today

# Daily Codex breakdown
ccstats codex daily

# Weekly Codex summary
ccstats codex weekly

# Provider weekly quota pace (same as `ccstats quota`)
ccstats codex quota

# By session
ccstats codex session

# With model breakdown
ccstats codex today -b
```

By default, ccstats checks Codex sessions under `~/.codex/sessions/`. You can
override the Codex home directory with `CODEX_HOME`:

```bash
CODEX_HOME="/path/to/.codex" ccstats codex daily
```

#### Codex weekly quota estimate

`ccstats quota` reads the newest server-provided 10,080-minute rate-limit
snapshot from local Codex session logs. It reports the used and remaining
percentages, reset time, projected percentage at reset, and an estimated
depletion time when the current pace would exceed 100%. It also prices local
usage from the exact active quota window and divides it by the reported used
fraction to estimate the full week's API-equivalent USD value and token count.

```bash
# Human-readable table
ccstats quota

# Equivalent nested command
ccstats codex quota

# Machine-readable output
ccstats quota --json
ccstats quota --csv
```

The dollar and token figures are approximations, not official provider
allowances. They vary with the current model and cache mix; token totals are
only comparable while that mix stays similar. Dollar values use ccstats' current
model price resolution; request-level pricing tiers may not be represented. Use
`--no-cost` to omit the value estimate. Quota estimates are reported in USD, so
an explicit non-USD `--currency` is rejected. If no current weekly snapshot
exists, the command exits with an error instead of estimating quota from token
totals.

### Cursor

Cursor uses the unified source flag rather than a dedicated subcommand.

```bash
# Today's Cursor usage
ccstats today --source cursor

# Daily Cursor breakdown
ccstats daily --source cursor

# Weekly Cursor summary
ccstats weekly --source cursor

# By session/conversation
ccstats session --source cursor

# Cursor alias
ccstats daily --source cur
```

Authenticate with one of:

```bash
# Enterprise Admin API key from cursor.com/dashboard/api
CURSOR_API_KEY="..." ccstats daily --source cursor

# Dashboard session cookie for individual / self-serve plans
CURSOR_SESSION_TOKEN="..." ccstats daily --source cursor
```

`CURSOR_API_KEY` calls `POST https://api.cursor.com/teams/filtered-usage-events`. `CURSOR_SESSION_TOKEN` calls the dashboard usage-events endpoint used by [cursor.com/dashboard/usage](https://cursor.com/dashboard/usage). For tests or offline replay, point `CURSOR_USAGE_FILE` at a saved JSON payload.

Current limitations:

- ccstats does not read local Cursor SQLite auth tokens. Set `CURSOR_API_KEY` or `CURSOR_SESSION_TOKEN` explicitly.
- Project aggregation and estimated session windows (`blocks`) are not supported for Cursor.
- Dashboard session cookies expire; refresh `CURSOR_SESSION_TOKEN` when requests start failing.
- Self-serve plans may return token counts with `$0` event costs. ccstats records that billed amount instead of estimating Cursor subscription cost from LiteLLM prices.

### Grok

```bash
# Today's Grok usage
ccstats grok today

# Daily Grok usage
ccstats grok

# Weekly Grok usage
ccstats grok weekly

# By session
ccstats grok session

# By project
ccstats grok project

# Grok alias
ccstats daily --source gx
```

By default, ccstats uses:

- `~/.grok/sessions/**/updates.jsonl` for complete per-turn token totals
- `~/.grok/logs/unified.jsonl` for per-inference API-equivalent pricing
- `~/.grok/sessions/**/summary.json` for model, project, and session metadata
- the platform application-data directory under `ccstats/grok/<source-root>/inference-v1.jsonl` for the durable, deduplicated ledger

For Grok 4.5 and 4.6, requests below 200k prompt tokens use the short-context rates. Requests at or above 200k use the long-context input, cached-input, and output rates for the entire inference. `completion_tokens` already includes reasoning, so ccstats does not charge reasoning twice. Rates follow the [xAI pricing reference](https://docs.x.ai/developers/pricing).

Structured period output includes the backward-compatible `api_equivalent_cost_coverage` object plus `grok_cost_summary`. The latter separates `api_equivalent` observed/estimated/range values, the provider-reported `costUsdTicks` metric and its token coverage, and `actual_billed_usd: null`. If request telemetry and complete turn totals cannot be reconciled, coverage is marked `mismatch` and ccstats does not publish an estimate. If a session has no `turn_completed.usage`, ccstats retains its explicitly labeled `estimated_proxy` context-snapshot fallback.

You can override the Grok home directory with `GROK_HOME`:

```bash
GROK_HOME="/path/to/.grok" ccstats grok
```

Current limitations:

- The durable ledger starts when ccstats first observes an inference. It cannot recover records Grok trimmed before the first run or between ccstats runs, so incomplete request coverage produces an estimated API equivalent and a short/long-context range.
- `turn_completed.usage.costUsdTicks` is reported separately as a provider metric. xAI does not document this field as public API list price or as the user's actual subscription charge.
- Grok models without a published ccstats per-inference tier remain unpriced and reduce priced-token coverage.
- Grok estimated session windows (`blocks`) are not supported.

### Kimi Code

```bash
# Today's Kimi Code usage and cost
ccstats kimi today

# Daily Kimi Code breakdown
ccstats kimi

# Weekly Kimi Code summary
ccstats kimi weekly

# By session
ccstats kimi session

# By project
ccstats kimi project

# Kimi alias
ccstats daily --source km
```

By default, ccstats reads Kimi Code wire logs under:

- `~/.kimi-code/sessions/*/*/agents/*/wire.jsonl` (main and sub-agent per-turn `usage.record` entries)
- `~/.kimi-code/session_index.jsonl` for session-to-project mapping

You can override the Kimi Code home directory with `KIMI_CODE_HOME`:

```bash
KIMI_CODE_HOME="/path/to/.kimi-code" ccstats kimi
```

Current limitations:

- Kimi Code subscription models (e.g. `kimi-code/k3`) have no public per-token pricing; costs use fallback estimates based on Moonshot's official `kimi-k2.6` API rates and are marked as `fallback` in structured output. Use `--strict-pricing` to show N/A instead.
- Cache creation tokens are reported but priced at $0 by the Kimi fallback estimate (Moonshot does not publish a separate cache-creation rate).
- Kimi estimated session windows (`blocks`) and tool-call statistics are not supported.

### Common Options

```bash
# Bucket by timezone
ccstats daily --timezone UTC

# Locale-aware number formatting
ccstats monthly --locale de

# Filter by date
ccstats daily --since 20260101 --until 20260131

# Monthly budget forecast (uses --until as the as-of date when present)
ccstats monthly --monthly-budget 25 --until 20260415

# Select data source explicitly (supports aliases)
ccstats daily --source codex

# Combine all supported data sources
ccstats monthly --source all

# Cursor source (usage API)
ccstats daily --source cursor

# Cursor alias
ccstats daily --source cur

# Grok source and alias
ccstats daily --source grok
ccstats daily --source gx

# Kimi Code source and alias
ccstats daily --source kimi
ccstats daily --source km

# Additional local agent sources
ccstats daily --source gemini
ccstats daily --source amp
ccstats daily --source qwen
ccstats daily --source cline
ccstats daily --source roocode
ccstats daily --source kilocode
ccstats daily --source opencode
ccstats daily --source mimocode
ccstats daily --source kilo
ccstats daily --source pi
ccstats daily --source senpi
ccstats daily --source kimchi

# Offline mode (use cached pricing)
ccstats today -O

# Compact output
ccstats today -c

# Hide cost column
ccstats today --no-cost
```

### Configuration

ccstats reads an optional TOML config file before command execution. CLI flags
override config values.

Search order:

1. `~/.config/ccstats/config.toml`
2. Platform config directory: for example
   `~/Library/Application Support/ccstats/config.toml` on macOS
3. `~/.ccstats.toml`

The first existing config file wins. If that file exists but cannot be read,
has invalid TOML, or has a wrong field type, ccstats exits with an error. It
does not fall back to defaults or lower-priority config paths. If no config file
exists, defaults are used.

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
| `source` | string | Source name or alias such as `claude`, `codex`, `opencode`, `mimocode`, `kilo`, `pi`, `senpi`, `kimchi`, `gjc`, `prime`, `omp`, `copilot`, `goose`, `openclaw`, `xum`, `hermes`, `dsh`, or `all` |

Source root env overrides are independent of config keys:

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
currently use their standard local directories.
Senpi `settings.json`/`settings.jsonc` `sessionDir` is discovered automatically. If it was launched
with the one-off `--session-dir` flag, set `SENPI_CODING_AGENT_SESSION_DIR` to
that same directory for ccstats.

### Session CSV Columns

`ccstats session --csv` now includes:

- `reasoning_tokens`
- `cache_creation_tokens`
- `cache_read_tokens`
- `cache_hit_rate`

### Cache Hit Rate

Statistical table, JSON, CSV, statusline, top, session, project, and block outputs
report prompt-cache hit rate as:

```text
cache_read / (input + cache_creation + cache_read) * 100
```

Table output uses one decimal place and a `%` suffix. JSON uses the numeric
`cache_hit_rate` field, while CSV uses a two-decimal `cache_hit_rate` column.
Claude, Codex, Cursor, Grok, Kimi Code, Gemini CLI, Amp, Qwen Code, Cline, Roo
Code, Kilo Code, OpenCode, MiMo Code, Kilo CLI, Pi, Senpi, Kimchi, Gajae Code,
Prime Agent, Oh My Pi, GitHub Copilot CLI, Goose, OpenClaw, Xum, Hermes Agent,
Reasonix, Vercel Fx, Unsloth Studio, and DeepSeek Harness expose the required
cache-read metric. Mixed `--source all` output reports the aggregate rate across
all selected usage.

### Parsing Warnings

When malformed JSONL records are encountered, ccstats reports them in stderr:

```text
Warning: ignored <N> malformed records
```

## Supported Data Sources

| Source | Directory | Override | Features |
|--------|-----------|----------|----------|
| Claude Code | `~/.claude/projects/` | `CLAUDE_CONFIG_DIR` | Projects, Estimated session windows, Deduplication |
| OpenAI Codex | `~/.codex/sessions/` and `~/.codex/archived_sessions/` | `CODEX_HOME` | Reasoning Tokens, cumulative-event deduplication |
| All Sources | Multiple | Source-specific env vars | Combined daily/weekly/monthly/today/statusline summaries |
| Cursor | Cursor usage API | `CURSOR_API_KEY` / `CURSOR_SESSION_TOKEN` | Per-event tokens, cache tokens, recorded `chargedCents` |
| Grok | `~/.grok/sessions/`, `~/.grok/logs/unified.jsonl` | `GROK_HOME` | Complete turn tokens, per-inference API pricing, coverage metadata, Projects, Cache / reasoning tokens, 200k pricing tier |
| Kimi Code | `~/.kimi-code/sessions/` | `KIMI_CODE_HOME` | Per-turn usage records, Projects, Cache tokens |
| Gemini CLI | `~/.gemini/tmp/` | `GEMINI_CLI_HOME` | Chat/headless formats, Reasoning and cache tokens |
| Amp | `~/.local/share/amp/threads/` | `XDG_DATA_HOME` | Ledger/message reconciliation, Cache tokens |
| Qwen Code | `~/.qwen/usage/token-usage-*.jsonl` | `QWEN_RUNTIME_DIR`, `QWEN_HOME` | Native usage ledger, Reasoning and cache tokens |
| Cline | `~/.cline/data/sessions/` and VS Code global storage | `CLINE_SESSION_DATA_DIR`, `CLINE_DATA_DIR`, `CLINE_DIR` | CLI and extension sessions, Projects, Cache tokens |
| Roo Code | VS Code global storage | — | Extension task usage, Cache tokens |
| Kilo Code | VS Code global storage | — | Extension task usage, Cache tokens |
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
| Goose | `~/.local/share/goose/sessions/sessions.db` | `GOOSE_PATH_ROOT`, `XDG_DATA_HOME` | Per-call ledger, Projects, cache tokens, provider-reported cost provenance |
| OpenClaw | Agent JSONL/zstd archives and configured/default SQLite stores | `OPENCLAW_STATE_DIR`, `OPENCLAW_CONFIG_PATH`, `OPENCLAW_HOME` | v3 assistant usage, Projects, cache TTL tokens, copied-entry deduplication, isolated archive errors, provider-billed cost provenance |
| Xum | `~/.xum/sessions/*/session-usage.json` | `XUM_ROOT` | Five independent token/cost buckets, reasoning/cache tokens, cycle-safe child roll-up reconciliation |
| Hermes Agent | `~/.hermes/state.db` | `HERMES_HOME` | Current per-model/task ledger plus session residual, Projects, exact API call counts, reasoning/cache tokens, actual/included cost provenance |
| Reasonix | `~/.reasonix/stats/YYYY-MM-DD.jsonl` | `REASONIX_STATE_HOME`, `REASONIX_HOME` | Per-call/request aggregates, reasoning/cache normalization, occurrence-time complete USD valuations, isolated parse errors |
| Vercel Fx | `~/.fx/usage.jsonl` plus canonical-session-validated recovery backlog | `HOME` | Profile-wide generation IDs, exact timestamps and costs, cache/reasoning normalization, duplicate/conflict detection, fail-closed sidecar recovery |
| Unsloth Studio | `~/.unsloth/studio/studio.db` | `UNSLOTH_STUDIO_HOME`, `STUDIO_HOME` | Chat and API receipts, fork-copy reconciliation, project attribution, response-model precedence, independent reported totals |
| DeepSeek Harness | `~/.dsh/sessions/<project>/<session>/session.jsonl[.zstd]` | `DSH_HOME` | Projects, retry-aware call accounting, cache/reasoning tokens, compaction calls, fork ownership, concatenated-zstd recovery |

## Architecture

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for:
- Adding new data sources
- Data flow and processing pipeline
- Caching mechanism
- Architecture and module boundaries

See [docs/algorithm/authoritative-token-accounting.md](docs/algorithm/authoritative-token-accounting.md) for:
- Token accounting rules
- Source-specific normalization
- Deduplication semantics

See [docs/research/provider-algorithm-audit-2026-08-31.md](docs/research/provider-algorithm-audit-2026-08-31.md)
for the pinned competitor comparison, official-schema evidence, and
adopt/adapt/reject decisions behind the additional providers.

## License

MIT. See [LICENSE](LICENSE).
