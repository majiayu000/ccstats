# Tech Spec

## Linked Issue

GH-32

## Product Spec

Link to `product.md`.

## Codebase Context

| Area | Files | Current behavior | Why relevant |
| --- | --- | --- | --- |
| Pricing cache path/read/write | `src/pricing/cache.rs` | `File::create` truncates target; `serde_json::to_writer` error is ignored; read errors use `.ok()?`. | Primary bug surface for corrupted cache and silent failures. |
| Pricing DB loading | `src/pricing/db.rs` | Offline cache miss falls back to empty `HashMap`; quiet mode suppresses stderr. | Determines whether corrupted cache becomes visible or turns into all `NaN` costs. |
| CLI/statusline callers | `src/app.rs`, `src/output/statusline.rs` | Call pricing load paths that currently cannot distinguish missing cache from corrupted cache. | Must preserve normal output while surfacing correctness failures. |
| Tests | `src/pricing/cache.rs`, `src/pricing/db.rs`, integration tests if needed | Existing tests cover cost math and unknown models, not cache corruption. | New behavior needs focused unit coverage. |

## 设计方案

1. Replace cache read helpers returning `Option<_>` with an internal result type that distinguishes:
   - cache missing
   - cache unreadable/corrupt
   - cache valid
2. Keep missing-cache fallback behavior unchanged where the product spec allows it.
3. Treat corrupt cache as an error for `offline` mode, including quiet/statusline paths.
4. Make `save_raw_cache` write to a same-directory temporary file, serialize the complete JSON, flush/sync it, then `fs::rename` over `pricing.json`.
5. Return or log save errors instead of using `let _ =`; online callers may continue with freshly fetched pricing after warning.
6. Ensure temp file names are process-unique enough for concurrent runs and clean up best-effort on failure.

## Product-to-Test Mapping

| Product invariant | Implementation area | Verification |
| --- | --- | --- |
| P1, P2 | `save_raw_cache` atomic temp+rename | Unit test with existing cache and injected/write-path failure where practical; inspect implementation for no direct truncating write. |
| P3 | Online load after fetch | Unit test or focused integration asserting save failure warns but fetched pricing is still used. |
| P4, P5 | Cache read result and `PricingDb::load_internal` | Tests for missing cache versus malformed cache in offline mode. |
| P6 | quiet/statusline load path | Test that corrupt cache does not silently return empty pricing in quiet mode. |

## 数据流

`fetch_litellm_raw` returns raw JSON data. `save_raw_cache` persists it through a temporary file in the cache directory. Later `PricingDb::load_internal` reads cache state, parses valid data into `PricingDb::from_raw_data`, handles missing cache as existing fallback behavior, and reports corrupt cache as an error for offline correctness.

## 备选方案

- Keep `Option` and only add warning logs: rejected because it still cannot distinguish missing cache from corrupt cache in offline mode.
- Use a third-party atomic-write crate: unnecessary for this small local file and would add dependency surface.

## 风险

- Security: no new external input execution; path remains local cache path.
- Compatibility: corrupt cache now errors in offline mode, which is intentional but user-visible.
- Performance: fsync adds minimal overhead only after online pricing fetch.
- Maintenance: result type adds some complexity but removes silent cache-state ambiguity.

## 测试计划

- [ ] Unit tests for malformed cache read.
- [ ] Unit tests for missing cache read.
- [ ] Unit tests around atomic save behavior and warning/error path where feasible.
- [ ] Existing pricing DB tests.
- [ ] `cargo check`
- [ ] `cargo test`

## 回滚方案

Revert the cache result-type and atomic-write changes. If needed, delete a corrupt local `pricing.json` to restore existing missing-cache fallback behavior.
