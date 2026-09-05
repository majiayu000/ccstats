# Product Spec — F2 Public identity and first-run docs

## Linked Issue

https://github.com/majiayu000/ccstats/issues/165

## 用户问题

GitHub 仓库 description 仍是 “Fast CLI for Claude Code and OpenAI Codex”。homepage 指向 crates.io。README 把桌面从源码编译、29 源百科、Grok 成本论文放在新用户第一屏。访客无法在 30 秒内回答：这是什么、怎么装、敲哪条命令。

## 目标

- GitHub description、topics、homepage 与 v0.5.2+ 产品一致。
- README 第一屏只保留：一句话定位、一条安装、一条默认命令、doctor 说明、桌面下载链接（不是编译指南）。
- 对外主打 Core 源：Claude、Codex、Cursor、Gemini、Grok、Kimi、OpenCode。其余源留在完整源表，不进第一屏。
- 文档中的默认命令与 F1 对齐：无参数即本机已检测源；没有数据则 doctor。

## 非目标

- 不新建独立营销站（本轮 homepage 改为 GitHub repo 即可，不要 crates.io）。
- 不改 CLI 行为（若 F1 尚未合并，README 仍写目标语义，并在 PR 中注明依赖；合并顺序：F1 先于或同批于 F2）。
- 不把 ARCHITECTURE / PRIVACY / 源表删掉，只下沉到后部链接。
- 不在本 PR 改桌面 UI。

## Behavior Invariants

1. README 开头 30 秒路径不超过：brew 安装、`ccstats`（或 `ccstats doctor` 若需强调只读检测）、一句话解释自动检测。
2. 「Desktop application development」整节移到文档后部或 `desktop/README` / `docs/RELEASING.md` 链接，不得出现在 Highlights 之前。
3. GitHub description 必须提到 local-first、多 coding agent、CLI（可提 desktop，不要只写双源）。
4. topics 增加实际可搜词：至少 `cursor`、`codex`；去掉会误导成「只有 chatgpt」的过时观感不是强制删 `chatgpt`，但 description 不得再写「仅 Claude 和 Codex」。
5. 完整 29 源表仍在 README 后部，标题明确是完整注册表。

## 验收标准

- [ ] `gh repo view majiayu000/ccstats --json description,homepageUrl,repositoryTopics` 与本 spec 一致（description/homepage/topics 由维护者在合并时或 PR 里用 `gh repo edit` 更新）。
- [ ] README 在第一个 `## Installation` 之前不出现 `npm run tauri`、Rust 桌面前置、SDK 长示例。
- [ ] Quick Start 不再默认只展示 Claude 命令作为唯一入口。
- [ ] Core 源表（≤8 行）出现在完整源表之前。

## 发布说明

文档与商店元数据同步；无运行时变化。
