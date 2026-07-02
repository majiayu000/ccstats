# Task Plan

## Linked Issue

GH-38

## Spec Packet

- Product: `product.md`
- Tech: `tech.md`

## 实现任务

- [ ] `SP38-T1` Owner: implementation. Done when: a single `OutputFormat` enum represents table/json/csv selection and CLI flags derive that enum in one place while preserving CSV-over-JSON behavior when both flags are present. Verify: unit test for default, json, csv, and json+csv precedence.
- [ ] `SP38-T2` Owner: implementation. Done when: session, project, blocks, top, tools, sources, and period rendering each have one format-dispatch entry point instead of repeated `if cli.csv` / `else if cli.json` branches in handlers. Verify: `rg "if cli\\.csv|else if cli\\.json" src/app.rs` returns no handler-level matches.
- [ ] `SP38-T3` Owner: implementation. Done when: `handle_sources` table/CSV/JSON expose the same capability column set, including `has_cache_creation` and `needs_dedup`. Verify: integration tests comparing source output columns across formats.
- [ ] `SP38-T4` Owner: implementation. Done when: statusline remains on its existing special path and is documented/isolated from the generic `OutputFormat` abstraction. Verify: existing statusline tests pass unchanged.
- [ ] `SP38-T5` Owner: verification. Done when: existing output snapshots/assertions pass, with only explicit source-column fixture updates where the spec permits behavior change. Verify: `cargo test`.
- [ ] `SP38-T6` Owner: verification. Done when: deterministic Rust and SpecRail gates pass before PR readiness is claimed. Verify: `cargo check`, `cargo test`, `python3 checks/check_workflow.py --repo .`, and `python3 checks/check_workflow.py --repo . --spec-dir specs/GH38`.

## 并行拆分

- Format derivation owns `src/cli/args.rs` and `src/output/mod.rs`.
- Handler dispatch owns `src/app.rs`; split by handler only if parallel lanes keep non-overlapping hunks.
- Source column parity owns source output functions and source CLI integration tests.
- Verification is coordinator-only and should run after all handler changes are integrated.

## 验证

- [ ] `SP38-T7` Owner: verification. Done when: daily/session/project/top/tools/sources representative commands are checked in table, JSON, and CSV where supported. Verify: existing plus new CLI integration tests.
- [ ] `SP38-T8` Owner: verification. Done when: implementation does not introduce a trait or plugin layer until a real fourth format exists, and PR docs do not claim O(1) new-format support from this slice. Verify: code review against tech spec non-goal.

## Handoff Notes

该 plan 是重构任务，PR 应保持小步、输出等价优先。任何列集变化仅限 GH38 已批准的 `handle_sources` capability columns。
