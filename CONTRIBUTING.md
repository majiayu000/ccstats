# Contributing to ccstats

Thanks for your interest in contributing!

## Development Setup

```bash
git clone https://github.com/majiayu000/ccstats.git
cd ccstats
cargo build
cargo test
```

## Guidelines

- Follow existing code style
- Add tests for new features
- Keep commits atomic — one change per commit
- Commit messages: `<type>: <description>` (feat/fix/refactor/docs/test/chore)

## Pull Requests

1. Fork the repo and create your branch from `main`
2. Make your changes
3. Ensure `cargo check` and `cargo test --locked` pass
4. Submit a PR with a clear description

## New sources

A new `--source` is accepted only with:

1. Evidence of an official or upstream log/schema (link the repo path or docs).
2. A minimal fixture under `tests/fixtures/<source>/` that the parser can read.
3. Tests covering token buckets and, when the source records a cost, `recorded_cost_usd`.

Unknown costs must stay `null` / `N/A` / `unknown`, never a silent `0`. Estimates must be labeled.

## Reporting Issues

Use [GitHub Issues](https://github.com/majiayu000/ccstats/issues) with:
- Steps to reproduce
- Expected vs actual behavior
- OS and Rust version
