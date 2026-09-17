# CLAUDE.md

## Project Guidelines

- **No backward compatibility**: This is a new project. Delete unused code completely. No deprecated functions, no `#[allow(dead_code)]`, no compatibility shims.
- **Keep it simple**: Avoid over-engineering. Only add what's needed now.
- **Latest dependencies**: Always use the latest stable versions.
- **Rust 2024 edition**: Use modern Rust idioms.
- **Unknown cost is never 0**: Do not emit unknown costs as `0`. Estimates must carry a visible label (`est.`, `estimated`, `EstimatedProxy`, or equivalent). Official provider numbers stay labeled `official`.

## Architecture

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md). The registered source list lives in `src/source/inventory.rs`. Product direction is in [docs/ROADMAP.md](docs/ROADMAP.md).
