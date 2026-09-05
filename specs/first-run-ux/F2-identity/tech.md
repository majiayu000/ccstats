# Tech Spec — F2 Public identity and first-run docs

## Linked Issue

https://github.com/majiayu000/ccstats/issues/165

## Product Spec

`specs/first-run-ux/F2-identity/product.md`

## Codebase Context

| Area | Files | Current behavior | Why relevant |
| --- | --- | --- | --- |
| README | `README.md` on origin/main | 30s start is doctor + daily --source all; then long desktop build; then SDK | Reorder, do not rewrite accounting docs. |
| Crate metadata | `Cargo.toml` description | "Fast, local-first token and cost analytics for AI coding agents" | Already closer to target; GitHub description is stale. |
| Branding | `docs/branding/` | Card image | Keep the card near the top. |
| Tests | `tests/public_readiness.rs` | Asserts doctor copy exists in source | Do not break; add README assertion only if the file already checks README. |

## 设计方案

1. Rewrite README sections **above Installation**:
   - One-paragraph product sentence (CLI + local-first + coding agents; desktop is a sentence, not a tutorial).
   - 30-second start: `brew install majiayu000/tap/ccstats` then `ccstats`.
   - One line: no args uses detected sources; none → doctor.
   - Core source table (7 rows) with start commands using `--source` canonical form; keep `ccstats codex today` as an alias example in a footnote or second column.
2. Move "Desktop application development" below Installation or to `docs/RELEASING.md` / `desktop/README.md`. Keep download links for DMG/MSI/AppImage in Installation.
3. Move Rust SDK examples below Usage or keep a short pointer to docs.rs.
4. After merge (or in PR body checklist): 

   ```bash
   gh repo edit majiayu000/ccstats \
     --description "Local-first CLI (and desktop) for token and cost analytics across AI coding agents" \
     --homepage "https://github.com/majiayu000/ccstats" \
     --add-topic cursor --add-topic grok --add-topic desktop
   ```

5. Do not change `src/**`.

## Product-to-Test Mapping

| Product invariant | Verification |
| --- | --- |
| First screen has no tauri dev | grep README from start through `## Installation` |
| Core table before full registry | heading order |
| GitHub identity | manual `gh repo view` on merge |

## 备选方案

- Point homepage at crates.io: rejected; crates.io 不是产品首页。
- Write a new GitHub Pages site this PR: rejected as out of scope.

## 风险

- F1 未合并时 README 写「无参数自动检测」会与已发布二进制不一致。缓解：F2 以 F1 为 base，或 F2 明确 “as of this PR / vNext”。
- topics 过多被 GitHub 截断：只加缺失的高价值词。

## 测试计划

- [ ] 人工阅读 README 第一屏。
- [ ] 不运行 `cargo test` 需求除非 public_readiness 读 README。
- [ ] 合并后 `gh repo view` 检查 description。

## 回滚方案

Revert README；`gh repo edit` 改回旧 description。
