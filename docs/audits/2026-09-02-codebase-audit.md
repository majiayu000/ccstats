# Codebase Audit Report

> Audit date: 2026-09-02
> Target: `origin/main` at `6bec20d` (`majiayu000/ccstats`)
> Tech stack: Rust 2024 CLI + library SDK
> Method: Static evidence pass against current `main`. An earlier pass on `codex/provider-algorithm-audit` is **not** the implementation baseline; that branch is behind `main` and its dirty worktree is snapshotted at `refs/backup/pre-audit-remediation-20260902`.

## How to read this report

- **Fact** is observed in current `main`.
- **Inference** is labeled.
- **Parked** means do not implement until a later design decision is confirmed.
- Companion design: [`docs/architecture/2026-09-02-provenance-and-source-decoupling.md`](../architecture/2026-09-02-provenance-and-source-decoupling.md).

## Summary

| Severity | Count on `main` | Status vs older branch |
|---|---:|---|
| Critical | 1 | Still open: `--source all` mixes token provenance |
| High | 4 | Gemini pricing, 1h cache opacity, clock-aligned blocks, registry duplication |
| Medium | 4 | Docs/SDK contract drift, Gemini JSON last-write, Cline dual logs, weekly full-history scan |
| Already fixed on `main` | 2 | Grok env fallback; Grok-only cost reports |

`main` now registers **29** sources. Token parsers for Claude / Codex / Qwen ledger remain the strongest path. The product risk is no longer “missing sources”; it is **one uniform `RawEntry` plus a hand-maintained registry** that cannot describe provenance, pricing families, or CLI/SDK contracts in one place.

## Already fixed on `main` (do not re-implement)

| Older finding | Evidence on `main` | Status |
|---|---|---|
| Grok `GROK_HOME` fell back to `~/.grok` when the override was not a directory | `src/source/grok/parser.rs:75-83` returns `None` when the override exists but is not a directory | Fixed |
| Grok-only views treated context snapshots as billable cost without a dedicated report | Grok daily path uses `load_grok_daily_with_cost` and `GrokCostReport` (`src/app.rs:531-555`, `src/source/grok/cost_report.rs`) | Fixed for `--source grok` only |
| OpenCode / Copilot / Pi missing as product gaps | Registry includes OpenCode, Copilot, Pi and forks (`src/source/registry.rs:35-66`) | Coverage added; quality still source-specific |

## Critical

### C1: `--source all` still mixes estimated-proxy tokens into token totals while pricing real-only

- Evidence: `src/app.rs:623-661` merges every source with `merge_day_stats`, then renders with `CostDisplayMode::RealOnly`. Grok snapshot rows still use `CostKind::EstimatedProxy` (`src/source/grok/parser.rs`).
- Fact: All-source tables/JSON can show Grok (and any other estimated-proxy) tokens in `input_tokens` / `total_tokens` while omitting those rows from cost.
- Impact: Users read one number as “usage” and another as “cost” and they are not the same accounting.
- Confidence: High.
- Suggested fix: Follow the provenance design. Split all-source token totals into `real` vs `estimated_proxy`, or exclude estimated-proxy tokens from the default all-source token columns. Do not invent a third mixed total without a label.

## High

### H1: LiteLLM ingest still drops Gemini / Google models

- Evidence: `src/pricing/resolver/parse.rs:11-25` only keeps Claude, OpenAI, xAI, and selected CN names. The unit test `test_parse_filters_non_claude_non_openai` asserts `google/gemini` is discarded.
- Fact: Gemini is a first-class source (`UsageSource::Gemini`) whose list prices never enter `PricingDb`. `fallback_pricing` has no Gemini family.
- Impact: `ccstats daily --source gemini` reports tokens and N/A (or fallback-none) cost. Cursor/Cline/OpenCode rows that name Gemini models have the same hole.
- Confidence: High.
- Suggested fix: Vendor allowlist becomes an explicit family set that includes Google/Gemini. Add fallback rates for current Gemini Flash/Pro ids. Keep the “metadata-only must not load as $0” rule.

### H2: `blocks` is clock-aligned, not Anthropic’s rolling 5-hour billing window

- Evidence: `src/core/aggregator.rs:237-249` uses `hour() / 5 * 5`. CLI copy still says “5-hour billing blocks”.
- Fact: Windows start at local 00:00 / 05:00 / 10:00 / 15:00 / 20:00.
- Impact: Numbers will not match Claude Code `/cost` or ccusage rolling windows.
- Confidence: High that the algorithm is not Anthropic billing; **parked** as a product decision (rename vs reimplement).
- Suggested fix: Either rename the command to clock windows, or implement gap-based rolling 5h windows with fixtures. Do not ship a silent algorithm swap.

### H3: 1-hour cache creation is billed but never shown

- Evidence: Pricing uses `cache_creation_1h` (`src/pricing/cost.rs:20-27`). Claude parser writes it (`src/source/claude/parser.rs:311-332`). JSON/CSV/SDK only expose `cache_creation_tokens` (`src/output/json.rs:68`, `src/sdk.rs` `TokenBreakdown`).
- Fact: Cost can exceed `cache_creation × 5-minute cache-create price` with no column explaining why.
- Impact: Claude 1h-cache users think pricing is wrong.
- Confidence: High.
- Suggested fix: Add `cache_creation_1h_tokens` to JSON, CSV, SDK `TokenBreakdown`, and optional table breakdown. Keep `cache_creation` as the inclusive total.

### H4: Source registry and SDK enum are parallel sources of truth

- Evidence: Adding a source requires `src/source/registry.rs` `SOURCES`, `UsageSource` plus `as_str` / `from_name` / `VARIANTS` in `src/sdk.rs` (869 lines), optional CLI routing, README, and doctor hints. `src/source/mod.rs` inlines an entire Reasonix parser module.
- Fact: 29 constructors are listed by hand. Tests can catch name drift after the fact; they cannot generate the wiring.
- Impact: CLI and SDK silently diverge; every new source is shotgun surgery across god files (`sdk.rs` 869, `types.rs` 882, `aggregator.rs` 799, `pricing/db.rs` 798, `loader.rs` 786, `app.rs` 754).
- Confidence: High.
- Suggested fix: One inventory macro/module that expands registry + SDK enum + name tables. Do not start that refactor in the same PR as a parser or pricing bug.

## Medium

### M1: Gemini chat JSON does not last-write-win on message id

- Evidence: JSONL `type=gemini` uses `direct_indices` (`src/source/gemini.rs`). `parse_session_json` does not. `needs_dedup` is false.
- Impact: Rewritten chat JSON messages can double-count.
- Confidence: Medium (depends on whether Gemini CLI rewrites in place).

### M2: Cline source reads CLI and VS Code extension logs together

- Evidence: `src/source/cline.rs` concatenates CLI `*.messages.json` and extension task files. CLI subtracts cache from `inputTokens`; extension `tokensIn` is stored as-is (`src/source/cline_extension.rs`).
- Impact: Dual-install users can double-count. Field inclusion may still differ across formats.
- Confidence: Medium.

### M3: CLI `weekly` / `monthly` load all history; SDK `ThisWeek` / `ThisMonth` filter to the current period

- Evidence: `SourceCommand::needs_today_filter` is only today/statusline (`src/cli/commands.rs`). SDK `UsageRange::ThisWeek` resolves Monday–today (`src/sdk.rs`).
- Impact: Same English word, different dates. Large histories make `weekly` a full scan.
- Confidence: High. Rename, do not silently change CLI defaults.

### M4: Architecture docs list a subset of sources and an outdated `Capabilities` shape

- Evidence: `docs/ARCHITECTURE.md` still describes a small source set and a 4-bool capability struct. Runtime has 8 capability flags, doctor, quota, Grok cost reports, and 29 sources.
- Impact: New parsers get implemented against the wrong contract.
- Confidence: High. Update as part of the design PR, not as a silent rewrite of runtime behavior.

## Out of scope / parked

| Item | Why parked |
|---|---|
| Rolling Anthropic billing blocks (H2 implementation) | Needs an explicit product choice vs ccusage |
| Full 200-line file split of `sdk.rs` / `app.rs` | Unrelated churn; decouple via inventory first |
| Cursor cross-database dedup | Cursor on `main` now has a usage-API client; re-audit after that path is stable |
| Codex `--codex-scope` session_meta ordering | Lives on the uncommitted `codex/provider-algorithm-audit` worktree, not `main` |
| Further inferred sources | Coverage is already wide; correctness of existing sources first |

## Repair order

See the companion design for contracts. Implementation slices:

1. This document pair (audit + architecture).
2. Gemini/Google pricing allowlist (H1).
3. Expose `cache_creation_1h` (H3).
4. All-source provenance split (C1).
5. Later, inventory macro (H4) and parked items only after review of 2–4.
