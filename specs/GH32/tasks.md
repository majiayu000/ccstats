# Task Plan

## Linked Issue

GH-32

## Spec Packet

- Product: `product.md`
- Tech: `tech.md`

## 实现任务

- [ ] `SP32-T1` Owner: implementation. Done when: `src/pricing/cache.rs` uses a same-directory temporary file, writes complete JSON, flushes/syncs, then atomically renames over `pricing.json` without truncating the existing cache on failure. Verify: `cargo test pricing::cache`.
- [ ] `SP32-T2` Owner: implementation. Done when: cache reads distinguish missing cache from malformed/unreadable cache, and offline mode fails closed for corrupt JSON instead of returning an empty pricing map. Verify: `cargo test pricing::db`.
- [ ] `SP32-T3` Owner: implementation. Done when: online pricing load continues with freshly fetched data after a cache save failure but emits a visible cache warning, including non-statusline paths. Verify: focused pricing loader tests plus `cargo test`.
- [ ] `SP32-T4` Owner: implementation. Done when: quiet/statusline loading cannot silently convert corrupt cache into all `N/A` costs. Verify: focused statusline or loader test covering quiet mode corrupt cache.
- [ ] `SP32-T5` Owner: verification. Done when: deterministic gates pass for the repo and GH32 spec packet. Verify: `cargo check`, `cargo test`, `python3 checks/check_workflow.py --repo .`, and `python3 checks/check_workflow.py --repo . --spec-dir specs/GH32`.

## 并行拆分

- `SP32-T1` owns `src/pricing/cache.rs`.
- `SP32-T2` and `SP32-T3` own `src/pricing/db.rs` and related pricing tests.
- `SP32-T4` owns statusline/quiet-path tests and must run after the cache read result type is available.
- `SP32-T5` is coordinator-only verification and should not edit production files.

## 验证

- [ ] `SP32-T6` Owner: verification. Done when: malformed cache, missing cache, save failure, and existing online pricing behavior are covered by tests. Verify: `cargo test pricing`.
- [ ] `SP32-T7` Owner: verification. Done when: full Rust and SpecRail checks pass before PR readiness is claimed. Verify: `cargo check`, `cargo test`, `python3 checks/check_workflow.py --repo .`, `python3 checks/check_workflow.py --repo . --spec-dir specs/GH32`.

## Handoff Notes

该 plan 只补实现任务，不代表 `spec_approved` 或 `ready_to_implement`。实现前仍需 maintainer 完成 spec review/approval，并确认离线损坏缓存从静默 `N/A` 变为错误属于可接受的用户可见行为变更。
