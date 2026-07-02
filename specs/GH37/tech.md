# Tech Spec

## Linked Issue

GH-37

## Product Spec

`specs/GH37/product.md`

## Codebase Context

| Area | Files | Current behavior | Why relevant |
| --- | --- | --- | --- |
| Loader helper | `src/source/loader.rs` | Private `DataLoader::merge_day_stats` plus tests. | Existing tested implementation to preserve. |
| All-source aggregation | `src/app.rs` | Local free function duplicates loader helper and has no direct tests. | Duplicate risk. |
| Shared core | `src/core/aggregator.rs` | Existing aggregation module for shared logic. | Natural owner for shared helper. |
| Tests | `src/source/loader.rs`, `tests/cli_integration.rs` | Helper tests currently live near loader. | Need migrate or call shared helper. |

## 设计方案

1. Move the helper implementation to `src/core/aggregator.rs` as `pub(crate) fn merge_day_stats(...)`.
2. Update loader and app all-source aggregation to call the shared helper.
3. Move existing helper unit tests to the helper owner module, or keep tests in loader only if they call the shared function directly.
4. Remove both duplicate definitions; do not leave compatibility wrappers.

## Product-to-Test Mapping

| Product invariant | Implementation area | Verification |
| --- | --- | --- |
| P1-P3 | `core/aggregator.rs` | Unit tests ported from loader. |
| P4 | `src/source/loader.rs`, `src/app.rs` | Compile check plus focused test/all-source regression. |
| P5 | duplicate removal | `rg "fn merge_day_stats" src` shows one definition. |

## 数据流

No runtime data flow changes. Existing callers pass `HashMap<String, DayStats>` into a shared helper and receive the same merged map effects.

## 备选方案

- Keep loader helper and expose it through `DataLoader`: rejected because app should not depend on source loader internals.
- Copy tests only and leave duplicate code: rejected because it preserves the maintenance hazard.

## 风险

- Security: no external input handling changes.
- Compatibility: no user-visible behavior change.
- Performance: unchanged.
- Maintenance: lower risk after single owner.

## 测试计划

- [ ] Unit tests for disjoint dates, overlapping dates, model preservation, empty source.
- [ ] `rg "fn merge_day_stats" src` confirms one definition.
- [ ] `cargo check`
- [ ] `cargo test`

## 回滚方案

Revert the helper move and caller updates. No data migration.
