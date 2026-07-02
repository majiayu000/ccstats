# Tech Spec

## Linked Issue

GH-42

## Product Spec

`specs/GH42/product.md`

## Codebase Context

| Area | Files | Current behavior | Why relevant |
| --- | --- | --- | --- |
| Claude file discovery | `src/source/claude/parser.rs` | Builds `home.join(".claude").join("projects")`. | Primary change surface. |
| Claude source | `src/source/claude/config.rs` | Calls `find_claude_files()`. | Should consume shared helper. |
| Other env overrides | `src/source/codex/parser.rs`, `cursor/parser.rs`, `grok/parser.rs` | Support `CODEX_HOME`, `CURSOR_HOME`, `GROK_HOME`. | Consistency reference. |
| Docs | `README.md`, `docs/algorithm/authoritative-token-accounting.md` | README lists Cursor/Grok env override but not Claude/Codex consistently. | Documentation gap. |

## 设计方案

1. Add `CLAUDE_CONFIG_DIR` constant and helper returning the Claude config root:
   - env var when present
   - otherwise `$HOME/.claude`
2. Build project glob from `claude_config_root().join("projects")`.
3. Keep path handling non-panicking; invalid/missing dirs simply yield no files, matching current behavior.
4. Add integration test with temp `CLAUDE_CONFIG_DIR` and no `HOME/.claude` data.
5. Update README/docs env override matrix.
6. If GH45 lands first, wire tool-call file discovery through the same helper.

## Product-to-Test Mapping

| Product invariant | Implementation area | Verification |
| --- | --- | --- |
| P1 | `find_claude_files` | Integration test with `CLAUDE_CONFIG_DIR`. |
| P2 | default path | Existing Claude tests. |
| P3 | missing path | Unit/integration no-data test. |
| P4 | docs | README/docs diff review. |
| P5 | tool discovery | GH45 compatibility test if applicable. |

## 数据流

Environment -> Claude config root helper -> `projects/**/*.jsonl` glob -> existing parser/loader. No persistence changes.

## 备选方案

- Add `CLAUDE_HOME`: rejected because Claude Code already defines `CLAUDE_CONFIG_DIR`.
- Add CLI flag: rejected as unnecessary until a broader source path override design exists.

## 风险

- Security: env var only influences local file discovery; no command execution.
- Compatibility: default path unchanged.
- Performance: unchanged.
- Maintenance: central helper prevents future tool-call path divergence.

## 测试计划

- [ ] Integration test for `CLAUDE_CONFIG_DIR`.
- [ ] Existing Claude default path tests.
- [ ] Docs check/review.
- [ ] `cargo check`
- [ ] `cargo test`

## 回滚方案

Revert helper, docs, and tests. Default `~/.claude` behavior remains available.
