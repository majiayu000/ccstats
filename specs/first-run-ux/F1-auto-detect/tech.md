# Tech Spec — F1 Auto-detect default source

## Linked Issue

https://github.com/majiayu000/ccstats/issues/164

## Product Spec

`specs/first-run-ux/F1-auto-detect/product.md`

## Codebase Context

| Area | Files | Current behavior | Why relevant |
| --- | --- | --- | --- |
| Source resolution | `src/lib.rs` `resolve_source_name` | `(None, None) => "claude"` | Primary change. |
| Diagnostics | `src/source/mod.rs` `Source::diagnose`, `DiagnosticStatus` | Detected / Configured / Missing, no network | Ready-set definition. |
| Catalog | `src/catalog.rs` `diagnose_usage_sources` | SDK projection of the same diagnose() | Reuse; do not fork a second detector. |
| Dispatch | `src/lib.rs` `dispatch_command` | `all` → `handle_all_sources_command` | Multi-ready path already exists. |
| Doctor | `src/doctor_cmd.rs` | Prints all 29 rows; empty hint to rerun doctor --json | Empty default should call this renderer, not duplicate strings. |
| Statusline | `src/output/statusline.rs`, `SourceCommand::is_statusline` | Quiet hook output | 0-ready must not dump doctor table. |
| Tests | `tests/cli_doctor.rs`, `tests/cli_codex_source_routing.rs`, `tests/common/mod.rs` | Isolated HOME + env scrub | New fixtures for 0/1/N ready. |

## 设计方案

1. Add `fn ready_source_names() -> Vec<&'static str>` (or crate-visible helper next to registry) that maps `all_sources()` through `diagnose()` and keeps Detected + Configured, preserving registry order.
2. Change `resolve_source_name` so the `(None, None)` arm:
   - if command is Doctor or Sources: keep returning a dummy (today `"claude"`) because those commands ignore it;
   - else if `ready` is empty: return a new sentinel **or** handle before dispatch.
3. Prefer an explicit empty path in `run_cli` after resolve:

   ```text
   if no explicit source and ready.is_empty() && command is a usage view:
     if statusline: print quiet empty statusline; return
     else: handle_doctor(ctx); return
   ```

   Do not invent a fake source named `"none"`.
4. If ready.len() == 1, resolve to that name. If len() >= 2, resolve to `all` (`consts::ALL_SOURCES`).
5. Explicit `--source` / hint / config remain as today. Config merge already happens before resolve via `cli.with_config`.
6. Do not parse files during detect. `diagnose()` for local sources uses `find_files().len()`; Cursor uses env/file existence only.

## Product-to-Test Mapping

| Product invariant | Implementation | Verification |
| --- | --- | --- |
| P1 priority | resolve order in `resolve_source_name` + run_cli empty branch | routing tests with --source, `ccstats codex`, config.toml |
| P3 ready set | helper over diagnose() | unit test with temp HOME: only codex files → `["codex"]` |
| P4 zero ready | run_cli → doctor | `tests/cli_doctor.rs` style empty HOME: `ccstats daily` contains "Source diagnostics" and setup hints; no period table title for Claude |
| P5 one ready | resolve to that name | Codex-only fixture, `ccstats today` matches `ccstats today --source codex` on stdout tokens |
| P6 many ready | resolve `all` | Claude+Codex fixtures, combined daily |
| P7 breakdown | existing validate_source_breakdown | still errors without all; auto-detect two sources allows --source-breakdown |
| statusline empty | quiet | `ccstats statusline` on empty HOME exits 0, stdout is a single short line, no 29-row dump |

## 数据流

```text
parse_command → load config → resolve_source_name
  explicit? → that name
  else ready_source_names()
    0 → doctor (or quiet statusline)
    1 → dispatch that source
    N → dispatch all
```

## 备选方案

- Always default to `--source all` even with 0 or 1 sources: rejected. 0 源会走进 all-sources 空表；1 源不必付 all 的合并语义（部分命令 all 不支持）。
- Default to first detected source only, never all: rejected. 多工具用户看不到总账，回到「以为没统计到」。
- Change SDK default similarly: rejected. 嵌入方需要稳定、显式的源。

## 风险

- **Breaking default** for scripts that assumed Claude. Mitigate: changelog + config `source`. Project already declares no compatibility shims.
- **Cursor Configured** may trigger API on first `ccstats`. Same as explicit cursor today; doctor already labels Configured as credentials-present not fetched.
- **Performance**: 29× `find_files()`/`diagnose()` on every invocation. Doctor already does this; keep it. Do not parse JSONL at detect time.
- **statusline**: noisy doctor would break prompts; dedicated quiet empty path is mandatory.

## 测试计划

- [ ] Unit tests for ready-set helper (0 / 1 / N).
- [ ] CLI: empty HOME `daily`, `today`, default no-subcommand.
- [ ] CLI: Codex-only auto daily equals `--source codex`.
- [ ] CLI: Claude+Codex auto daily uses all-sources path (combined label or source-breakdown).
- [ ] CLI: `--source claude` override with Codex-only data.
- [ ] CLI: empty statusline quiet.
- [ ] Existing doctor, codex routing, public_readiness tests.
- [ ] `cargo test`

## 回滚方案

Restore `(None, None) => "claude"` and delete the ready helper. Doctor command remains.
