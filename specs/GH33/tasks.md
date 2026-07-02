# Task Plan

## Linked Issue

GH-33

## Spec Packet

- Product: `product.md`
- Tech: `tech.md`

## 实现任务

- [ ] `SP33-T1` Owner: implementation. Done when: `PricingDb` exposes `pricing_source` plus cache age/mtime covering `live`, `cache`, `cache_stale`, `fallback`, and `mixed`. Verify: focused unit tests in pricing load/cache modules.
- [ ] `SP33-T2` Owner: implementation. Done when: model price resolution records known/fallback/unknown source counts so all-fallback differs from mixed. Verify: unit tests for known-only, fallback-only, mixed, and unknown scenarios.
- [ ] `SP33-T3` Owner: implementation. Done when: JSON outputs carry `pricing_source` on existing row/object shapes without replacing array roots or changing unknown-cost `null`. Verify: CLI integration tests for JSON daily/monthly/session/project/blocks/top/statusline paths.
- [ ] `SP33-T4` Owner: implementation. Done when: CSV outputs expose the same source value without moving header line 0 or inserting pre-header comments. Verify: CLI integration tests for CSV outputs.
- [ ] `SP33-T5` Owner: implementation. Done when: table outputs add a human-visible pricing-source footnote for every non-live source, including fresh cache and stale-cache age when known. Verify: CLI integration tests for table output.
- [ ] `SP33-T6` Owner: verification. Done when: five source scenarios are covered: live, fresh cache, stale cache, full fallback, and mixed fallback. Verify: `cargo test`.
- [ ] `SP33-T7` Owner: verification. Done when: deterministic Rust and SpecRail gates pass before PR readiness is claimed. Verify: `cargo check`, `cargo test`, `python3 checks/check_workflow.py --repo .`, and `python3 checks/check_workflow.py --repo . --spec-dir specs/GH33`.

## 并行拆分

- Pricing source detection owns `src/pricing/db.rs`, `src/pricing/cache.rs`, and pricing resolver tests.
- Output propagation owns `src/output/{json,csv,table,statusline,session,project,blocks,top,budget}.rs` and must not change pricing behavior.
- Integration fixtures/tests own `tests/cli_integration.rs` or split test files if GH47 has already landed.
- Final verification is coordinator-only and should not edit implementation files.

## 验证

- [ ] `SP33-T8` Owner: verification. Done when: JSON and CSV report identical `pricing_source` for the same fixture run. Verify: paired CLI integration assertions.
- [ ] `SP33-T9` Owner: verification. Done when: unknown-model behavior remains unchanged: JSON cost is `null`, table/CSV display `N/A`. Verify: existing unknown pricing tests plus targeted regression if needed.

## Handoff Notes

该 plan 只补任务，不代表 `spec_approved` 或 `ready_to_implement`。实现前需要 maintainer 决定 CSV 追加列/metadata row 的具体形态；不得改成 JSON top-level envelope 或 CSV pre-header comment。
