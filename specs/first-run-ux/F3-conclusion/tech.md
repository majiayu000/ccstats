# Tech Spec — F3 Human conclusion line

## Linked Issue

https://github.com/majiayu000/ccstats/issues/166

## Product Spec

`specs/first-run-ux/F3-conclusion/product.md`

## Codebase Context

| Area | Files | Current behavior | Why relevant |
| --- | --- | --- | --- |
| Period tables | `src/output/table.rs`, `src/app/period_table.rs` | Renders period table then summary line (record counts) | Insert conclusion before table. |
| Cost display | `CostDisplayMode`, `api_equivalent_cost_coverage`, `model_cost_kind` | Floor vs exact already exist for desktop/SDK | Reuse; do not invent a third cost theory. |
| Number format | `NumberFormat`, currency converter | Table already formats cost | Conclusion must call the same helpers. |
| Tests | `src/output/table_tests.rs`, CLI integration | Snapshot-ish stdout | Pin the line; pin json unchanged. |

## 设计方案

1. Add `print_period_conclusion(...)` in `output/table.rs` (or `output/conclusion.rs` if table.rs is already large) taking aggregated `Stats`/day map, cost mode, coverage, compact flag.
2. Call it from the single period-table path used by daily/today/weekly/monthly (including all-sources). Do not scatter prints in every source handler.
3. Cost sentence construction:
   - Sum display cost with existing `sum_display_model_costs` / coverage flags already computed for the table footer.
   - If `cost_is_lower_bound` or mixed unknown models: prefix `≥ ` using the same rule as desktop `displayedCost`.
4. For non-compact table-mode `today`, load the previous seven local calendar days through the existing aggregation path as a separate, non-overlapping comparison query. Keep the displayed table, totals, and quality metadata restricted to today; pass prior days only to the comparison. JSON/CSV keep their original query range. Compare against at least three prior days with complete data and known costs (or tokens with `--no-cost`); otherwise omit the comparison. Do not substitute older active dates for missing days in this seven-day window.
5. Keep the existing trailing summary line (record counts / elapsed). Conclusion is extra, not a replacement.

## Product-to-Test Mapping

| Invariant | Test |
| --- | --- |
| Table-only | CLI today json has no conclusion prose in stdout |
| Floor | fixture with unpriced model → stdout contains `≥` or `floor` and not `$0.00` as the headline |
| Compact | `--compact today` still matches `^` conclusion then table |
| Numbers match | parse cost from conclusion and footer; equal within formatting |

## 备选方案

- Put conclusion after the table: rejected；用户先看到表就会忽略结论。
- Add JSON `headline` field now: rejected as schema change；需要的话单独 issue。

## 风险

- 双计费用：必须复用表的合计函数。
- 7 日均值在时区边界出错：只用已经按 CLI timezone bucket 过的 `date_str` 键。
- 颜色：结论可用与 summary line 相同的 color 开关，不要新调色盘。

## 测试计划

- [ ] Unit tests for conclusion string builder (exact / floor / no-cost / insufficient pace data).
- [ ] CLI today table contains conclusion; json does not.
- [ ] `cargo test`

## 回滚方案

Remove the print call and builder. Tables unchanged.
