# Tech Spec

## Linked Issue

GH-47

## Product Spec

`specs/GH47/product.md`

## Codebase Context

| Area | Files | Current behavior | Why relevant |
| --- | --- | --- | --- |
| Loader | `src/source/loader.rs` | 845 lines, includes production loader and many tests. | Over hard ceiling. |
| Table output | `src/output/table.rs` | 821 lines. | Over hard ceiling. |
| CSV output | `src/output/csv.rs` | 766 lines. | Near hard ceiling. |
| CLI integration tests | `tests/cli_integration.rs` | 1964 lines. | Needs source/feature split. |
| Naming | `src/sdk.rs`, `src/output/table.rs`, `src/output/mod.rs`, `src/app.rs` | SDK and internal output both use `SummaryOptions`. | Confusing type identity. |
| Too-many-lines allows | `src/source/codex/parser.rs`, `src/lib.rs` | Issue points to parser and CLI dispatch allows. | Should be extracted into helpers. |

## 设计方案

Implement as multiple PR slices rather than one large refactor:

1. **Naming slice**: Rename internal output `SummaryOptions` to `PeriodSummaryFooter` or equivalent; update `output/mod.rs` re-export and app call site.
2. **Loader test split slice**: Move loader unit tests into `src/source/loader/tests.rs` or module-local submodule to bring `loader.rs` under 800 without behavior changes.
3. **Output split slice**: Split table/csv renderers by data shape or helper modules while preserving function exports.
4. **Integration test split slice**: Move `tests/cli_integration.rs` into focused files such as `cli_codex.rs`, `cli_claude.rs`, `cli_grok_cursor.rs`, `cli_budget_currency.rs`, sharing helper module.
5. **Complex function extraction slice**: Extract `parse_codex_file_with_debug` steps and `run_cli` setup/dispatch helpers enough to remove the issue-named clippy allows.

Each slice must run `cargo test`; large output splits should also run targeted CLI tests for affected commands.

## Product-to-Test Mapping

| Product invariant | Implementation area | Verification |
| --- | --- | --- |
| P1 | output split | Existing CLI output tests. |
| P2 | naming slice | Compile + SDK tests. |
| P3 | integration split | Full test count/coverage parity and `cargo test`. |
| P4 | parser/lib extraction | Parser unit tests and CLI command tests. |
| P5 | PR slicing | PR bodies list owned files and verification. |

## 数据流

No intended runtime data-flow change. This is code organization: exported functions keep their current call contracts while implementation moves into smaller modules/helpers.

## 备选方案

- One giant refactor PR: rejected due review risk.
- Add more `allow` attributes: rejected by project guidance.
- Split only tests and ignore production files: rejected because table.rs remains above hard ceiling.

## 风险

- Security: no auth/secrets surface.
- Compatibility: no intended behavior change; output regressions are main risk.
- Performance: unchanged.
- Maintenance: reduced file size and clearer names.

## 测试计划

- [ ] `cargo check`
- [ ] `cargo test`
- [ ] Focused CLI tests after each output/test split.
- [ ] `wc -l` evidence for targeted files.
- [ ] `rg "allow\\(clippy::too_many_lines\\)"` evidence for issue-named functions.

## 回滚方案

Each slice should be independently revertible. Because behavior is unchanged, rollback is standard git revert per PR.
