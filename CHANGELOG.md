# Changelog

All notable changes to this project will be documented in this file.

## [0.2.15] - 2026-02-12

### Changed
- Refactor `output/table.rs`: extract shared rendering logic into `print_period_table` with helpers, reducing 1097 lines to 404 (63% reduction)

## [0.2.14] - 2026-02-12

### Added
- Unit tests for `core/dedup.rs`: empty input, single entry, all-completed duplicates, multiple distinct IDs, no-id-without-stop, mixed entries
- Unit tests for `pricing/resolver.rs`: LiteLLM parsing, model name resolution (exact/prefix/partial/no-match), fallback pricing for all model families

## [0.2.13] - 2026-02-12

### Fixed
- Replace `panic!()` in main.rs with graceful `eprintln` + `process::exit(1)`

## [0.1.20] - 2026-02-03

### Added
- Multi-platform release automation (GitHub Actions)
  - macOS (Intel + Apple Silicon)
  - Linux (x86_64 + ARM64)
  - Windows (x86_64)
- Install script for quick installation
- cargo-binstall support for prebuilt binaries
- README with installation instructions

### Fixed
- Correct rust-toolchain action name in CI

## [0.1.18] - 2026-02-03

### Changed
- Upgrade to Rust 2024 edition
- Update all dependencies to latest versions
- Replace `atty` with `std::io::IsTerminal`
- Remove unused dependencies (chrono-tz, num-format)

## [0.1.17] - 2026-02-03

### Fixed
- Improve deduplication by preferring entries with `stop_reason`
- Streaming responses now correctly use completed message token counts

## [0.1.16] - 2026-02-02

### Added
- Timezone support (`--timezone`)
- Locale support (`--locale`)

### Fixed
- Normalize model names by removing date suffix dynamically

## [0.1.15] - 2026-02-01

### Added
- Cost control options (`--cost`, `--no-cost`)
- Config file support (`~/.config/ccstats/config.toml`)
- jq filter support (`--jq`)

## [0.1.14] - 2026-01-31

### Added
- `blocks` command for 5-hour billing window reports
- `project` command for per-project usage reports
- `session` command for per-session usage reports

## [0.1.13] - 2026-01-30

### Added
- `statusline` command for tmux/statusbar integration
- Compact mode (`-c`/`--compact`)
- Debug mode (`--debug`)
- Color output control (`--color`/`--no-color`)
- Sort order option (`--order asc/desc`)

## [0.1.12] - 2026-01-29

### Added
- Weekly and monthly report commands
- JSON output with breakdown support
- Per-model breakdown (`-b`/`--breakdown`)

## [0.1.0] - 2026-01-28

### Added
- Initial release
- Daily token usage statistics
- Cost calculation from LiteLLM pricing
- Date filtering (`--since`, `--until`)
