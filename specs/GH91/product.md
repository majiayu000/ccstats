# Product Spec

## Linked Issue

GH-91

## 用户问题

Kimi Code CLI 用户无法使用 ccstats 查看自己的 token 用量与估算费用。ccstats 已覆盖 Claude Code、OpenAI Codex、Cursor、Grok，唯独缺少 Kimi Code 数据源；随着 Kimi Code 用户增长，这一缺口使跨工具用量对比（`--source all`）也不完整。

## 目标

- 新增 Kimi Code 数据源：解析 `~/.kimi-code/sessions/` 下的 wire 日志，输出与其他数据源一致的 daily/weekly/monthly/today/session/project/statusline/top 视图。
- 统计每轮真实 token 用量（input/output/cache-read/cache-creation），包含子 agent 用量。
- `kimi-code/*` 订阅模型无公开单价时给出明确标记的 fallback 费用估算。

## 非目标

- Kimi 5 小时计费块、工具调用统计、endpoints 视图（Kimi Code 本地无对应数据）。
- 改动其他数据源的解析、聚合或定价行为。
- Kimi 服务端配额/订阅余量查询（无本地数据，需官方 API）。

## Behavior Invariants

1. `ccstats kimi`（无子命令）默认展示按天聚合视图；`daily/weekly/monthly/today/session/project/statusline` 子命令与全局 `--source kimi`、别名 `km` 行为一致。
2. 每条 `usageScope == "turn"` 的 `usage.record` 计为一次调用：`inputOther → input_tokens`、`output → output_tokens`、`inputCacheRead → cache_read`、`inputCacheCreation → cache_creation`。
3. 同一会话目录下 `agents/main` 与 `agents/agent-N` 的用量聚合到同一 session，不重复、不遗漏。
4. 项目路径优先取 `session_index.jsonl` 的 `workDir`；索引缺失时回退 `workDirKey` slug；再缺失为空字符串。
5. 非 `turn` scope 的用量记录、全零用量记录、非 `usage.record` 类型的行（即使文本中包含该标记）均不计入、不计错。
6. 无效 JSON 行、缺失/非法时间戳的 `usage.record` 计入解析错误数，不产生条目。
7. 负数 token 计数按 0 处理。
8. 费用按 LiteLLM 解析 → fallback 的顺序定价；fallback 使用 Moonshot 官方 `kimi-k2.6` 参考费率，结构化输出标记 `pricing_source: fallback`；`--strict-pricing` 下未知模型费用显示 N/A。
9. `KIMI_CODE_HOME` 环境变量覆盖数据根目录；未设置时使用 `~/.kimi-code`。
10. `ccstats kimi daily --source <非 kimi>` 报冲突错误并退出非零。
11. 其他数据源的解析、聚合、定价与输出行为保持不变。

## 验收标准

- [x] `ccstats kimi daily` 输出按天聚合的 token 与费用，数据来自真实 wire 日志
- [x] 子 agent 用量计入同一会话
- [x] `--source kimi` / 别名 `km` 生效，`ccstats sources` 列表包含 kimi
- [x] 费用标记为 fallback 估算；`--strict-pricing` 下显示 N/A
- [x] `KIMI_CODE_HOME` 覆盖生效
- [x] 单元测试与 CLI 集成测试覆盖解析、聚合、冲突校验
- [x] cargo fmt / clippy -D warnings / cargo test / check_workflow 全部通过

## 边界情况

- wire 日志包含大段对话负载：按行预过滤 `usage.record` 标记，避免全量 JSON 解析。
- 对话文本中提及 `usage.record` 字样的行不误判为用量记录（校验 `type` 字段）。
- 会话进行中 wire 文件被追加写入：逐行解析，容忍末尾不完整行并计入解析错误。
- `session_index.jsonl` 缺失或损坏：回退 `workDirKey` slug，不影响 token 统计。

## 发布说明

新增功能，无迁移要求。README 增加 Kimi Code 快速上手与限制说明；费用为 fallback 估算（Kimi Code 订阅模型无公开单价），需要精确计费口径的用户可使用 `--strict-pricing`。
