# Tech Spec — F5 `ccstats limits`

## Linked Issue

https://github.com/majiayu000/ccstats/issues/168

## Product Spec

`specs/first-run-ux/F5-limits/product.md`

## Codebase Context

| Area | Files | Current behavior | Why relevant |
| --- | --- | --- | --- |
| Quota command | `src/quota_cmd.rs`, `src/output/quota.rs`, `src/cli/commands.rs` Quota | Top-level, hard-hints Codex | Reuse handlers; do not copy pricing math. |
| Blocks | `src/output/blocks.rs`, aggregator billing windows | Activity-driven 5h windows | Extract "active window" selection. |
| Parse/dispatch | `parse_command`, `SourceCommand::Quota` | quota implies source_hint codex | `Limits` must **not** set source_hint to claude. |
| Capabilities | `has_billing_blocks` | Claude true, others false | Gate Claude section. |

## 设计方案

1. Add `Commands::Limits` and `SourceCommand::Limits`. `parse_command` does not set a source hint.
2. `limits` is metadata-ish but Codex quota and Claude blocks need their loaders. Skip unused sources.
3. New `src/limits_cmd.rs`:
   - Determine requested sections from `--source` (none/all → both attempts; claude/codex → one).
   - Codex: call the same `load_weekly_quota` as quota_cmd; on error capture string.
   - Claude: load Claude entries with existing pipeline, aggregate blocks, pick the window that contains `now` or the latest active window used by today's `blocks` output.
4. Rendering functions in `output/limits.rs` wrapping existing quota table + a compact single-block summary (tokens, time remaining if already computed, estimated disclaimer).
5. Keep `handle_quota` untouched besides help text on the Quota variant.
6. Pricing: Codex value estimate follows quota_cmd (`show_cost`). Claude block cost uses existing block pricing if table already shows it; do not add a new estimator.

## Product-to-Test Mapping

| Invariant | Test |
| --- | --- |
| quota unchanged | existing `tests/cli_codex_quota.rs` |
| limits codex-only | temp HOME with quota snapshot + no claude |
| limits claude-only | blocks fixture, json `codex: null` |
| bad source | `--source cursor` exit 1 |
| disclaimer | table contains `not an official` |

## 备选方案

- Make `quota` generic and overload it: rejected；破坏现有 Codex 脚本。
- Fetch Anthropic rate-limit headers: rejected this round（凭证与网络边界不同）。

## 风险

- Blocks 全历史加载可能比 quota 慢：只 load Claude source，日期过滤 today±1 天若现有 API 允许；否则沿用 blocks 命令同一加载。
- F1 auto-detect 若先合并：`limits` 仍按本 spec 的 --source 规则，不要把 limits 默认成 all-usage 表。

## 测试计划

- [ ] Extend quota tests; add `tests/cli_limits.rs`.
- [ ] `cargo test`

## 回滚方案

Remove Limits command and output module. quota/blocks remain.
