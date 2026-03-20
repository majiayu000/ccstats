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
3. Ensure `cargo check` and `cargo test` pass
4. Submit a PR with a clear description

## Reporting Issues

Use [GitHub Issues](https://github.com/majiayu000/ccstats/issues) with:
- Steps to reproduce
- Expected vs actual behavior
- OS and Rust version
