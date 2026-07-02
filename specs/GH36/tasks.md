# Task Plan

## Linked Issue

GH-36

## Spec Packet

- Product: `product.md`
- Tech: `tech.md`

## 实现任务

- [ ] `SP36-T1` Owner: implementation. Done when: Codex duplicate detection compares the normalized cumulative usage vector instead of only `total_tokens`. Verify: parser unit tests for equal and differing vectors.
- [ ] `SP36-T2` Owner: implementation. Done when: missing or zero `total_tokens` with growing component tokens produces a non-zero delta. Verify: focused parser test plus CLI integration fixture.
- [ ] `SP36-T3` Owner: implementation. Done when: existing `last_token_usage` priority and source-wide replay dedup behavior are unchanged. Verify: existing Codex integration tests.
- [ ] `SP36-T4` Owner: verification. Done when: deterministic Rust and SpecRail gates pass before PR readiness is claimed. Verify: `cargo check`, `cargo test`, `python3 checks/check_workflow.py --repo .`, and `python3 checks/check_workflow.py --repo . --spec-dir specs/GH36`.

## 并行拆分

- Parser semantics owns `src/source/codex/parser.rs`.
- CLI regression owns `tests/cli_integration.rs` or a split Codex integration file if GH47 lands first.
- Verification is coordinator-only.

## 验证

- [ ] `SP36-T5` Owner: verification. Done when: existing replay, cache, reasoning, and date-boundary Codex tests still pass. Verify: `cargo test codex`.
- [ ] `SP36-T6` Owner: verification. Done when: no assertions are weakened. Verify: code review plus full `cargo test`.

## Handoff Notes

实现 PR 应 use closing keyword only when all acceptance criteria are met. 本 spec packet 本身不代表 `ready_to_implement`;仍需 maintainer spec approval。
