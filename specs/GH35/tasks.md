# Task Plan

## Linked Issue

GH-35

## Spec Packet

- Product: `product.md`
- Tech: `tech.md`

## 实现任务

- [ ] `SP35-T1` Owner: implementation. Done when: entries/aggregates carry cost kind/provenance (`real` vs `estimated_proxy`, plus aggregate `mixed` when needed); source capability may provide defaults but is not the only fact source. Verify: unit tests covering all current sources and mixed aggregates.
- [ ] `SP35-T2` Owner: implementation. Done when: `--source all` aggregation separates real costs from estimated Grok costs and never adds estimated cost into the real total. Verify: aggregation unit or integration test with Claude/Codex plus Grok fixtures.
- [ ] `SP35-T3` Owner: implementation. Done when: table, JSON, CSV, statusline, top, and SDK outputs expose real total cost and estimated Grok cost as distinguishable values without changing JSON array roots. Verify: CLI and SDK integration tests.
- [ ] `SP35-T4` Owner: implementation. Done when: single-source Grok output preserves token information and adds estimated-cost labeling in table, JSON, CSV, and SDK summaries. Verify: Grok-specific CLI/SDK integration tests.
- [ ] `SP35-T5` Owner: implementation. Done when: monthly budget/forecast, all-source statusline, and all-source top ranking use only real-cost semantics for real spend totals/shares while exposing estimated proxy cost separately where relevant. Verify: budget/statusline/top integration tests with mixed real and Grok data.
- [ ] `SP35-T6` Owner: verification. Done when: deterministic Rust and SpecRail gates pass before PR readiness is claimed. Verify: `cargo check`, `cargo test`, `python3 checks/check_workflow.py --repo .`, and `python3 checks/check_workflow.py --repo . --spec-dir specs/GH35`.

## 并行拆分

- Cost provenance work owns `src/core/types.rs`, `src/source/mod.rs`, and parser/aggregate defaults.
- Aggregation work owns `src/app.rs` all-source aggregation paths.
- Output work owns `src/output/{json,csv,table,budget,statusline,top}.rs`, `src/sdk.rs`, and associated fixtures.
- Verification owns final command execution and PR evidence only.

## 验证

- [ ] `SP35-T7` Owner: verification. Done when: mixed real+Grok data, Grok-only data, and no-Grok data are all covered. Verify: CLI integration tests.
- [ ] `SP35-T8` Owner: verification. Done when: future GH18 true Grok usage can coexist with historical proxy records through entry/aggregate provenance instead of source-name string checks or source-wide boolean flips. Verify: code review plus mixed cost-kind test.

## Handoff Notes

该 plan 不要求改变 Grok token parser。实现应只改变成本语义分流与输出标注，并在 release notes 中说明 `all` 总成本可能变小但更准确。
