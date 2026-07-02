# Task Plan

## Linked Issue

GH-44

## Spec Packet

- Product: `product.md`
- Tech: `tech.md`

## 实现任务

- [ ] `SP44-T1` Owner: implementation. Done when: `resolve_pricing_known` removes unbounded bidirectional `contains` matching. Verify: code review and resolver unit tests.
- [ ] `SP44-T2` Owner: implementation. Done when: exact, normalized prefix, and approved version variant matches still work. Verify: resolver tests.
- [ ] `SP44-T3` Owner: implementation. Done when: ambiguous or substring-only candidates return `None` rather than a silent wrong price. Verify: negative resolver tests.
- [ ] `SP44-T4` Owner: verification. Done when: pricing DB fallback/unknown behavior remains correct. Verify: `cargo test pricing`.
- [ ] `SP44-T5` Owner: verification. Done when: deterministic Rust and SpecRail gates pass before PR readiness is claimed. Verify: `cargo check`, `cargo test`, `python3 checks/check_workflow.py --repo .`, and `python3 checks/check_workflow.py --repo . --spec-dir specs/GH44`.

## 并行拆分

- Resolver strategy owns `src/pricing/resolver/resolve.rs`.
- Variant preservation owns resolver/parse tests.
- Verification is coordinator-only.

## 验证

- [ ] `SP44-T6` Owner: verification. Done when: at least one realistic false-positive fixture is documented or encoded as a regression test. Verify: resolver test name and PR body evidence.

## Handoff Notes

This issue is medium-confidence until a concrete key-space fixture is added. Implementation should prefer returning unknown over guessing.
