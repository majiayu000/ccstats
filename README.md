# ccstats

Fast token usage statistics CLI for Claude Code, Codex, and more.

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

# Offline mode (use cached pricing)
ccstats today -O

# Compact output
ccstats today -c

# Hide cost column
ccstats today --no-cost
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

MIT
