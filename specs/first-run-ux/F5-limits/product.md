# Product Spec — F5 `ccstats limits`

## Linked Issue

https://github.com/majiayu000/ccstats/issues/168

## 用户问题

订阅用户的 P0 问题是「会不会超额度」，不是 LiteLLM 美元。今天 `ccstats quota` 只服务 Codex，却是顶层命令，看起来像通用额度。Claude 只有 `blocks`（活动推断的 5 小时窗，文档已写明不是官方 reset）。没有一条命令同时回答「Codex 这周」和「Claude 这个窗口」。

## 目标

- 新增 `ccstats limits`：在一条命令里展示本机 **能诚实给出** 的额度/窗口。
- Codex：复用现有 weekly quota 报告（官方本地 snapshot）。
- Claude：复用现有 activity-driven `blocks` 的 **当前活动窗**（若有），标明 estimated、not official billing reset。
- `ccstats quota` 保留，行为不变，帮助文本标明它是 Codex 专用别名。
- 未检测到的源整段省略，不要伪造余量。

## 非目标

- 不接入 Anthropic 官方 5h/7d 余量 API（那是后续；本轮只用本地已有信号）。
- 不做 Cursor 计划余量（Cursor 源没有可靠本地 quota；禁止猜）。
- 不改 blocks 窗口算法。
- 不在本 PR 做菜单栏。

## Behavior Invariants

1. `ccstats limits` 忽略 `--source` 对「只跑一个源」的强制，**除非** `--source` 是 `codex` 或 `claude`（或 all/省略）。
   - 省略 / `all`：输出所有能产生的段。
   - `--source codex`：只 Codex 段。
   - `--source claude`：只 Claude 段。
   - `--source cursor` 或其他：exit 1，说明 limits 只支持 claude、codex、all。
2. Codex 段：与 `ccstats quota` 相同的表/JSON 字段；失败时该段显示错误原因，不阻断 Claude 段。
3. Claude 段：若无活动 block，写 `No active estimated 5-hour window`，不要 0% 假装有额度。
4. 每个 Claude 窗口必须有一句：`Estimated from local logs; not an official Anthropic billing reset.`
5. JSON：`{ "codex": {...}|null, "claude_blocks": {...}|null, "notes": [...] }` 或等价可版本化结构；null 表示源未就绪或不可用，不是零额度。
6. CSV：稳定列，缺失段留空，不写 0。
7. Table：两段标题 `Codex weekly quota` / `Claude estimated session window`。
8. `ccstats quota --help` 写明 Codex-only；`limits --help` 写明组合视图。

## 验收标准

- [ ] 只有 Codex fixture：`limits` 含 Codex 数字，Claude 段为无窗口或不输出空进度条。
- [ ] 只有 Claude 活动 fixture：`limits` 含 estimated 声明；无 Codex 时 Codex 段为不可用而不是 0%。
- [ ] `ccstats quota` stdout 与改前测试一致。
- [ ] `--source cursor` 的 limits 非 0 成功码。
- [ ] JSON 不含把 missing 写成 used_pct=0。

## 边界情况

- 两源都 missing：table 解释跑 doctor；exit 0。
- `--json` 与 jq 过滤继续可用。

## 发布说明

新增 `limits` 组合额度视图。`quota` 仍为 Codex 周额度。Claude 窗口仍是本地估算。
