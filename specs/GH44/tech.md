# Tech Spec

## Linked Issue

GH-44

## Product Spec

`specs/GH44/product.md`

## Codebase Context

| Area | Files | Current behavior | Why relevant |
| --- | --- | --- | --- |
| Resolver | `src/pricing/resolver/resolve.rs` | Filters candidates with `name_lower.contains(model_lower) || model_lower.contains(name_lower)` and picks longest. | Primary bug surface. |
| Parser variants | `src/pricing/resolver/parse.rs` | Adds variant keys such as dot-version forms. | Existing safe matching mechanism. |
| Fallback pricing | `src/pricing/resolver/fallback.rs` | Handles broad family estimates separately. | Should remain the fallback, not LiteLLM mis-match. |
| Pricing DB | `src/pricing/db.rs` | Calls known resolver then fallback. | Unknown path should remain valid. |

## 设计方案

1. Keep exact lowercase key lookup as first path.
2. Define a small normalization function for accepted aliases/prefixes:
   - known provider prefix stripping where already expected
   - version punctuation variants from existing parse logic
3. Replace arbitrary substring filter with ordered deterministic candidates from normalization only.
4. If more than one distinct candidate remains after normalization, return `None` and let fallback/unknown handling decide.
5. Add tests for existing positive cases plus negative ambiguous/substring cases.
6. Optionally add a fixture using a small LiteLLM-like key set that reproduces the current risk.

## Product-to-Test Mapping

| Product invariant | Implementation area | Verification |
| --- | --- | --- |
| P1 | `resolve_pricing_known` | exact match test. |
| P2/P4 | normalization helper | sonnet/version variant tests. |
| P3 | ambiguity handling | multi-candidate test returns None. |
| P5 | regression fixture | substring false-positive test. |

## 数据流

Model name -> normalized candidate generation -> exact candidate lookup -> optional unambiguous variant lookup -> known pricing or None -> fallback pricing path.

## 备选方案

- Keep longest substring but add denylist: rejected because key space changes over time.
- Fuzzy string distance matching: rejected because silent price selection still lacks deterministic safety.

## 风险

- Security: no external execution.
- Compatibility: some previous fuzzy matches may become `N/A` or fallback; safer than wrong known price.
- Performance: fewer candidate scans likely faster.
- Maintenance: normalization rules must be explicit and tested.

## 测试计划

- [ ] Unit tests for exact, provider prefix, version variant, no match.
- [ ] Negative tests for substring false positives and ambiguity.
- [ ] Existing pricing DB/fallback tests.
- [ ] `cargo check`
- [ ] `cargo test`

## 回滚方案

Revert resolver strategy and tests. No data migration.
