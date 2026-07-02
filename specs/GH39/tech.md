# Tech Spec

## Linked Issue

GH-39

## Product Spec

`specs/GH39/product.md`

## Codebase Context

| Area | Files | Current behavior | Why relevant |
| --- | --- | --- | --- |
| Claude parser | `src/source/claude/parser.rs` | `usage.input_tokens.unwrap_or(0)` and related fields preserve negative values. | Primary bug surface. |
| Cursor parser | `src/source/cursor/parser.rs` | Accepts a record if input or output is positive; negative counterpart can remain negative. | Primary bug surface. |
| Codex/Grok parsers | `src/source/codex/parser.rs`, `src/source/grok/parser.rs` | Existing logic clamps deltas/signals with `.max(0)`. | Consistency reference. |
| Aggregation/cost | `src/core`, `src/pricing` | Assumes token counts are non-negative. | Should not need defensive fixes if parsers normalize. |

## 设计方案

1. Add a small parser-local helper or shared utility for token normalization only if it removes duplication; otherwise use explicit `.max(0)` at each parse boundary.
2. Apply normalization in Claude `parse_entry` before constructing `RawEntry`.
3. Apply normalization in Cursor bubble and generation parse paths before filtering/constructing `RawEntry`.
4. Ensure Cursor's "has any usage" filter runs after normalization so all-negative records do not survive as meaningful usage.
5. Add targeted unit tests in parser modules and one integration regression if needed.

## Product-to-Test Mapping

| Product invariant | Implementation area | Verification |
| --- | --- | --- |
| P1, P2 | `claude/parser.rs` | Unit tests for negative usage fields. |
| P1, P3 | `cursor/parser.rs` | Unit tests for both table parse paths. |
| P4 | parser modules | Existing positive/missing field tests unchanged. |
| P5 | CLI aggregation | Integration regression or existing tests with new fixture. |

## 数据流

Raw source record -> parser field extraction -> non-negative normalization -> `RawEntry` -> existing dedup/aggregation/cost. No persistence changes.

## 备选方案

- Clamp in aggregation layer: rejected because invalid parser output would still leak into session/model-level views before aggregation.
- Reject entire record on any negative field: stricter but can discard valid positive counterpart fields; current project precedent is clamp-to-zero.

## 风险

- Security: no new external input execution.
- Compatibility: malformed logs with negative values now report non-negative totals; normal logs unaffected.
- Performance: negligible.
- Maintenance: keep normalization close to source schema parsing.

## 测试计划

- [ ] Claude parser unit tests for negative input/output/cache fields.
- [ ] Cursor parser unit tests for bubble and generation records with mixed positive/negative values.
- [ ] Existing Codex/Grok tests continue to pass.
- [ ] `cargo check`
- [ ] `cargo test`

## 回滚方案

Revert parser normalization and tests. No data migration.
