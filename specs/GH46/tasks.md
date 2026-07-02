# Task Plan

## Linked Issue

GH-46

## Spec Packet

- Product: `product.md`
- Tech: `tech.md`

## 实现任务

- [ ] `SP46-T1` Owner: implementation. Done when: config loading distinguishes missing config from invalid existing config. Verify: config unit tests.
- [ ] `SP46-T2` Owner: implementation. Done when: invalid config exits with clear error in normal, quiet, and statusline paths. Verify: CLI integration tests.
- [ ] `SP46-T3` Owner: docs. Done when: README/docs document config search order, supported keys, example config, and source env vars including `CODEX_HOME`. Verify: docs diff review.
- [ ] `SP46-T4` Owner: verification. Done when: legal config and missing-config defaults still work. Verify: config/CLI regression tests.
- [ ] `SP46-T5` Owner: verification. Done when: deterministic Rust and SpecRail gates pass before PR readiness is claimed. Verify: `cargo check`, `cargo test`, `python3 checks/check_workflow.py --repo .`, and `python3 checks/check_workflow.py --repo . --spec-dir specs/GH46`.

## 并行拆分

- Config behavior owns `src/config.rs` and startup error propagation files.
- Docs own `README.md` and relevant docs.
- Tests own config/CLI integration fixtures.

## 验证

- [ ] `SP46-T6` Owner: verification. Done when: no test weakens invalid-config expectations. Verify: full `cargo test`.

## Handoff Notes

GH42 may add `CLAUDE_CONFIG_DIR`; if it lands first, include it in the env override docs for this issue.
