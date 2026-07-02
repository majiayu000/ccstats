# Task Plan

## Linked Issue

GH-47

## Spec Packet

- Product: `product.md`
- Tech: `tech.md`

## 实现任务

- [ ] `SP47-T1` Owner: implementation. Done when: internal output `SummaryOptions` is renamed without changing SDK `SummaryOptions` or output behavior. Verify: `cargo check` and SDK/output tests.
- [ ] `SP47-T2` Owner: implementation. Done when: loader tests are split out so `src/source/loader.rs` is below 800 lines. Verify: `wc -l src/source/loader.rs` and `cargo test merge_day_stats`.
- [ ] `SP47-T3` Owner: implementation. Done when: table output is split below 800 lines with exported function contracts preserved. Verify: `wc -l src/output/table.rs` and affected CLI output tests.
- [ ] `SP47-T4` Owner: implementation. Done when: CSV output has a concrete split or remains below 800 with a documented follow-up boundary. Verify: `wc -l src/output/csv.rs` and CSV CLI tests.
- [ ] `SP47-T5` Owner: implementation. Done when: `tests/cli_integration.rs` is split into focused integration files with shared helpers. Verify: `cargo test --test <new-targets>` and full `cargo test`.
- [ ] `SP47-T6` Owner: implementation. Done when: issue-named `too_many_lines` allows on `parse_codex_file_with_debug` and `run_cli` are removed through helper extraction. Verify: `rg "allow\\(clippy::too_many_lines\\)" src/source/codex/parser.rs src/lib.rs` has no matches.
- [ ] `SP47-T7` Owner: verification. Done when: deterministic Rust and SpecRail gates pass before PR readiness is claimed. Verify: `cargo check`, `cargo test`, `python3 checks/check_workflow.py --repo .`, and `python3 checks/check_workflow.py --repo . --spec-dir specs/GH47`.

## 并行拆分

- `SP47-T1` owns `src/output/table.rs`, `src/output/mod.rs`, `src/app.rs`.
- `SP47-T2` owns loader test module files only.
- `SP47-T3` and `SP47-T4` must not edit the same output helper modules concurrently.
- `SP47-T5` owns `tests/` integration layout and shared helper.
- `SP47-T6` owns `src/source/codex/parser.rs` and `src/lib.rs`.

## 验证

- [ ] `SP47-T8` Owner: verification. Done when: before/after `wc -l` evidence is recorded for `loader.rs`, `table.rs`, `csv.rs`, and `cli_integration.rs`. Verify: `wc -l ...`.
- [ ] `SP47-T9` Owner: verification. Done when: full `cargo test` passes after each slice before the next slice starts. Verify: per-PR verification logs.

## Handoff Notes

This issue should produce multiple implementation PRs. Use `Refs #47` for partial slices and closing keywords only on the final slice that satisfies all acceptance criteria.
