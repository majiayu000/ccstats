# Task Plan

## Linked Issue

GH-33

## Spec Packet

- Product: `product.md`
- Tech: `tech.md`

## 实现任务

- [ ] `SP33-T1` Owner: implementation. Done when: `PricingDb` exposes a session-level `pricing_source` enum covering `live`, `cache`, `cache_stale`, `fallback`, and `mixed`, including cache age when available. Verify: focused unit tests in pricing load/cache modules.
- [ ] `SP33-T2` Owner: implementation. Done when: model price resolution records fallback hits and composes them with the DB-level source so partial fallback becomes `mixed`. Verify: unit tests for all source combinations.
- [ ] `SP33-T3` Owner: implementation. Done when: JSON outputs carry machine-readable `pricing_source` metadata without changing existing field meanings. Verify: CLI integration tests for JSON daily/monthly/statusline paths.
- [ ] `SP33-T4` Owner: implementation. Done when: CSV outputs expose the same source value without breaking existing data columns, using the agreed comment-row or metadata strategy. Verify: CLI integration tests for CSV outputs.
- [ ] `SP33-T5` Owner: implementation. Done when: table outputs add a human-visible pricing-source footnote for non-live/non-fresh-cache sources, including stale-cache age when known. Verify: CLI integration tests for table output.
- [ ] `SP33-T6` Owner: verification. Done when: five source scenarios are covered: live, fresh cache, stale cache, full fallback, and mixed fallback. Verify: `cargo test`.
- [ ] `SP33-T7` Owner: verification. Done when: deterministic Rust and SpecRail gates pass before PR readiness is claimed. Verify: `cargo check`, `cargo test`, `python3 checks/check_workflow.py --repo .`, and `python3 checks/check_workflow.py --repo . --spec-dir specs/GH33`.

## 并行拆分

- Pricing source detection owns `src/pricing/db.rs`, `src/pricing/cache.rs`, and pricing resolver tests.
- Output propagation owns `src/output/{json,csv,table,statusline}.rs` and must not change pricing behavior.
- Integration fixtures/tests own `tests/cli_integration.rs` or split test files if GH47 has already landed.
- Final verification is coordinator-only and should not edit implementation files.

## 验证

- [ ] `SP33-T8` Owner: verification. Done when: JSON and CSV report identical `pricing_source` for the same fixture run. Verify: paired CLI integration assertions.
- [ ] `SP33-T9` Owner: verification. Done when: unknown-model `NaN` -> `N/A` behavior remains unchanged. Verify: existing unknown pricing tests plus targeted regression if needed.

## Handoff Notes

该 plan 只补任务，不代表 `spec_approved` 或 `ready_to_implement`。实现前需要 maintainer 决定 CSV metadata 具体形态，并确认新增 JSON top-level 字段的兼容性说明。
