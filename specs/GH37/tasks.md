# Task Plan

## Linked Issue

GH-37

## Spec Packet

- Product: `product.md`
- Tech: `tech.md`

## 实现任务

- [ ] `SP37-T1` Owner: implementation. Done when: `merge_day_stats` lives in one shared core module and both loader/app callers use it. Verify: `rg "fn merge_day_stats" src` shows one definition.
- [ ] `SP37-T2` Owner: implementation. Done when: existing merge behavior tests cover the shared helper owner. Verify: `cargo test merge_day_stats`.
- [ ] `SP37-T3` Owner: implementation. Done when: no compatibility wrappers or duplicate helper bodies remain. Verify: code review and `rg "merge_day_stats" src/source/loader.rs src/app.rs src/core`.
- [ ] `SP37-T4` Owner: verification. Done when: deterministic Rust and SpecRail gates pass before PR readiness is claimed. Verify: `cargo check`, `cargo test`, `python3 checks/check_workflow.py --repo .`, and `python3 checks/check_workflow.py --repo . --spec-dir specs/GH37`.

## 并行拆分

- Shared helper move owns `src/core/aggregator.rs`, `src/source/loader.rs`, and `src/app.rs`.
- Test migration owns the helper unit tests in the chosen owner module.
- Verification is coordinator-only.

## 验证

- [ ] `SP37-T5` Owner: verification. Done when: all-source aggregation still compiles and existing CLI integration tests pass. Verify: `cargo test`.

## Handoff Notes

这是可作为 `exception_allowed` 的小型维护修复，但本 PR 仅提交 spec packet。实现 PR 可保持很小并关闭 #37。
