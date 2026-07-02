# Task Plan

## Linked Issue

GH-34

## Spec Packet

- Product: `product.md`
- Tech: `tech.md`

## 实现任务

- [ ] `SP34-T1` Owner: investigation. Done when: a local evidence artifact records whether real `~/.claude/projects/**` data contains the same `message.id` in multiple files, including sample count and estimated token impact when present. Verify: rerunnable scan command documented in PR notes without committing private logs.
- [ ] `SP34-T2` Owner: implementation. Done when: if `SP34-T1` confirms cross-file duplicates, Claude entries use source-wide dedup keys while preserving the existing "prefer completed stop_reason, otherwise latest timestamp" rule. Verify: focused dedup/parser tests.
- [ ] `SP34-T3` Owner: implementation. Done when: if `SP34-T1` does not confirm the issue, the implementation path is not taken and the PR documents the per-file scope decision for maintainer review. Verify: no production code change beyond approved documentation.
- [ ] `SP34-T4` Owner: testing. Done when: two Claude jsonl files with the same message id count only once, skipped increments by one, and the completed entry wins. Verify: integration test or loader-level test using temporary fixtures.
- [ ] `SP34-T5` Owner: testing. Done when: a no-duplicate fixture produces unchanged aggregate output. Verify: regression test comparing precomputed expected daily/monthly output.
- [ ] `SP34-T6` Owner: verification. Done when: deterministic Rust and SpecRail gates pass before PR readiness is claimed. Verify: `cargo check`, `cargo test`, `python3 checks/check_workflow.py --repo .`, and `python3 checks/check_workflow.py --repo . --spec-dir specs/GH34`.

## 并行拆分

- `SP34-T1` is read-only investigation and must not commit private Claude log contents.
- `SP34-T2` owns `src/source/claude/parser.rs` and any dedup key helper changes.
- `SP34-T4` and `SP34-T5` own test fixtures and loader/integration tests.
- `SP34-T6` is coordinator-only verification.

## 验证

- [ ] `SP34-T7` Owner: verification. Done when: cross-file duplicate, single-file stream duplicate, and no-duplicate cases all have explicit assertions. Verify: `cargo test claude` or the nearest focused test target.
- [ ] `SP34-T8` Owner: verification. Done when: skipped counts include cross-file duplicates and existing skipped-count behavior is unchanged. Verify: loader-level assertion.

## Handoff Notes

实现必须从 `SP34-T1` 的实证结论开始。若真实日志未发现跨文件重复，本 issue 应转为文档化现状/关闭建议，而不是默认改成全局去重。
