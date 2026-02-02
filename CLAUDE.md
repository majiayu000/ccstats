# CLAUDE.md

## Project Guidelines

- **No backward compatibility**: This is a new project. Delete unused code completely. No deprecated functions, no `#[allow(dead_code)]`, no compatibility shims.
- **Keep it simple**: Avoid over-engineering. Only add what's needed now.
- **Latest dependencies**: Always use the latest stable versions.
- **Rust 2024 edition**: Use modern Rust idioms.

## Architecture

Fast CLI tool for analyzing Claude Code token usage from `~/.claude/` JSONL logs.

### Key modules
- `src/cli/` - Command line interface (clap)
- `src/data/` - JSONL parsing and deduplication logic
- `src/pricing/` - Cost calculation from LiteLLM pricing
- `src/output/` - Table and JSON output formatting
- `src/utils/` - Date parsing, jq filtering

### Deduplication logic
Streaming responses create multiple entries per message ID. We keep the entry with `stop_reason` set (completed message) to get accurate token counts.
