# ccstats

[![Crates.io](https://img.shields.io/crates/v/ccstats.svg)](https://crates.io/crates/ccstats)
[![GitHub Release](https://img.shields.io/github/v/release/majiayu000/ccstats)](https://github.com/majiayu000/ccstats/releases)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://github.com/majiayu000/ccstats/blob/main/LICENSE)

`ccstats` is a fast CLI for token and cost usage analytics for Claude Code and OpenAI Codex logs.

Search keywords: `claude code usage stats`, `codex usage stats`, `token usage cli`, `ai token cost tracker`.

## Highlights

- Fast local analysis of usage JSONL logs
- Claude Code support (`~/.claude/projects/`)
- OpenAI Codex support (`~/.codex/sessions/`)
- Daily/weekly/monthly/project/session views
- Optional model-level token and cost breakdown

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
```

### Manual download
Download from [GitHub Releases](https://github.com/majiayu000/ccstats/releases).

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

## Crate Documentation

- docs.rs: <https://docs.rs/ccstats/latest/ccstats/>
- crates.io: <https://crates.io/crates/ccstats>
- The crate-level Rustdoc in `src/main.rs` explains supported sources and common commands.

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

# With model breakdown
ccstats today -b

# JSON output
ccstats today -j

# Debug mode (timing info)
ccstats today --debug
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

### Common Options

```bash
# Bucket by timezone
ccstats daily --timezone UTC

# Locale-aware number formatting
ccstats monthly --locale de

# Filter by date
ccstats daily --since 20260101 --until 20260131

# Select data source explicitly (supports aliases)
ccstats daily --source codex

# Offline mode (use cached pricing)
ccstats today -O

# Compact output
ccstats today -c

# Hide cost column
ccstats today --no-cost
```

### Session CSV Columns

`ccstats session --csv` now includes:

- `reasoning_tokens`
- `cache_creation_tokens`
- `cache_read_tokens`

### Parsing Warnings

When malformed JSONL records are encountered, ccstats reports them in stderr:

```text
Warning: ignored <N> malformed records
```

## Supported Data Sources

| Source | Directory | Features |
|--------|-----------|----------|
| Claude Code | `~/.claude/projects/` | Projects, Billing Blocks, Deduplication |
| OpenAI Codex | `~/.codex/sessions/` | Reasoning Tokens |

## Architecture

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for:
- Adding new data sources
- Data flow and processing pipeline
- Caching mechanism
- Deduplication algorithm

## License

MIT. See [LICENSE](LICENSE).
