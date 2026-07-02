# Tech Spec

## Linked Issue

GH-40

## Product Spec

`specs/GH40/product.md`

## Codebase Context

| Area | Files | Current behavior | Why relevant |
| --- | --- | --- | --- |
| Parser output | `src/source/mod.rs` | `ParseOutput` has `errors`; `LoadResult` has `skipped`/`valid`. | Data exists but is not fully surfaced. |
| Loader | `src/source/loader.rs` | Non-incremental path prints malformed warnings; incremental path hardcodes skipped 0. | Primary behavior gap. |
| App summary | `src/app.rs`, `src/output/table.rs` | Summary footer uses `LoadResult.skipped`/`valid`. | Table footer already has partial data. |
| Structured output | `src/output/json.rs`, `src/output/csv.rs`, `src/output/statusline.rs` | Outputs mostly data rows, not data-quality metadata. | Need metadata injection. |

## 设计方案

1. Extend `LoadResult` or add a nested metadata struct to carry `valid`, `dedup_skipped`, and `parse_errors` explicitly.
2. Accumulate `parse_errors` from every source and all-source path.
3. Fix incremental loading to return real skipped/dedup information if it dedups, or explicitly mark skipped count unavailable if the path cannot calculate it. Prefer computing true skipped count by reusing the same dedup finalization result.
4. Add JSON metadata field at the least disruptive top-level location used by daily/monthly/session outputs.
5. Add CSV metadata as comment/header metadata if current parser expectations allow it; otherwise document and test a dedicated metadata row.
6. Preserve table stderr warning while ensuring structured metadata is authoritative.

## Product-to-Test Mapping

| Product invariant | Implementation area | Verification |
| --- | --- | --- |
| P1 | JSON output | malformed fixture assertion. |
| P2 | CSV output | metadata assertion without changing data columns. |
| P3 | statusline JSON | statusline JSON test. |
| P4 | quiet mode | quiet structured output test. |
| P5 | loader incremental path | loader unit test for skipped accounting. |

## 数据流

Parser returns entries + parse errors -> loader filters/dedups and builds `LoadResult` metadata -> app all-source aggregation sums metadata -> output layer renders metadata alongside rows.

## 备选方案

- Keep stderr-only warnings: rejected because statusline/JSON consumers remain blind.
- Add a single `skipped` field for all causes: rejected because malformed parse errors and dedup skips mean different things.

## 风险

- Security: no new external input execution.
- Compatibility: JSON/CSV add metadata; strict consumers may need to ignore new fields/comments.
- Performance: negligible.
- Maintenance: overlaps with GH33/GH38 output metadata work; implementation should share helpers if those land first.

## 测试计划

- [ ] Loader/unit tests for parse error and dedup skipped accounting.
- [ ] CLI integration tests for JSON, CSV, statusline JSON, quiet mode.
- [ ] All-source aggregation metadata test.
- [ ] `cargo check`
- [ ] `cargo test`

## 回滚方案

Revert metadata plumbing and output fields. No data migration.
