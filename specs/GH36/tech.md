# Tech Spec

## Linked Issue

GH-36

## Product Spec

`specs/GH36/product.md`

## Codebase Context

| Area | Files | Current behavior | Why relevant |
| --- | --- | --- | --- |
| Codex parser | `src/source/codex/parser.rs` | `previous_totals` duplicate check compares only `total.total_tokens == prev.total_tokens`. | Primary bug surface. |
| Usage model | `src/source/codex/parser.rs` | `TokenUsage` fields are optional; `UsageTotals::from_usage` converts missing fields to 0. | Needs complete-vector comparison after normalization. |
| CLI fixtures | `tests/cli_integration.rs` | Existing Codex tests cover replay, reasoning, cache, and source-wide dedup. | Add regression without weakening existing assertions. |
| Dedup core | `src/core/dedup.rs` | Handles source-wide message id dedup after parser emits entries. | Should remain unchanged. |

## 设计方案

1. Add an explicit helper on `UsageTotals` such as `is_duplicate_of(&self, prev: &Self) -> bool` that compares every cumulative field used for delta calculation.
2. Replace the current `total_tokens`-only duplicate guard with that helper.
3. Keep delta calculation unchanged: use `last_token_usage` when present, otherwise subtract previous cumulative totals.
4. Add focused parser/unit tests for missing `total_tokens` with growing component fields.
5. Add or update CLI integration fixture proving `ccstats codex daily --json` reports the full expected total.

## Product-to-Test Mapping

| Product invariant | Implementation area | Verification |
| --- | --- | --- |
| P1, P4 | `UsageTotals::is_duplicate_of` | Unit tests for equal and different vectors. |
| P2 | `parse_codex_file_with_debug` | Fixture with missing `total_tokens` and growing components. |
| P3 | last-token path | Regression with `last_token_usage` present. |
| P5 | `dedup.rs` unchanged | Existing replay/source-wide tests. |

## 数据流

Codex JSONL line -> `TokenUsage` -> normalized `UsageTotals` -> complete-vector duplicate check -> delta selection -> `RawEntry` -> existing dedup/aggregation.

## 备选方案

- Compare raw optional `TokenUsage` directly: rejected because aliases and missing fields should normalize consistently before comparison.
- Remove duplicate guard entirely: rejected because replayed identical cumulative events would double count.

## 风险

- Security: no new external input execution.
- Compatibility: only fixes undercounting; expected totals may increase for affected logs.
- Performance: comparing five integers per event is negligible.
- Maintenance: helper centralizes duplicate semantics and makes future usage fields visible in tests.

## 测试计划

- [ ] Unit tests: equal usage vector, differing component with same `total_tokens`, missing `total_tokens`.
- [ ] Integration tests: Codex JSONL missing `total_tokens` but growing components.
- [ ] Regression: existing Codex replay/dedup/reasoning/cache tests.
- [ ] `cargo check`
- [ ] `cargo test`

## 回滚方案

Revert the helper and duplicate guard change. No data migration.
