# Task Plan

## Linked Issue

GH-45

## Spec Packet

- Product: `product.md`
- Tech: `tech.md`

## 实现任务

- [ ] `SP45-T1` Owner: implementation. Done when: `UsageSource::from_str` delegates accepted names and aliases through the source registry instead of maintaining duplicate alias literals in `src/sdk.rs`. Verify: unit tests for every registry source and alias.
- [ ] `SP45-T2` Owner: implementation. Done when: SDK enum/name mapping remains type-safe and has an explicit future-source policy such as `#[non_exhaustive]` if approved during review. Verify: SDK integration tests and compile check.
- [ ] `SP45-T3` Owner: implementation. Done when: an automated consistency test proves registry source choices and SDK parseable sources stay aligned. Verify: test traversing `source_choices()` and `UsageSource::from_str`.
- [ ] `SP45-T4` Owner: implementation. Done when: `Capabilities` includes `has_tool_calls`, Claude sets it true, other current sources set it false, and `ccstats tools` gates by capability rather than string name checks. Verify: CLI/tools tests for Claude and non-Claude sources.
- [ ] `SP45-T5` Owner: implementation. Done when: tool-call file discovery is owned by the relevant source implementation, so loader no longer independently hardcodes `~/.claude/projects`. Verify: loader/source tests, with GH42 path override compatibility noted if GH42 lands first.
- [ ] `SP45-T6` Owner: verification. Done when: deterministic Rust and SpecRail gates pass before PR readiness is claimed. Verify: `cargo check`, `cargo test`, `python3 checks/check_workflow.py --repo .`, and `python3 checks/check_workflow.py --repo . --spec-dir specs/GH45`.

## 并行拆分

- SDK/registry consistency owns `src/sdk.rs`, `src/source/registry.rs`, and SDK tests.
- Capability gating owns `src/source/mod.rs`, source capability declarations, and `src/app.rs`.
- Tool file discovery owns `src/source/loader.rs` plus Claude source path helpers.
- Verification is coordinator-only and should check for duplicate alias literals after implementation.

## 验证

- [ ] `SP45-T7` Owner: verification. Done when: aliases such as `cc`, `cx`, `cur`, and `gx` parse consistently through CLI and SDK paths. Verify: SDK and CLI integration assertions.
- [ ] `SP45-T8` Owner: verification. Done when: no capability gate in `src/app.rs` depends on `source.name() == "claude"` or `!= "claude"`. Verify: `rg '"claude"' src/app.rs` plus code review.

## Handoff Notes

若 GH42 先合并，`SP45-T5` 必须复用其 Claude path override helper；若 GH45 先合并，应在 PR body 中标明 GH42 后续只需改 Claude source path helper，不应再改 loader 硬编码路径。
