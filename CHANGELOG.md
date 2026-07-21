# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Added
- Report each resolved model's pricing lookup key and source in `--debug` output without changing structured stdout.

## [0.4.0] - 2026-07-17

### Added
- Add Kimi Code usage and cost statistics from `~/.kimi-code/sessions` wire logs, available through `ccstats kimi`, `--source kimi`, and alias `km`.
- Add Kimi Code daily, weekly, monthly, today, session, project, and statusline views using the existing output formats, including sub-agent usage and cache token fields.
- Add Moonshot/Kimi fallback pricing for `kimi-code/*` subscription models based on official `kimi-k2.6` API reference rates.
- Add Rust SDK source selection for Kimi Code usage summaries.
- Report prompt cache hit rate as `cache_read / (input + cache_creation + cache_read)` across table, JSON, CSV, statusline, top, session, project, and block outputs, and in Rust SDK summaries.

### Fixed
- Report cache hit rate as unavailable instead of zero for sources without a cache-read metric (Cursor, Grok, and mixed `--source all` output).

## [0.3.0] - 2026-07-16

### Added
- Add the `endpoints` subcommand for native-versus-proxy traffic and cost breakdowns.
- Add multi-range cost summaries to the Rust SDK with `summarize_cost_ranges`, `MultiSummaryOptions`, and `MultiCostSummary`.
- Add fallback pricing for GLM, DeepSeek, Qwen, and Moonshot model families.
- Expose parse data-quality metadata and pricing-source provenance in structured CLI and SDK output.

### Changed
- Separate observed proxy costs from estimated token-based proxy costs and report their provenance explicitly.
- Use the platform-appropriate pricing cache directory and honor `CLAUDE_CONFIG_DIR` when discovering Claude data.
- Make pricing model matching more conservative and surface invalid configuration instead of silently falling back.
- Align source capability discovery and structured output dispatch across the CLI and Rust SDK.

### Fixed
- Count Claude subagent (sidechain) usage and price one-hour cache creation tokens correctly.
- Honor the requested currency consistently across output modes.
- Deduplicate replayed Codex usage and repeated Claude messages across files without dropping distinct usage.
- Clamp invalid negative Claude and Cursor token counts and keep merged daily totals consistent.
- Make pricing cache writes atomic and report corrupted cache data instead of silently degrading.
- Harden Cursor database parsing and preserve parse failures in data-quality metadata.
- Exclude records outside requested SDK summary ranges.

## [0.2.65] - 2026-05-31

### Fixed
- Deduplicate repeated Claude tool-use records in `ccstats tools` so streaming progress entries with the same tool identity count once.

## [0.2.64] - 2026-05-26

### Added
- Add Grok local context-token statistics from `~/.grok/sessions`, available through `ccstats grok`, `--source grok`, and alias `gx`.
- Add Grok daily, weekly, monthly, today, session, project, and statusline views using the existing output formats.
- Add xAI/Grok pricing resolution for `grok-build` and `grok-*` models.
- Add Rust SDK source selection for Grok usage summaries.

### Changed
- Document Grok support as local context-token trend reporting, not exact provider input/output billing or Grok account quota usage.
- Track exact Grok usage support separately until Grok exposes stable `usage`, `cost_in_usd_ticks`, `input_tokens`, `output_tokens`, `cached_tokens`, and `reasoning_tokens` fields.

## [0.2.63] - 2026-05-09

### Added
- Expose a reusable Rust SDK entry point with `summarize_cost`, `SummaryOptions`, `UsageSource`, and `UsageRange`.
- Return structured cost summaries with token totals, model breakdowns, source metadata, date bounds, and pricing/currency fields.

### Changed
- Move CLI startup behind `ccstats::run_cli()` so the binary is a thin caller of the library crate.

## [0.2.62] - 2026-04-27

### Added
- `--source all` overview support for registered sources.
- Monthly budget forecasting for monthly reports.
- Experimental Cursor usage source support.

### Fixed
- Harden release metadata checks and align release preflight with the current dependency MSRV.
- Scope session and message deduplication by source file to avoid cross-file collisions.
- Add GPT-5.4 pricing fallback and fall back to USD when currency conversion cannot be loaded.

## [0.2.61] - 2026-04-02

### Added
- `sources` command for discovering available data sources and aliases.

### Changed
- Improve unsupported-command guidance with actionable source hints.
- Refresh README landing visuals and crate documentation.
- Reduce parser, loader, aggregation, JSON serialization, and no-cost execution overhead.

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
