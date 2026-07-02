# Task Plan

## Linked Issue

GH-39

## Spec Packet

- Product: `product.md`
- Tech: `tech.md`

## 实现任务

- [ ] `SP39-T1` Owner: implementation. Done when: Claude parser clamps negative usage fields before constructing `RawEntry`. Verify: `cargo test claude`.
- [ ] `SP39-T2` Owner: implementation. Done when: Cursor bubble and generation parse paths clamp negative token fields and all-negative records do not produce meaningful usage. Verify: `cargo test cursor`.
- [ ] `SP39-T3` Owner: implementation. Done when: no negative token can reach aggregate JSON output from malformed Claude/Cursor fixtures. Verify: focused CLI integration test if unit tests do not cover output boundary.
- [ ] `SP39-T4` Owner: verification. Done when: deterministic Rust and SpecRail gates pass before PR readiness is claimed. Verify: `cargo check`, `cargo test`, `python3 checks/check_workflow.py --repo .`, and `python3 checks/check_workflow.py --repo . --spec-dir specs/GH39`.

## 并行拆分

- Claude parser work owns `src/source/claude/parser.rs`.
- Cursor parser work owns `src/source/cursor/parser.rs`.
- Integration verification owns test fixtures only.

## 验证

- [ ] `SP39-T5` Owner: verification. Done when: positive and missing-field parser tests remain unchanged. Verify: existing parser test suite.

## Handoff Notes

实现应遵循现有 Codex/Grok `.max(0)` 先例。不要在输出层隐藏负值；负值必须在 parser boundary 处理。
