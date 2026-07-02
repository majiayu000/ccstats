# Product Spec

## Linked Issue

GH-46

## 用户问题

ccstats 支持 `config.toml`、多个配置搜索路径和 `CODEX_HOME`，但 README/docs 没有完整说明。更严重的是，存在但非法的 config 只 warning 后回落默认值；quiet/statusline 场景下用户可能完全不知道 `strict_pricing`、`offline`、`currency` 等设置被忽略。

## 目标

- 文档完整列出 config 文件名、搜索路径、可用键和 source env overrides。
- `CODEX_HOME` 与其他 source env vars 一样被记录。
- 存在但无法解析的 config 不得静默回落默认值。
- quiet/statusline 路径也不能隐藏会影响输出正确性的 config parse failure。

## 非目标

- 不新增配置键。
- 不改变合法 config 的解析语义。
- 不引入交互式 config 初始化。

## Behavior Invariants

1. 用户可以从 README/docs 找到 config 文件位置和所有支持键。
2. 用户可以从 README/docs 找到 `CODEX_HOME`、`CURSOR_HOME`、`GROK_HOME`，以及若 GH42 已合并则 `CLAUDE_CONFIG_DIR`。
3. Config 文件不存在时，默认配置行为保持不变。
4. Config 文件存在但 TOML 无法解析时，命令返回明确错误或显式错误状态，不得继续使用默认值生成误导输出。
5. quiet 模式不抑制 config parse failure。

## 验收标准

- [ ] README/docs 包含 config 示例和键表。
- [ ] README/docs 包含 source env override 矩阵。
- [ ] 非法 config fixture 让 CLI 返回失败并显示明确错误。
- [ ] 缺失 config 仍正常使用 default。
- [ ] statusline/quiet 路径覆盖非法 config。

## 边界情况

- config 文件不存在。
- config 文件存在但 TOML 语法错误。
- config 文件存在但字段类型错误。
- 多个搜索路径中较高优先级 config 非法。

## 发布说明

非法 config 从 warning+default 变为明确错误，避免静默忽略用户配置。
