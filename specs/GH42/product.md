# Product Spec

## Linked Issue

GH-42

## 用户问题

Claude Code 支持通过 `CLAUDE_CONFIG_DIR` 迁移配置目录，但 ccstats Claude 源只扫描 `~/.claude/projects`。迁移过配置目录的用户会看到 Claude 用量为 0，且没有 CLI 或环境变量方式指向真实目录。

## 目标

- Claude 源优先尊重 `CLAUDE_CONFIG_DIR`。
- 默认行为仍扫描 `~/.claude/projects`。
- 文档说明 Claude/Codex/Cursor/Grok 的环境变量覆盖方式。

## 非目标

- 不新增 ccstats 自有 Claude 专用变量。
- 不改变 Claude JSONL 解析、去重或过滤规则。
- 不改变其他源 env override 行为。

## Behavior Invariants

1. `CLAUDE_CONFIG_DIR=/path/to/claude-config` 时，Claude 源扫描 `/path/to/claude-config/projects/**/*.jsonl`。
2. 未设置 `CLAUDE_CONFIG_DIR` 时，行为保持 `~/.claude/projects/**/*.jsonl`。
3. 空字符串或无效路径不应 panic；应按既有“无文件”行为返回空结果。
4. README/docs 记录 `CLAUDE_CONFIG_DIR` 与现有 `CODEX_HOME`、`CURSOR_HOME`、`GROK_HOME`。
5. tools/tool-call 路径若依赖 Claude 文件发现，应复用同一配置目录逻辑。

## 验收标准

- [ ] 集成测试证明 `CLAUDE_CONFIG_DIR` 下的 Claude JSONL 被读取。
- [ ] 默认 `HOME/.claude/projects` 测试继续通过。
- [ ] README 至少记录 `CLAUDE_CONFIG_DIR` 和 `CODEX_HOME`。
- [ ] 若 GH45 先合并，tool-call discovery 同样尊重该 helper。

## 边界情况

- env var 指向不存在目录。
- env var 指向相对路径。
- env var 路径末尾带斜杠。
- `HOME` 和 `CLAUDE_CONFIG_DIR` 同时设置。

## 发布说明

新增 Claude 配置目录环境变量支持，迁移配置目录的用户可直接统计 Claude 数据。
