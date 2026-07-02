# Tech Spec

## Linked Issue

GH-41

## Product Spec

`specs/GH41/product.md`

## Codebase Context

| Area | Files | Current behavior | Why relevant |
| --- | --- | --- | --- |
| Cursor DB open | `src/source/cursor/parser.rs` | `Connection::open_with_flags` without `busy_timeout`. | Lock handling gap. |
| `cursorDiskKV` parser | `src/source/cursor/parser.rs` | Parses bubbles with ids from keys. | Potential overlap source. |
| `ItemTable` parser | `src/source/cursor/parser.rs` | Parses `aiService.generations` with `generationUUID`. | Potential overlap source. |
| Capabilities | `src/source/cursor/config.rs` | `has_projects: false`, `needs_dedup: false`. | Inconsistent with parsed `project_path`. |
| Tests | `tests/cli_integration.rs`, parser tests | Cursor fixture currently covers basic parse. | Need targeted coverage. |

## 设计方案

1. Add an investigation step that samples real Cursor DB rows or builds fixtures showing whether both tables can represent the same interaction. Do not commit private DB data.
2. If overlap is possible, introduce a Cursor-specific dedup key derived from stable id when possible, otherwise timestamp/model/token tuple with documented collision risk, and set `needs_dedup` appropriately.
3. If overlap is not possible, add comments/tests documenting the table ownership invariant and leave `needs_dedup` false.
4. Set a conservative `busy_timeout` immediately after opening a readonly connection.
5. Resolve `project_path`: either enable `has_projects` with tested project output, or stop extracting it and document Cursor projects as unsupported until a real product requirement exists.

## Product-to-Test Mapping

| Product invariant | Implementation area | Verification |
| --- | --- | --- |
| P1/P2 | Cursor table parsing/dedup | Investigation artifact plus fixture tests. |
| P3 | `open_readonly` | Unit/integration test or code-level assertion around timeout setup. |
| P4 | `CursorSource::capabilities` and parser | CLI project test or removal test. |
| P5 | parser error path | Existing errors tests plus new lock/parse regression. |

## 数据流

Cursor DB -> readonly connection with timeout -> table existence checks -> per-table parse -> optional dedup/merge -> `ParseOutput { entries, errors }` -> existing loader.

## 备选方案

- Disable one table unconditionally: rejected until evidence identifies the authoritative source.
- Mark `needs_dedup: true` without stable keys: rejected because it may hide valid records.
- Leave project_path dead: rejected because it keeps a misleading internal contract.

## 风险

- Security: read-only DB access remains read-only.
- Compatibility: project output may become newly available or dead field removed; document in changelog if user-visible.
- Performance: busy_timeout can add bounded wait under lock.
- Maintenance: investigation evidence is required before choosing dedup behavior.

## 测试计划

- [ ] Cursor parser unit tests for both table paths.
- [ ] Fixture for overlapping or documented non-overlap behavior.
- [ ] Busy timeout setup test where practical.
- [ ] CLI project/source capability test if enabling projects.
- [ ] `cargo check`
- [ ] `cargo test`

## 回滚方案

Revert Cursor parser/capability changes. No data migration.
