# Task Plan

## Linked Issue

GH-35

## Spec Packet

- Product: `product.md`
- Tech: `tech.md`

## 实现任务

- [ ] `SP35-T1` Owner: implementation. Done when: source capabilities can express whether a source cost is estimated, with Grok marked estimated and other current sources left unchanged. Verify: unit test covering all registered sources.
- [ ] `SP35-T2` Owner: implementation. Done when: `--source all` aggregation separates real costs from estimated Grok costs and never adds estimated cost into the real total. Verify: aggregation unit or integration test with Claude/Codex plus Grok fixtures.
- [ ] `SP35-T3` Owner: implementation. Done when: table, JSON, and CSV outputs expose real total cost and estimated Grok cost as machine/human-distinguishable values. Verify: CLI integration tests for all three formats.
- [ ] `SP35-T4` Owner: implementation. Done when: single-source Grok output preserves token information and adds estimated-cost labeling without changing non-cost fields. Verify: Grok-specific CLI integration tests.
- [ ] `SP35-T5` Owner: implementation. Done when: monthly budget/forecast uses only real-cost semantics and excludes Grok estimated cost. Verify: budget integration test with mixed real and Grok data.
- [ ] `SP35-T6` Owner: verification. Done when: deterministic Rust and SpecRail gates pass before PR readiness is claimed. Verify: `cargo check`, `cargo test`, `python3 checks/check_workflow.py --repo .`, and `python3 checks/check_workflow.py --repo . --spec-dir specs/GH35`.

## 并行拆分

- Capability work owns `src/source/mod.rs` and source config/capability tests.
- Aggregation work owns `src/app.rs` all-source aggregation paths.
- Output work owns `src/output/{json,csv,table,budget}.rs` and associated fixtures.
- Verification owns final command execution and PR evidence only.

## 验证

- [ ] `SP35-T7` Owner: verification. Done when: mixed real+Grok data, Grok-only data, and no-Grok data are all covered. Verify: CLI integration tests.
- [ ] `SP35-T8` Owner: verification. Done when: future GH18 true Grok usage can switch semantics through capability state instead of source-name string checks. Verify: code review plus capability test with no `name() == "grok"` aggregation gate.

## Handoff Notes

该 plan 不要求改变 Grok token parser。实现应只改变成本语义分流与输出标注，并在 release notes 中说明 `all` 总成本可能变小但更准确。
