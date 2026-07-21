# ccstats

[![Crates.io](https://img.shields.io/crates/v/ccstats.svg)](https://crates.io/crates/ccstats)
[![GitHub Release](https://img.shields.io/github/v/release/majiayu000/ccstats)](https://github.com/majiayu000/ccstats/releases)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://github.com/majiayu000/ccstats/blob/main/LICENSE)

![ccstats token and cost analytics card](docs/branding/readme-card.png)

`ccstats` is a fast CLI for token and cost usage analytics for Claude Code, OpenAI Codex, Cursor, Grok, and Kimi Code logs.

Search keywords: `claude code usage stats`, `codex usage stats`, `cursor usage stats`, `token usage cli`, `ai token cost tracker`.

## Highlights

- Fast local analysis of usage JSONL logs
- Claude Code support (`~/.claude/projects/`)
- OpenAI Codex support (`~/.codex/sessions/`)
- Experimental Cursor support (`Cursor/User/globalStorage/state.vscdb`)
- Grok support (`~/.grok/sessions/`)
- Kimi Code support (`~/.kimi-code/sessions/`)
- Daily/weekly/monthly/project/session views
- Top-N leaderboard ranking models or projects by cost share
- Optional model-level token and cost breakdown
- Reusable Rust SDK for embedding local usage and cost summaries in other apps

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
curl -fsSL https://raw.githubusercontent.com/majiayu000/ccstats/main/install.sh | VERSION=v0.2.63 sh
```

### Manual download
Download prebuilt archives and SHA-256 checksums from [GitHub Releases](https://github.com/majiayu000/ccstats/releases).

## Quick Start (Codex)

```bash
# Install
brew install majiayu000/tap/ccstats

# Today
ccstats codex today

# Daily trend
ccstats codex daily

# Same result via unified source flag
ccstats daily --source codex
```

## Quick Start (Cursor)

Cursor support is experimental because Cursor's local database schema is not a public API. ccstats reads local SQLite `tokenCount` fields only and does not estimate missing usage.

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

Grok support reads local session `summary.json`, `signals.json`, and fallback `updates.jsonl` metadata under `~/.grok/sessions/`. These files expose local context-token snapshots, not precise provider input/output billable usage or Grok account quota usage, so ccstats reports Grok context tokens as input tokens.

```bash
# Install
brew install majiayu000/tap/ccstats

# Today's local context-token trend
ccstats grok today

# Daily local context-token trend
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

## Usage

### Claude Code

```bash
# Today's usage
ccstats today

# Daily breakdown
ccstats daily

# Weekly summary
ccstats weekly

# Monthly summary
ccstats monthly

# By project
ccstats project

# By session
ccstats session

# 5-hour billing blocks
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

### Cursor (Experimental)

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

By default, ccstats checks these local Cursor databases:

- macOS: `~/Library/Application Support/Cursor/User/globalStorage/state.vscdb`
- Linux: `~/.config/Cursor/User/globalStorage/state.vscdb`
- `workspaceStorage/*/state.vscdb` under the same Cursor user directory

You can override the Cursor user directory with `CURSOR_HOME`:

```bash
CURSOR_HOME="/path/to/Cursor/User" ccstats daily --source cursor
```

Current limitations:

- Only explicit `tokenCount`/usage fields are counted.
- Project aggregation and 5-hour billing blocks are not supported for Cursor.
- Cache creation, cache read, and reasoning token fields are reported as zero unless Cursor exposes them directly in a supported local record.

### Grok

```bash
# Today's Grok local context-token trend
ccstats grok today

# Daily Grok local context-token breakdown
ccstats grok

# Weekly Grok local context-token summary
ccstats grok weekly

# By session
ccstats grok session

# By project
ccstats grok project

# Grok alias
ccstats daily --source gx
```

By default, ccstats checks Grok session files under:

- `~/.grok/sessions/**/summary.json`
- `~/.grok/sessions/**/signals.json`
- `~/.grok/sessions/**/updates.jsonl` when `signals.json` is missing

You can override the Grok home directory with `GROK_HOME`:

```bash
GROK_HOME="/path/to/.grok" ccstats grok
```

Current limitations:

- Grok local session files expose context token usage, not exact provider input/output usage.
- These local context-token totals may not match Grok account, quota, or 5-hour usage UI totals when those views use server-side accounting.
- ccstats reports Grok context tokens as input tokens and leaves output, cache creation, cache read, and reasoning token fields at zero.
- Grok 5-hour billing blocks are not supported.

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
- Kimi 5-hour billing blocks and tool-call statistics are not supported.

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

# Experimental Cursor source (reads local SQLite tokenCount fields)
ccstats daily --source cursor

# Cursor alias
ccstats daily --source cur

# Grok source and alias
ccstats daily --source grok
ccstats daily --source gx

# Kimi Code source and alias
ccstats daily --source kimi
ccstats daily --source km

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
| `source` | string | Source name or alias such as `claude`, `codex`, `cursor`, `grok`, `kimi`, or `all` |

Source root env overrides are independent of config keys:

| Source | Env var | Value | Default when unset |
|--------|---------|-------|--------------------|
| Claude Code | `CLAUDE_CONFIG_DIR` | Claude config root containing `projects/` | `~/.claude` |
| OpenAI Codex | `CODEX_HOME` | Codex root containing `sessions/` | `~/.codex` |
| Cursor | `CURSOR_HOME` | Cursor `User` directory | Cursor `User` under platform app/config data dirs |
| Grok | `GROK_HOME` | Grok root containing `sessions/` | `~/.grok` |
| Kimi Code | `KIMI_CODE_HOME` | Kimi Code root containing `sessions/` | `~/.kimi-code` |

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
Claude and Codex expose the required cache-read metric. Cursor, Grok, and
mixed `--source all` output report the value as unavailable (`N/A`, `null`, or
an empty CSV field) instead of treating missing metrics as zero.

### Parsing Warnings

When malformed JSONL records are encountered, ccstats reports them in stderr:

```text
Warning: ignored <N> malformed records
```

## Supported Data Sources

| Source | Directory | Override | Features |
|--------|-----------|----------|----------|
| Claude Code | `~/.claude/projects/` | `CLAUDE_CONFIG_DIR` | Projects, Billing Blocks, Deduplication |
| OpenAI Codex | `~/.codex/sessions/` | `CODEX_HOME` | Reasoning Tokens |
| All Sources | Multiple | Source-specific env vars | Combined daily/weekly/monthly/today/statusline summaries |
| Cursor (experimental) | Cursor `User/globalStorage/state.vscdb` | `CURSOR_HOME` | Local SQLite `tokenCount` fields only |
| Grok | `~/.grok/sessions/` | `GROK_HOME` | Context-token session summaries, Projects |
| Kimi Code | `~/.kimi-code/sessions/` | `KIMI_CODE_HOME` | Per-turn usage records, Projects, Cache tokens |

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

## License

MIT. See [LICENSE](LICENSE).
