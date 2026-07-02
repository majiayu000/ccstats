# Task Plan

## Linked Issue

GH-43

## Spec Packet

- Product: `product.md`
- Tech: `tech.md`

## 实现任务

- [ ] `SP43-T1` Owner: implementation. Done when: pricing cache path helper prefers `dirs::cache_dir()/ccstats/pricing.json`. Verify: unit tests for path selection.
- [ ] `SP43-T2` Owner: implementation. Done when: load path can read legacy `~/.cache/ccstats/pricing.json` when preferred cache is absent. Verify: legacy fallback test.
- [ ] `SP43-T3` Owner: implementation. Done when: save path writes to the current preferred/fallback platform path, not the legacy path. Verify: save target test.
- [ ] `SP43-T4` Owner: implementation. Done when: `PricingDb` callers do not duplicate cache path logic. Verify: code review and `rg "\\.cache.*pricing\\.json" src`.
- [ ] `SP43-T5` Owner: verification. Done when: deterministic Rust and SpecRail gates pass before PR readiness is claimed. Verify: `cargo check`, `cargo test`, `python3 checks/check_workflow.py --repo .`, and `python3 checks/check_workflow.py --repo . --spec-dir specs/GH43`.

## 并行拆分

- Cache helper work owns `src/pricing/cache.rs`.
- Pricing DB integration owns `src/pricing/db.rs` only if API shape changes.
- Tests own pricing cache/db tests.

## 验证

- [ ] `SP43-T6` Owner: verification. Done when: offline mode with legacy-only cache remains usable. Verify: focused pricing DB test.

## Handoff Notes

GH32 may change cache read/write error handling. If GH32 lands first, implement GH43 by extending the new cache-state API rather than reintroducing `Option` ambiguity.
