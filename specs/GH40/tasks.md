# Task Plan

## Linked Issue

GH-40

## Spec Packet

- Product: `product.md`
- Tech: `tech.md`

## 实现任务

- [ ] `SP40-T1` Owner: implementation. Done when: load metadata distinguishes `valid`, `dedup_skipped`, and `parse_errors`. Verify: loader unit tests.
- [ ] `SP40-T2` Owner: implementation. Done when: incremental path no longer reports fake skipped 0 and uses true skipped count or explicit unavailable state. Verify: focused incremental loader test.
- [ ] `SP40-T3` Owner: implementation. Done when: JSON/CSV/statusline JSON expose data-quality metadata without removing existing fields. Verify: CLI integration tests.
- [ ] `SP40-T4` Owner: implementation. Done when: all-source aggregation sums metadata across sources. Verify: mixed-source fixture test.
- [ ] `SP40-T5` Owner: verification. Done when: deterministic Rust and SpecRail gates pass before PR readiness is claimed. Verify: `cargo check`, `cargo test`, `python3 checks/check_workflow.py --repo .`, and `python3 checks/check_workflow.py --repo . --spec-dir specs/GH40`.

## 并行拆分

- Loader metadata owns `src/source/mod.rs` and `src/source/loader.rs`.
- Output metadata owns `src/output/{json,csv,statusline}.rs` and app call sites.
- All-source aggregation owns `src/app.rs`.
- Verification is coordinator-only.

## 验证

- [ ] `SP40-T6` Owner: verification. Done when: existing malformed stderr warning test still passes or is explicitly expanded. Verify: `cargo test malformed`.

## Handoff Notes

若 GH33 或 GH38 先合并，优先复用它们的 metadata/output context rather than adding another parallel metadata path.
