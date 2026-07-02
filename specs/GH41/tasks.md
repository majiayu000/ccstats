# Task Plan

## Linked Issue

GH-41

## Spec Packet

- Product: `product.md`
- Tech: `tech.md`

## 实现任务

- [ ] `SP41-T1` Owner: investigation. Done when: PR records whether `cursorDiskKV` and `ItemTable` can overlap, without committing private Cursor DB data. Verify: documented query/fixture and maintainer-reviewable conclusion.
- [ ] `SP41-T2` Owner: implementation. Done when: overlap conclusion is encoded as either dedup behavior or a tested non-overlap invariant. Verify: Cursor parser tests.
- [ ] `SP41-T3` Owner: implementation. Done when: readonly Cursor DB connections set a conservative `busy_timeout`. Verify: focused parser/open test or code-level assertion.
- [ ] `SP41-T4` Owner: implementation. Done when: `project_path` and `has_projects` are consistent. Verify: CLI project test if enabled, or parser test proving no dead field is exposed.
- [ ] `SP41-T5` Owner: verification. Done when: deterministic Rust and SpecRail gates pass before PR readiness is claimed. Verify: `cargo check`, `cargo test`, `python3 checks/check_workflow.py --repo .`, and `python3 checks/check_workflow.py --repo . --spec-dir specs/GH41`.

## 并行拆分

- Investigation is read-only and owns no production files.
- Cursor parser work owns `src/source/cursor/parser.rs`.
- Capability work owns `src/source/cursor/config.rs` and output/project tests.
- Verification is coordinator-only.

## 验证

- [ ] `SP41-T6` Owner: verification. Done when: basic Cursor source tests still pass and no new double-count risk is introduced. Verify: `cargo test cursor`.

## Handoff Notes

不要在没有 `SP41-T1` 证据的情况下直接选择双表 dedup 策略。Cursor 源仍保持 experimental。
