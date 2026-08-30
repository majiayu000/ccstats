# 本地 AI 编程工具 Token 算法审计（2026-08-31）

本文记录 ccstats 在实现数据源前使用的证据、算法比较和取舍。目标是统计本地日志中已经由客户端或 API 返回的 token，而不是根据文本重新分词，也不是把订阅额度、上下文窗口或客户端积分伪装成账单 token。

## 结论摘要

1. 任何竞品都不能单独作为事实来源。Tokscale 覆盖面最大，但当前 Qwen 实现仍读取旧会话路径，而且没有从包含缓存的 input 中扣除 cache；照搬会重复计数。
2. 官方写入端源码是字段语义的最高优先级证据。Qwen Code 的官方 usage ledger 和 Gemini CLI 的官方 recording types 可以直接证明 cache、reasoning 与 total 的包含关系。
3. ccstats 的统一 token 桶必须互不重叠。每个 parser 在边界处完成一次归一化，聚合和价格层不得再次猜测。
4. 本轮确认两个需要修复的问题：Qwen 应改读官方 usage ledger 并分离 cache；Codex 应同时扫描 `sessions` 和 `archived_sessions`。
5. 最新 ccstats 已把 Cursor 切到 usage events API，把 Grok 切到逐推理 usage ledger；这两者不再使用旧的本地 SQLite/上下文快照算法。但 Cursor endpoint 仍是未公开稳定协议，provider event cost 也不等同于最终发票。

## 证据等级

| 等级 | 含义 | 可用于实现 |
|---|---|---|
| 已验证 | 官方源码、官方技术文档或本机真实日志能证明路径与字段语义 | 是 |
| 交叉验证 | 至少两个独立维护项目实现相同算法，并有 fixture 或测试 | 是，但在文档中保留来源说明 |
| 推断 | 只有一个第三方 parser 或私有格式样本 | 仅做实验性支持 |
| 未知 | 无法确定字段单位、包含关系或稳定性 | 不实现或明确返回不可用 |

营销页面和 README 中的功能列表只能证明“声称支持”，不能证明 parser 算法正确。

## 调研快照

所有比较都固定到提交，避免以后把变化后的 `main` 当成本轮证据。

| 项目 | 提交 | 主要用途 |
|---|---|---|
| [Tokscale](https://github.com/junhoyeo/tokscale/tree/cf9f0b3453249649be8c0634bd74080c062ac3bc) | `cf9f0b3` | 广泛的数据源发现、parser 和 fixture |
| [ccusage](https://github.com/ccusage/ccusage/tree/033d25ee38a5945370fe6deae9155ad314c26ee8) | `033d25e` | Claude Code 和 OpenCode 的成熟本地统计 |
| [CodexBar](https://github.com/steipete/CodexBar/tree/efb952e0bf5f92e639959de549952ec73a88b9e9) | `efb952e` | Claude/Codex 本地 cost 与订阅 quota 的边界设计 |
| [Codeburn](https://github.com/getagentseal/codeburn/tree/6a3fa2d0ceb965baa842354dfcf8dae3eac55a1c) | `6a3fa2d` | 多供应商 per-turn parser 和工具调用信息 |
| [Tokenleak](https://github.com/ya-nsh/tokenleak/tree/80c6dc0dd0acc241cc532f9a1dbe8cc5078a7c1d) | `80c6dc0` | 多供应商注册表、fixture 和展示层 |
| [tkntracker](https://github.com/junaiddshaukat/tkntracker/tree/63cf913c02fcb337f134df37752ad50496f95f17) | `63cf913` | Qwen 官方 usage ledger 路径和广泛的轻量采集器 |
| [rust-agtop](https://github.com/collectiveai-team/rust-agtop/tree/73f8f99bd1932e956cfdad19a4afe9f154a285f6) | `73f8f99` | Gemini telemetry 优先的实时进程观测 |
| [token-history](https://github.com/keli-wen/token-history/tree/2351bb6ae4606dc0c364abe184ba9ba498ba447c) | `2351bb6` | 以现有 CLI 为输入的历史采集和可视化管线 |

官方 schema 证据：

- [Gemini CLI `TokensSummary`](https://github.com/google-gemini/gemini-cli/blob/0bd1d439751478771c45d3d0895a6a9760554bf4/packages/core/src/services/chatRecordingTypes.ts) 定义 `input`、`output`、`cached`、`thoughts`、`tool` 和 `total`；[session 文档](https://github.com/google-gemini/gemini-cli/blob/0bd1d439751478771c45d3d0895a6a9760554bf4/docs/cli/session-management.md) 确认 `~/.gemini/tmp/<project_hash>/chats/`。
- [Qwen Code `TokenUsageRecord`](https://github.com/QwenLM/qwen-code/blob/3aa1b14624789797b33bffad3d70190ce41cedce/packages/core/src/services/tokenUsageService.ts) 是当前原生 usage ledger。其 total fallback 为 `input + output + thoughts`，没有再次加 cached，证明 cached 是 input 的子集。
- [Qwen Code `Storage`](https://github.com/QwenLM/qwen-code/blob/3aa1b14624789797b33bffad3d70190ce41cedce/packages/core/src/config/storage.ts) 给出 `QWEN_RUNTIME_DIR > QWEN_HOME > ~/.qwen` 的根目录优先级。
- [Cline `SessionUsageMetadata`](https://github.com/cline/cline/blob/48d63852745460ff0fa3dfcc0457bbe2493841de/sdk/packages/core/src/types/sessions.ts) 定义 `inputTokens`、`outputTokens`、`cacheReadTokens`、`cacheWriteTokens` 和 `totalCost`。

## 跨工具算法比较

| 工具 | 系统边界 | 优点 | 需要警惕的地方 | ccstats 决策 |
|---|---|---|---|---|
| Tokscale | 本地日志、价格、提交和前端 profile | 数据源最多，per-client parser 和 fixture 完整 | 覆盖面不等于正确性；Qwen 路径和 cache 语义已落后官方源码 | 适配发现与 fixture，不复制通用壳 |
| ccusage | Claude/OpenCode 本地历史 | Claude usage、sidechain 和价格处理成熟 | 供应商范围较窄 | Claude 算法的主要交叉证据 |
| CodexBar | 本地 cost 与在线 quota 分层 | Codex 增量扫描、归档目录和账户边界很强 | Swift 实现和桌面状态层很重，不适合直接移植 | 采用 `archived_sessions` 与 source-boundary 思路 |
| Codeburn | 多供应商会话和活动分析 | per-turn、工具、命令和用户 prompt 关联丰富 | 部分数据源仍读旧路径；产品数据模型与 ccstats 不同 | 只适配有官方证据的 per-turn 规则 |
| Tokenleak | 多供应商 usage 与可视化 | registry/fixture 清楚，Gemini 归一化较完整 | Qwen 仍使用旧 chats；部分 parser 把 thoughts 合并到 output | 用于交叉测试，不作为字段语义权威 |
| tkntracker | 多供应商轻量事件采集 | 已使用 Qwen 官方 `usage/token-usage-*.jsonl` | 通用 extractor 对不同供应商使用同一相加公式，Qwen cache 会重复计数 | 采用路径，不采用通用 token 求和 |
| rust-agtop | 实时进程、session、telemetry 和 quota | Gemini telemetry 比 session fallback 更接近 API response | 面向实时观测，不保证完整历史；telemetry 可能未开启 | 后续实时模式参考，不混入本地历史 parser |
| token-history | 定时采集已有 CLI 输出 | 多机器历史、快照和图表流程清晰 | 不拥有底层 parser，正确性继承上游 | 后续增长和历史产品层参考 |

## 统一算法不变量

### 1. token 桶必须互不重叠

```text
total = fresh_input + cache_read + cache_write + visible_output + reasoning
```

- Codex：`cached_input_tokens ⊆ input_tokens`，`reasoning_output_tokens ⊆ output_tokens`，两边都要做减法。
- Gemini/Qwen：cached 是 input 的子集；thoughts 是独立于 candidates/output 的输出侧桶。
- Claude：input、output、cache read、cache creation 原生就是分离字段，不再做减法。
- Cline CLI：持久化 `inputTokens` 包含 cache read/write，因此先扣除两个 cache 桶。

### 2. 累积快照必须先转增量

- Codex 优先使用 `last_token_usage`；缺失时对相邻 `total_token_usage` 做分量差。
- 不能只比较 `total_tokens`，因为某些版本可能缺失或写零；应比较完整向量。
- 组件回退不能直接当作新调用相加；需要识别 reset 或陈旧快照。

### 3. 去重键必须对应真实所有权

- Claude 流式重复以 provider message id 去重，并优先完成记录。
- Gemini 当前 JSONL 可能重写同一 message id，应保留最后版本。
- Amp ledger 与 assistant message 是同一次调用的两个视图：先按 message id 匹配，再按 model + token 向量匹配，并且每条 ledger 只能消费一次。
- Qwen 官方 usage ledger 自带 UUID，每行已经是一条调用记录，不需要累计去重。

### 4. 本地费用必须标记来源

客户端 reported cost、API list-price estimate、订阅额度消耗不是同一个值。当前 `RawEntry` 已能单独保存 provider-reported USD cost；但 Amp credits 与本轮读取的 Cline message/task 记录没有得到可验证的一一归属和 USD 单位，因此仍写为 `None`，由价格层估算。缺少可靠价格时应显示未知或 fallback，而不是声称为实际账单。

### 5. 错误不能静默变成零使用量

空文件和没有 usage 的合法事件可以跳过；无法读取、损坏 JSONL、非法时间戳导致有效 usage 丢失时必须增加 parse error。这样 CLI 才能把“不存在使用量”和“解析失败”区分开。

## ccstats 当前 11 个来源审计

| 来源 | 数据路径与算法 | 证据状态 | 结论 |
|---|---|---|---|
| Claude Code | `projects/**/*.jsonl` assistant usage；独立 cache 桶；完成消息优先去重；含 sidechain | 已验证 + ccusage/Tokscale/Codeburn 交叉验证 | 核心 token 算法正确；继续保留 source-wide message id 去重 |
| OpenAI Codex | `token_count`；last 优先、total 差分；拆分 cached 与 reasoning | 已验证 + Tokscale/CodexBar 交叉验证 | token 算法正确；遗漏 `archived_sessions`，本轮修复 |
| Cursor | provider usage events API；token usage 与 `chargedCents` 分开保存 | 实现与 fixture 已验证，endpoint 稳定性未知 | 不再读取 SQLite；保留私有 API 变化风险，不把订阅 `$0` 事件改写成列表价 |
| Grok | `shell.turn.inference_done` 逐推理 usage；短/长上下文整请求定价；原子 ledger 保存被上游裁剪的事件 | 实现与 fixture 已验证 | 不再使用 context snapshot 代理值；本地日志缺失仍无法恢复 |
| Kimi Code | wire `usage.record` 的 turn scope；含 sub-agent | 本地 fixture + 实现交叉验证 | 当前支持的是 `.kimi-code` 产品格式；订阅模型价格仍为 fallback |
| Gemini CLI | chat JSON/JSONL 与 headless stats；input/cache 条件归一化；tool 加入输入侧 | 已验证 + Tokscale/Codeburn/Tokenleak 交叉验证 | 当前算法正确；保留 per-message，而不是把整个 session 折成一条 |
| Amp | usage ledger 与 assistant usage 一对一合并 | 交叉验证 | token 算法可用；credits 的 USD 单位和调用归属未验证，暂不导入 |
| Qwen Code | 当前应读取官方 usage ledger；input 必须减 cached；thoughts 独立 | 已验证 | 现实现错误：旧路径且 cache 重复，本轮替换 |
| Cline | CLI assistant metrics + VS Code task `api_req_started` | CLI 已验证，扩展格式交叉验证 | token 算法正确；当前读取记录未证明 reported cost 的稳定归属，暂不导入 |
| Roo Code | VS Code task `api_req_started`，读取最后 environment model | 推断/交叉验证 | 保持支持并在格式变化时用真实 fixture 更新 |
| Kilo Code | 与 Roo/Cline extension task 格式相同 | 推断/交叉验证 | 保持支持；不要与 Kilo gateway 的在线 quota 混淆 |

## 本轮采用、适配、拒绝决策

### 采用

- Qwen 官方 usage ledger schema、目录优先级和 UUID 去重键。
- Codex 同时扫描 active 与 archived session roots。
- 官方字段语义优先于任何第三方 parser。

### 适配

- Tokscale 的 Amp 双视图合并顺序。
- Tokscale、Codeburn、Tokenleak 的 Gemini cache 归一化测试向量。
- CodexBar 的“本地 cost”和“在线 quota”分层概念，但不引入其桌面账户管理层。

### 拒绝

- 直接复制 Tokscale parser：会把已确认的 Qwen 旧路径和重复计数带进来。
- 使用一个通用字段别名表处理所有供应商：相同名称在不同 API 中可能是包含值或独立值。
- 根据 prompt/response 文本重新 tokenizer：模型 tokenizer、隐藏 system prompt、tool schema 和服务端缓存都不可完整重建。
- 在没有验证 USD 单位和调用归属时导入 credits/reported cost：即使模型支持 provenance，也会把不同口径的值混在一起。

## 后续数据源顺序

新增来源必须先取得官方 schema 或两个独立 fixture，再进入实现。建议顺序：

1. OpenCode：官方 SQLite/JSON schema，且 ccusage、Tokscale、Codeburn、Tokenleak 都有独立实现，可充分交叉验证。
2. GitHub Copilot：优先 OTEL 本地事件；必须区分 premium request quota 与模型 token。
3. Pi：JSONL message usage 格式简单，多个项目已有 fixture。
4. Droid、Kiro：先确认本地日志是否包含真实 token，而不是订阅额度或客户端 credits。
5. Goose、Zed、Warp：SQLite/缓存格式需要版本化 fixture，不能只根据列名猜测。

每一个来源独立成 PR，包含官方证据链接、最小真实格式 fixture、RED/GREEN 测试记录、数据路径和已知限制。这样覆盖面可以持续增长，但不会用错误数字换取“支持数量”。
