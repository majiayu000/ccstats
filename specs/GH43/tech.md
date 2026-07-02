# Tech Spec

## Linked Issue

GH-43

## Product Spec

`specs/GH43/product.md`

## Codebase Context

| Area | Files | Current behavior | Why relevant |
| --- | --- | --- | --- |
| Pricing cache | `src/pricing/cache.rs` | `cache_file()` uses `$HOME/.cache/ccstats/pricing.json`. | Primary change surface. |
| Pricing DB | `src/pricing/db.rs` | Calls cache load/save helpers. | Should remain unaware of path details. |
| Config paths | `src/config.rs` | Uses `dirs::config_dir()` plus fallbacks. | Style reference for platform path fallback. |
| Tests | pricing unit tests | Cache path is not currently platform-injected. | Need testable helper design. |

## 设计方案

1. Refactor cache path selection into helpers:
   - preferred platform path via `dirs::cache_dir()`
   - legacy path via `$HOME/.cache/ccstats/pricing.json`
   - fallback path when platform path unavailable
2. Load order:
   - preferred path first
   - legacy path as compatibility fallback if preferred missing
3. Save target:
   - always preferred/fallback current path, not legacy, after successful online fetch
4. Keep `PricingDb` API unchanged.
5. Add tests by injecting path roots or extracting pure path-selection helpers so tests do not depend on the host OS.

## Product-to-Test Mapping

| Product invariant | Implementation area | Verification |
| --- | --- | --- |
| P1/P2 | cache path helper | Unit tests with injected dirs. |
| P3 | load helper | Test legacy-only offline cache read. |
| P4 | save helper | Test save target path selection. |
| P5 | code structure | Code review and focused unit tests. |

## 数据流

Pricing load -> cache helper chooses read candidates -> parse raw cache -> pricing DB. Online fetch -> save helper writes preferred platform cache path.

## 备选方案

- Hard switch with no legacy fallback: rejected because offline users with existing cache would see avoidable breakage.
- Keep `~/.cache` forever: rejected because it preserves platform inconsistency.

## 风险

- Security: local path resolution only; no command execution.
- Compatibility: cache file location changes; legacy read fallback mitigates.
- Performance: one extra metadata check when preferred path missing.
- Maintenance: centralized helper reduces path drift.

## 测试计划

- [ ] Unit tests for preferred path, fallback path, legacy read fallback.
- [ ] Existing pricing cache/load tests.
- [ ] `cargo check`
- [ ] `cargo test`

## 回滚方案

Revert cache path helper changes. Existing legacy cache remains readable at old path.
