# Task Plan

## Linked Issue

GH-42

## Spec Packet

- Product: `product.md`
- Tech: `tech.md`

## 实现任务

- [ ] `SP42-T1` Owner: implementation. Done when: Claude file discovery uses `CLAUDE_CONFIG_DIR` as config root before falling back to `$HOME/.claude`. Verify: integration test with temp config dir.
- [ ] `SP42-T2` Owner: implementation. Done when: default Claude path behavior remains unchanged. Verify: existing Claude tests.
- [ ] `SP42-T3` Owner: docs. Done when: README/docs document `CLAUDE_CONFIG_DIR` and `CODEX_HOME` alongside existing source env vars. Verify: docs diff review.
- [ ] `SP42-T4` Owner: compatibility. Done when: if GH45 is present, tool-call discovery reuses the same helper. Verify: tools/Claude test or code review.
- [ ] `SP42-T5` Owner: verification. Done when: deterministic Rust and SpecRail gates pass before PR readiness is claimed. Verify: `cargo check`, `cargo test`, `python3 checks/check_workflow.py --repo .`, and `python3 checks/check_workflow.py --repo . --spec-dir specs/GH42`.

## 并行拆分

- Claude discovery owns `src/source/claude/parser.rs` and Claude tests.
- Docs owns `README.md` and relevant docs files.
- GH45 compatibility is conditional and must avoid conflicting edits if GH45 lands first.

## 验证

- [ ] `SP42-T6` Owner: verification. Done when: nonexistent `CLAUDE_CONFIG_DIR` does not panic. Verify: focused test or manual no-data command.

## Handoff Notes

实现应 use Claude Code's existing variable name exactly: `CLAUDE_CONFIG_DIR`.
