# ccstats

[![CI](https://github.com/majiayu000/ccstats/actions/workflows/ci.yml/badge.svg)](https://github.com/majiayu000/ccstats/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/ccstats.svg)](https://crates.io/crates/ccstats)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://github.com/majiayu000/ccstats/blob/main/LICENSE)

![ccstats token and cost analytics card](docs/branding/readme-card.png)

Local-first token and cost analytics for 29 AI coding-agent data sources. One binary turns
usage metadata you already have into terminal reports and JSON/CSV. Unknown
cost is never printed as `$0`. Estimates are labeled. No ccstats account.

## 30-second start

```bash
brew install majiayu000/tap/ccstats
ccstats
```

With no arguments, `ccstats` uses sources detected on this machine. If none
are ready, it shows `doctor` instead of an empty report.

## Core sources

| Source | Usage input | Start here |
|--------|-------------|------------|
| Claude Code | `~/.claude/projects/` | `ccstats today --source claude` |
| OpenAI Codex | `~/.codex/sessions/` | `ccstats today --source codex` |
| Cursor | Usage API or `CURSOR_USAGE_FILE` | `ccstats today --source cursor` |
| Gemini CLI | `~/.gemini/tmp/` | `ccstats today --source gemini` |
| Grok | `~/.grok/` sessions + unified log | `ccstats today --source grok` |
| Kimi Code | `~/.kimi-code/sessions/` | `ccstats today --source kimi` |
| OpenCode | `opencode/opencode*.db` | `ccstats today --source opencode` |

All 29 sources, including Unsloth Studio and DeepSeek Harness. Env overrides
and per-source notes: [docs/sources.md](docs/sources.md). `ccstats sources`
lists aliases. Grok and Kimi cost semantics:
[docs/sources/grok.md](docs/sources/grok.md),
[docs/sources/kimi.md](docs/sources/kimi.md).

## Why ccstats

- **Accurate** — source-aware dedup, cache/reasoning buckets, and provenance
  (`Real` / `EstimatedProxy` / unknown). Unknown is `N/A` or `unknown`, not `0`.
- **Fast** — Rust CLI; usage-facts cache so `weekly` / `monthly` do not
  reparse unchanged files. `--no-cache` forces a rebuild; `--debug` shows
  cache hit/miss.
- **Live remaining** — `statusline` reads Claude Code hook JSON on stdin
  (Pro/Max `rate_limits` from 2.1.80+). `limits` and `watch` show official
  windows when present and mark estimates otherwise.
- **Local-first** — no telemetry. Network is pricing refresh, FX, and
  Cursor API. See [docs/PRIVACY.md](docs/PRIVACY.md).

## Install

```bash
brew install majiayu000/tap/ccstats
cargo binstall ccstats
cargo install ccstats
curl -fsSL https://raw.githubusercontent.com/majiayu000/ccstats/main/install.sh | sh
```

Desktop installers on [GitHub Releases](https://github.com/majiayu000/ccstats/releases)
are Developer ID signed and notarized on macOS. Missing Apple secrets fail
the release instead of shipping an ad-hoc DMG. Windows MSIs ship unsigned
(SmartScreen: More info → Run anyway). The supported real-time
surface is the CLI (`ccstats watch`). Packaging notes:
[docs/RELEASING.md](docs/RELEASING.md), [desktop/README.md](desktop/README.md).

## Usage

```bash
ccstats today
ccstats daily --since 20260901 --until 20260915
ccstats weekly --source all
ccstats monthly --source all --no-cache
ccstats statusline                 # tmux / Claude Code status line
ccstats limits                     # Codex + Claude (+ Cursor when available)
ccstats watch --once               # one frame; non-zero if a window is hot
ccstats verify                     # ccstats estimate vs source-recorded cost
ccstats serve                      # 127.0.0.1 JSON for the same types as --json
ccstats doctor
ccstats login cursor               # never pass secrets on argv
```

Claude Code status line (hook JSON on stdin, Claude Code 2.1.80+):

```json
{ "statusLine": { "type": "command", "command": "ccstats statusline --source claude" } }
```

`--cost-source auto|ccstats|cc|both` chooses local vs Claude Code session
cost. Official percentages have no suffix; estimates use `est.`.

`weekly` / `monthly` group already-filtered history. They do not default to
"this week". Bound dates with `--since` / `--until`.

Config file (first existing path wins): `~/.config/ccstats/config.toml`, the
platform config dir, or `~/.ccstats.toml`. CLI flags override the file.
Keys and source-root env vars: [docs/sources.md](docs/sources.md).

Common flags: `--json`, `--csv`, `--offline` / `-O`, `--strict-pricing`,
`--debug`, `--no-cache`, `--timezone`, `--currency`, `--source all`.
`--source all` includes per-source subtotals; `--no-source-breakdown`
keeps a single combined table/array.

## Rust SDK

[docs.rs/ccstats](https://docs.rs/ccstats/latest/ccstats/). Same parsers and
pricing as the CLI. Local HTTP (`ccstats serve`) and CLI `--json` share the
types documented in [docs/api.md](docs/api.md).

```rust
use ccstats::{SummaryOptions, UsageRange, UsageSource, summarize_cost_with_cli_config};

let summary = summarize_cost_with_cli_config(SummaryOptions {
    source: UsageSource::Codex,
    range: UsageRange::Today,
    ..SummaryOptions::default()
})?;
```

## Architecture

- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) — sources, pipeline, cache
- [docs/algorithm/authoritative-token-accounting.md](docs/algorithm/authoritative-token-accounting.md)
- [docs/ROADMAP.md](docs/ROADMAP.md) — product sequence
- [CONTRIBUTING.md](CONTRIBUTING.md) — new sources need a fixture + upstream link

## License

MIT. See [LICENSE](LICENSE).
