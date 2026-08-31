# 本地 AI 编程工具 Token 算法审计（2026-08-31）

本文记录 ccstats 在实现数据源前使用的证据、算法比较和取舍。目标是统计本地日志中已经由客户端或 API 返回的 token，而不是根据文本重新分词，也不是把订阅额度、上下文窗口或客户端积分伪装成账单 token。

## 结论摘要

1. 任何竞品都不能单独作为事实来源。Tokscale 覆盖面最大，但当前 Qwen 实现仍读取旧会话路径，而且没有从包含缓存的 input 中扣除 cache；照搬会重复计数。
2. 官方写入端源码是字段语义的最高优先级证据。Qwen Code 的官方 usage ledger 和 Gemini CLI 的官方 recording types 可以直接证明 cache、reasoning 与 total 的包含关系。
3. ccstats 的统一 token 桶必须互不重叠。每个 parser 在边界处完成一次归一化，聚合和价格层不得再次猜测。
4. 本轮确认两个需要修复的问题：Qwen 应改读官方 usage ledger 并分离 cache；Codex 应同时扫描 `sessions` 和 `archived_sessions`。
5. 最新 ccstats 已把 Cursor 切到 usage events API，把 Grok 切到逐推理 usage ledger；这两者不再使用旧的本地 SQLite/上下文快照算法。但 Cursor endpoint 仍是未公开稳定协议，provider event cost 也不等同于最终发票。
6. Tokscale 在固定提交中登记 52 个 client，ccstats 本批实现前登记 11 个 source；41 的原始差额不等于 41 个独立算法。OpenCode、MiMo Code、Kilo CLI 共享一个 SQLite 消息族，Pi、GJC、Senpi、Kimchi、Prime Agent、Oh My Pi 共享一个 JSONL 消息族，另有在线 quota、headless capture 和社交提交等不同系统边界。
7. OpenCode 与 Pi 已取得当前官方写入端源码证据，可以进入实现。Pi 官方还会给 compaction / branch summary 保存独立 usage，Tokscale 当前普通 Pi parser 未统计这两类真实调用；ccstats 应补上而不是继承这个遗漏。
8. Copilot CLI 官方将一次 LLM 请求定义为一条 `chat` span；Tokscale 只从 input 扣 cache read、也不从 output 扣 reasoning，会重复计算 cache creation 与 reasoning。ccstats 同时拆除两个 inclusive 子集，并忽略 `invoke_agent` 汇总。
9. Goose 最新权威数据是 schema v15+ `usage_ledger`。Tokscale 仍读取 session 累计快照、把整段用量记到 session 创建日，并用 total 差额猜 reasoning；ccstats 改读逐调用 ledger，保留 cache、项目、模型、时间和 cost provenance。
10. MiMo Code、Kilo CLI、Senpi、Kimchi、GJC、Prime Agent、Oh My Pi 已逐一核对当前写入端并实现；它们共享基础格式，但 fork 复制、patch、child attribution、task rollup 和目录语义不同，不能作为普通 Pi 记录直接相加。
11. Batch 5 新增 OpenClaw、Xum 与 Hermes Agent，总 source 数达到 25。三项均以当前官方写入端为准：OpenClaw 同时读取 JSONL/zstd 与当前 SQLite store，只采用 provider-billed cost；Xum 处理 `rolledUpFrom` child 双计并锁定完整五桶成本；Hermes 保留 task/billing 维度、补 session residual 并从 output 拆除 reasoning。Tokscale 对这些边界均不完整。
12. Batch 6 新增 Reasonix 与 Vercel Fx，总 source 数达到 27。Reasonix 读取按日 provider-call stats，保留完整 USD occurrence-time valuation；Fx 以 profile generation ledger 为主，只合并官方 recovery registry 有界指向、canonical event-log/commit-watermark 可重放且 sidecar 自身完整有效的 publication backlog；projection 不一致会保留恢复提示并标记不完整，不泛扫普通 session snapshot。Tokscale 的 Fx 把 inclusive input/output 与 cache/reasoning 再次相加，会把示例总数从 155 错算为 190。

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
| [OpenCode](https://github.com/anomalyco/opencode/tree/10765ff2a9da8c3b88e4de873aa383a49c318912) | `10765ff` | 当前 SQLite 路径、v1/v2 message schema、cache 与 cost 归一化 |
| [Pi](https://github.com/earendil-works/pi/tree/853a80d26c90a14c1886f0ebb8ffaae133ca2185) | `853a80d` | 当前 JSONL session schema、usage/cost 语义、目录环境变量 |
| [GitHub Copilot CLI](https://github.com/github/copilot-cli/tree/be82101e70f0253b57519bebb9cc9d0f6dfb2ed2) | `be82101` | 当前 OTel 版本与字段变更记录；公开仓库不含运行时源码 |
| [Goose](https://github.com/aaif-goose/goose/tree/8ae4e4ba02836529790f47109b8785e8b42843a7) | `8ae4e4b` | 当前 SQLite schema、usage ledger、cache/cost 语义与路径 |
| [MiMo Code](https://github.com/XiaomiMiMo/MiMo-Code/tree/be5af909aeccdeb1b707ac4c5f9214e6fe4b8d2b) | `be5af90` | OpenCode fork 的 DB 路径、message/session 时间、fork copy 与 cost 写入 |
| [Kilo CLI](https://github.com/Kilo-Org/kilocode/tree/bbf6a278d791842ababfbc8d58f902cb0f6b9bf4) | `bbf6a27` | Kilo DB 路径、legacy/current 双表与 fork copy 行为 |
| [Senpi](https://github.com/code-yeongyu/senpi/tree/7ac3cf302950a4b258421748f944ed1281007c4b) | `7ac3cf3` | Pi v3 session 目录、四类 usage carrier 与 branch copy |
| [Kimchi](https://github.com/getkimchi/kimchi/tree/eacd20f15c2e2ed2b8d24fc52f08fa7638b8f759) | `eacd20f` | 固定 session 目录、child transcript 与 parent rollup 关系 |
| [GJC](https://github.com/Yeachan-Heo/gajae-code/tree/7d23ed3d9e8cb6e5062ba2840462d59fe18eb784) | `7d23ed3` | 当前 v5 stats parser，证明其已偏离普通 Pi v3 |
| [Prime Agent](https://github.com/PrimeIntellect-ai/prime-agent/tree/c382f09856d4a8c8d2b765179657047d58691f25) | `c382f09` | parent aggregate、child transcript 与 fork aggregate 的 RLM 语义 |
| [Oh My Pi](https://github.com/can1357/oh-my-pi/tree/969062200754ea02cfac922e5ebb8c608c079e15) | `9690622` | profile/XDG 路径、child transcript、parent task rollup 与 orchestration usage |
| [OpenClaw](https://github.com/openclaw/openclaw/tree/2181ae7ba2e836451e90068ec1a41e31bef87f93) | `2181ae7` | v3 transcript、usage/cost provenance、state root 与 fork entry identity |
| [Xum](https://github.com/coder/mux/tree/ad7f569ef21cc293ecbb71f8b718e30d11da4b27) | `ad7f569` | 当前产品名/root、五桶累计 usage 与 child roll-up ledger |
| [Hermes Agent](https://github.com/NousResearch/hermes-agent/tree/4f22543509d1b91dc45bcb369447126c5eb14fb7) | `4f22543` | 当前 SQLite 复合主键、task/API call、reasoning 与 cost status |
| [Reasonix](https://github.com/futureflowtech/reasonix/tree/e9e4ca68ba6d1f82679e2f2877bdbbee89e1c19d) | `e9e4ca6` | 当前 stats ledger、cache/reasoning 包含关系与 occurrence-time cost quote |
| [Vercel Fx](https://github.com/vercel-labs/fx/tree/2ed0f44c5913dd61d35cba8495838a9f1542ade1) | `2ed0f44` | profile generation ledger、五桶包含关系、generation ID 去重、canonical session 与 sidecar recovery 边界 |

官方 schema 证据：

- [Gemini CLI `TokensSummary`](https://github.com/google-gemini/gemini-cli/blob/0bd1d439751478771c45d3d0895a6a9760554bf4/packages/core/src/services/chatRecordingTypes.ts) 定义 `input`、`output`、`cached`、`thoughts`、`tool` 和 `total`；[session 文档](https://github.com/google-gemini/gemini-cli/blob/0bd1d439751478771c45d3d0895a6a9760554bf4/docs/cli/session-management.md) 确认 `~/.gemini/tmp/<project_hash>/chats/`。
- [Qwen Code `TokenUsageRecord`](https://github.com/QwenLM/qwen-code/blob/3aa1b14624789797b33bffad3d70190ce41cedce/packages/core/src/services/tokenUsageService.ts) 是当前原生 usage ledger。其 total fallback 为 `input + output + thoughts`，没有再次加 cached，证明 cached 是 input 的子集。
- [Qwen Code `Storage`](https://github.com/QwenLM/qwen-code/blob/3aa1b14624789797b33bffad3d70190ce41cedce/packages/core/src/config/storage.ts) 给出 `QWEN_RUNTIME_DIR > QWEN_HOME > ~/.qwen` 的根目录优先级。
- [Cline `SessionUsageMetadata`](https://github.com/cline/cline/blob/48d63852745460ff0fa3dfcc0457bbe2493841de/sdk/packages/core/src/types/sessions.ts) 定义 `inputTokens`、`outputTokens`、`cacheReadTokens`、`cacheWriteTokens` 和 `totalCost`。
- [OpenCode `SessionMessage.Assistant`](https://github.com/anomalyco/opencode/blob/10765ff2a9da8c3b88e4de873aa383a49c318912/packages/schema/src/session-message.ts) 定义 nested `model`、独立 token 桶、可选 recorded cost 与毫秒时间；[`session/sql.ts`](https://github.com/anomalyco/opencode/blob/10765ff2a9da8c3b88e4de873aa383a49c318912/packages/core/src/session/sql.ts) 证明当前数据库同时存在 `message` 与 `session_message` 两代消息表。
- [OpenCode `getUsage`](https://github.com/anomalyco/opencode/blob/10765ff2a9da8c3b88e4de873aa383a49c318912/packages/opencode/src/session/session.ts) 会先从 AI SDK inclusive input 中扣除 cache read/write，并把 reasoning 从 output 中扣除；持久化后的五个桶已经互不重叠，consumer 不应再次做减法。
- [Pi `Usage`](https://github.com/earendil-works/pi/blob/853a80d26c90a14c1886f0ebb8ffaae133ca2185/packages/ai/src/types.ts) 明确 `reasoning` 是 `output` 的子集，而 `totalTokens` 等于 input、output、cache read、cache write 之和；因此 reasoning 只能作为说明，不能再加入 ccstats 的 additive reasoning 桶。
- [Pi session schema](https://github.com/earendil-works/pi/blob/853a80d26c90a14c1886f0ebb8ffaae133ca2185/packages/coding-agent/src/core/session-manager.ts) 证明 assistant message、compaction 与 branch summary 都可能携带 usage；后两者是独立 LLM 调用，不应静默丢弃。
- [Copilot CLI OTel 官方文档](https://docs.github.com/en/copilot/reference/copilot-cli-reference/cli-command-reference#opentelemetry-monitoring) 定义 file exporter、每请求一条 `chat` span、累计 `invoke_agent` span，以及当前 token/cache/cost 属性；[OpenTelemetry GenAI registry](https://opentelemetry.io/docs/specs/semconv/registry/attributes/gen-ai/) 定义 cache/reasoning detail bucket 的语义。
- [Goose `Usage`](https://github.com/aaif-goose/goose/blob/8ae4e4ba02836529790f47109b8785e8b42843a7/crates/goose-provider-types/src/conversation/token_usage.rs) 明确 input 包含 cache read/write；[`session_manager`](https://github.com/aaif-goose/goose/blob/8ae4e4ba02836529790f47109b8785e8b42843a7/crates/goose/src/session/session_manager.rs) 定义 schema v16 `usage_ledger`、逐调用 model/time/cache/cost 与 session project。
- [MiMo Code session schema](https://github.com/XiaomiMiMo/MiMo-Code/blob/be5af909aeccdeb1b707ac4c5f9214e6fe4b8d2b/packages/opencode/src/session/session.sql.ts) 与 [fork 实现](https://github.com/XiaomiMiMo/MiMo-Code/blob/be5af909aeccdeb1b707ac4c5f9214e6fe4b8d2b/packages/opencode/src/session/session.ts) 证明消息/session creation time；fork 会复制 usage、保留原消息时间并改写消息 ID，但不写 parent ID。
- [Kilo CLI schema](https://github.com/Kilo-Org/kilocode/blob/bbf6a278d791842ababfbc8d58f902cb0f6b9bf4/packages/core/src/session/sql.ts) 同时定义 `message` 与 `session_message`，[fork 实现](https://github.com/Kilo-Org/kilocode/blob/bbf6a278d791842ababfbc8d58f902cb0f6b9bf4/packages/opencode/src/session/session.ts) 证明复制历史使用新 ID 且 copied cost 归零。
- [Senpi session format](https://github.com/code-yeongyu/senpi/blob/7ac3cf302950a4b258421748f944ed1281007c4b/packages/coding-agent/docs/session-format.md) 与 [session manager](https://github.com/code-yeongyu/senpi/blob/7ac3cf302950a4b258421748f944ed1281007c4b/packages/coding-agent/src/core/session-manager.ts) 证明 assistant、compaction、branch summary、tool-result usage 和 branch-copy ID。
- [Kimchi child writer](https://github.com/getkimchi/kimchi/blob/eacd20f15c2e2ed2b8d24fc52f08fa7638b8f759/src/extensions/agents/manager/session-file.ts) 与 [launcher](https://github.com/getkimchi/kimchi/blob/eacd20f15c2e2ed2b8d24fc52f08fa7638b8f759/src/entry.ts) 证明 child transcript 独立落盘、parent tool-result 只是 rollup，而且目录被 launcher 固定。
- [GJC session manager](https://github.com/Yeachan-Heo/gajae-code/blob/7d23ed3d9e8cb6e5062ba2840462d59fe18eb784/packages/coding-agent/src/session/session-manager.ts) 证明 v5 header/patch、fork copy 与 task child transcript；[AI Usage](https://github.com/Yeachan-Heo/gajae-code/blob/7d23ed3d9e8cb6e5062ba2840462d59fe18eb784/packages/ai/src/types.ts) 证明 reasoning 和 cache TTL 的包含关系。
- [Prime Agent session manager](https://github.com/PrimeIntellect-ai/prime-agent/blob/c382f09856d4a8c8d2b765179657047d58691f25/packages/coding-agent/src/core/session-manager.ts) 证明 `child_usage_attributed` 会用 aggregate 覆盖 parent；[context tree](https://github.com/PrimeIntellect-ai/prime-agent/blob/c382f09856d4a8c8d2b765179657047d58691f25/packages/coding-agent/src/core/context-tree.ts) 证明 own usage 是最后 aggregate 减全部 child usage。
- [Oh My Pi dirs](https://github.com/can1357/oh-my-pi/blob/969062200754ea02cfac922e5ebb8c608c079e15/packages/utils/src/dirs.ts) 证明 active profile/XDG 目录语义；[task types](https://github.com/can1357/oh-my-pi/blob/969062200754ea02cfac922e5ebb8c608c079e15/packages/coding-agent/src/task/types.ts) 证明 `details.usage` 与 `results[].usage` 都是 child rollup，而非额外调用。
- [OpenClaw Usage](https://github.com/openclaw/openclaw/blob/2181ae7ba2e836451e90068ec1a41e31bef87f93/packages/llm-core/src/types.ts) 定义互斥 input/output/cache 桶、1h cache 子桶与 `totalOrigin = provider-billed`；[SQLite schema](https://github.com/openclaw/openclaw/blob/2181ae7ba2e836451e90068ec1a41e31bef87f93/src/state/openclaw-agent-schema.sql)、[SDK store path](https://github.com/openclaw/openclaw/blob/2181ae7ba2e836451e90068ec1a41e31bef87f93/src/agents/sessions/sdk.ts) 和 [artifact classifier](https://github.com/openclaw/openclaw/blob/2181ae7ba2e836451e90068ec1a41e31bef87f93/src/config/sessions/artifacts.ts) 分别证明 current DB、默认路径以及 reset/deleted 与 checkpoint 的边界。
- [Xum session usage schema](https://github.com/coder/mux/blob/ad7f569ef21cc293ecbb71f8b718e30d11da4b27/src/common/orpc/schemas/chatStats.ts) 定义五个互斥桶、`costsIncluded` 和 `rolledUpFrom`；[roll-up 实现](https://github.com/coder/mux/blob/ad7f569ef21cc293ecbb71f8b718e30d11da4b27/src/node/services/sessionUsageService.ts) 证明 parent 已累加 child，剩余 child snapshot 不能再次统计。
- [Hermes current schema](https://github.com/NousResearch/hermes-agent/blob/4f22543509d1b91dc45bcb369447126c5eb14fb7/hermes_state_common.py) 定义 `session_model_usage` 的 endpoint/mode/task 复合主键；[CanonicalUsage](https://github.com/NousResearch/hermes-agent/blob/4f22543509d1b91dc45bcb369447126c5eb14fb7/agent/usage_pricing.py) 定义 reasoning 是 output 子集、cache 是独立 prompt 桶和 actual/estimated/included cost status；[Insights](https://github.com/NousResearch/hermes-agent/blob/4f22543509d1b91dc45bcb369447126c5eb14fb7/agent/insights.py) 证明当前 session aggregate 必须减去细分 rows 后作为 residual。
- [Reasonix stats writer](https://github.com/futureflowtech/reasonix/blob/e9e4ca68ba6d1f82679e2f2877bdbbee89e1c19d/internal/stats/record.go) 定义按日 append-only provider-call 行及 cost quote 字段；[provider usage](https://github.com/futureflowtech/reasonix/blob/e9e4ca68ba6d1f82679e2f2877bdbbee89e1c19d/internal/provider/provider.go#L711-L734) 证明 cache hit/miss 划分 prompt，reasoning 是 completion 子集。
- [Fx profile store](https://github.com/vercel-labs/fx/blob/2ed0f44c5913dd61d35cba8495838a9f1542ade1/src/core/session/profile_usage_store.zig#L882-L919) 定义 `usage.jsonl` 的 generation/pending/incident 记录；[generation fact codec](https://github.com/vercel-labs/fx/blob/2ed0f44c5913dd61d35cba8495838a9f1542ade1/src/core/session/generation_fact_codec.zig#L6-L31) 固定 ID、时间、模型、token 与 USD cost；[snapshot validation](https://github.com/vercel-labs/fx/blob/2ed0f44c5913dd61d35cba8495838a9f1542ade1/src/core/session/session_usage.zig#L2164-L2323) 证明 cache/reasoning 是 inclusive parent 的子集且 model aggregates 必须与顶层相等；[bounded recovery collector](https://github.com/vercel-labs/fx/blob/2ed0f44c5913dd61d35cba8495838a9f1542ade1/src/core/session/usage_recovery.zig#L67-L132) 与 [canonical read boundary](https://github.com/vercel-labs/fx/blob/2ed0f44c5913dd61d35cba8495838a9f1542ade1/src/core/session/session_log.zig#L1251-L1328) 证明 marker、canonical replay、protected timestamp 和 sidecar merge 的顺序。

## Tokscale 52-client 差距分解

Tokscale 的 registry 数量适合衡量发现覆盖面，不适合直接衡量算法数量。按实际数据所有权拆分如下：

| 分组 | 代表 client | 算法关系 | ccstats 决策 |
|---|---|---|---|
| 已覆盖且已审计 | Claude、Codex、Cursor、Gemini、Amp、Kimi、Qwen、Cline、Roo、Kilo Code、Grok | 11 个现有 source | 保持逐来源证据，不回退为通用 extractor |
| OpenCode SQLite 族 | OpenCode、MiMo Code、Kilo CLI | 同一消息 payload，路径、表版本、成本 provenance 和 fork ID 有差异 | 三者已按各自当前 schema 实现；MiMo/Kilo 用 message/session creation time 识别复制历史 |
| Pi JSONL 族 | Pi、GJC、Senpi、Kimchi、Prime Agent、Oh My Pi | 基础 assistant usage 相近；子调用与聚合层并不相同 | 六者已按各自 reconciliation 实现；不使用通用 rollup extractor |
| 已实现的独立格式 | GitHub Copilot CLI、Goose、OpenClaw、Xum、Hermes | OTel per-call、SQLite ledger、JSONL per-call、JSON aggregate 混合 | 已按各自 provenance 与 reconciliation 实现，不把累计层再次相加 |
| 独立本地格式候选 | Droid、Codebuff、Zed、Junie、Augment、DSH、LM Studio、Unsloth 等 | JSONL、SQLite、IDE cache 混合 | 逐个取得官方 schema 或真实 fixture 后进入 |
| 在线 quota / subscription | Antigravity、Trae、Warp | 运行中 RPC、authenticated cache 或 GraphQL aggregate | 独立 product track，不伪装为本地 token ledger |
| 无权威 token ledger | Crush、Kiro、Command Code、MiniMax headless capture | cost-only、按文本估算或由 Tokscale 自行捕获 | 拒绝进入 authoritative source registry |
| 产品层而非 token parser | usage/quota、headless wrapper、profile、leaderboard、device/group、autosubmit、MCP | 系统边界不同 | 单列产品路线；不为追求 source 数量混入核心账本 |

这意味着“全方位补齐”应分成两条可验证轨道：先补 authoritative local accounting，再补 quota/桌面/社区产品能力。把两者塞进同一个 parser PR 会让错误数字更难发现。

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
- Copilot CLI：OTel input/output 分别包含 cache read/write 与 reasoning detail，两个方向都要拆除子集。
- Goose：官方 input 包含 cache read/write；官方没有 reasoning 字段，不从 total 差额推断。
- OpenClaw/Xum：持久化五桶已经互斥，不再做减法；OpenClaw 当前没有独立 reasoning 桶。
- Hermes：cache 是独立 prompt 桶，reasoning 是 output 子集，只从 output 扣 reasoning。
- Reasonix：cache hit/miss 划分 prompt，reasoning 是 completion 子集；账本未保存 cache write，不能猜。
- Fx：cache read/write 都是 input 子集，reasoning 是 output 子集，必须从 parent bucket 扣除。

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

客户端 recorded cost、provider invoice、API list-price estimate和订阅额度消耗不是同一个值。当前 `RawEntry` 可单独保存 source-recorded USD cost，但每个数据源必须说明它是 provider bill，还是当时价格表生成的客户端估值。Amp credits 与本轮读取的 Cline message/task 记录没有得到可验证的一一归属和 USD 单位，因此仍写为 `None`，由价格层估算。缺少可靠价格时应显示未知或 fallback，而不是声称为实际账单。

### 5. 错误不能静默变成零使用量

空文件和没有 usage 的合法事件可以跳过；无法读取、损坏 JSONL、非法时间戳导致有效 usage 丢失时必须增加 parse error。这样 CLI 才能把“不存在使用量”和“解析失败”区分开。

## ccstats 实现前 11 个来源审计

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

本批新增结果：

| 来源 | 数据路径与算法 | 证据状态 | 结论 |
|---|---|---|---|
| OpenCode | 当前 SQLite `message` + `session_message`；五个桶已在写入端分离；跨表/跨 channel ID 去重 | 官方源码已验证 + Tokscale/ccusage 交叉验证 | 实现为第 12 个 source；保留项目、reasoning、cache 与正的 recorded cost |
| Pi | JSONL assistant + compaction + branch summary；reasoning 属于 output 子集；branch copy 按 entry ID 去重 | 官方源码已验证 + Tokscale fixture 交叉验证 | 实现为第 13 个 source；补上 Tokscale 普通 Pi parser 未统计的 summary usage |
| GitHub Copilot CLI | OTel file exporter 的 per-request `chat` span；拆分 cache/reasoning inclusive bucket；trace/span 跨文件去重 | 官方文档已验证 + Tokscale current fixture 交叉验证 | 实现为第 14 个 source；不累计 `invoke_agent`，修复 Tokscale cache creation/reasoning 双计 |
| Goose | schema v15+ `usage_ledger`；逐调用时间/model/cache/cost，join session project | 官方源码已验证 + Tokscale parser 对比 | 实现为第 15 个 source；不使用 session snapshot，不猜 reasoning，只接受 provider-reported cost provenance |
| MiMo Code | OpenCode-family `message`；独立 token 桶；按 copy timestamp + model/five-bucket + unique original session 识别新 ID fork copy | 官方源码已验证 | 实现为第 16 个 source；支持跨目录与 original-deleted multi-copy fork，保留显式 recorded cost，包括零值 |
| Kilo CLI | legacy `message` + current `session_message`；跨表 ID 和 fork lineage 去重 | 官方源码已验证 | 实现为第 17 个 source；同时覆盖当前/旧表，不把 copied-zero history 当新调用 |
| Senpi | Pi v3 四类 usage carrier；branch copy 保留 entry ID；JSONC settings 合并 | 官方源码已验证 | 实现为第 18 个 source；统计 tool-result 子调用、跨分支去重并发现 project/global sessionDir |
| Kimchi | parent `details.tokenUsage` + sibling child transcript；remote 没有 session file | 官方源码已验证 | 实现为第 19 个 source；含 usage 的本地 child 优先，header-only/缺失/remote 时 parent details fallback |
| Gajae Code | v5 assistant + patch replay；只扣已有 direct child result usage，缺失 child 用 task residual 补总量；fork artifact 沿 parent lineage 解析 | 官方源码已验证 + adversarial fork fixture | 实现为第 20 个 source；reasoning/TTL 分桶、保留显式零成本，nested child 独立计数且 fork 不重复 residual |
| Prime Agent | parent 最后 aggregate 减全部 child attribution；recursive child 独立计数；attribution 前分支从 ancestor 恢复 own usage | 官方源码已验证 + adversarial pre-attribution fork fixture | 实现为第 21 个 source；状态重建后再按 usage/cost-aware fingerprint 去重，支持 project/global sessionDir |
| Oh My Pi | active profile/XDG；assistant orchestration；递归 child/advisor transcript；过滤 lower-priority profile 派生 agent dir | 官方源码已验证 + env-priority fixture | 实现为第 22 个 source；忽略 task 两层 rollup，保留 reasoning/cache/orchestration，显式 default profile 不串读 named profile |
| OpenClaw | 当前 v3 JSONL/reset/deleted zstd + 默认/配置 SQLite events/archives；entry id 跨 store/fork 去重；cost origin 与 cache TTL 分离 | 官方源码已验证 + JSONL/SQLite/zstd/custom-store/dedup/error-isolation E2E | 实现为第 23 个 source；排除 checkpoint/trajectory/bak，坏 archive 不吞 active rows，只锁定 provider-billed USD |
| Xum | 当前 `~/.xum` cumulative `session-usage.json`；同 root `rolledUpFrom` reconciliation；混合 included/paid 的完整五桶 cost | 官方源码已验证 + parent/child/invalid-parent/cycle/mixed-cost E2E | 实现为第 24 个 source；不读取旧 Mux root，坏 parent 不吞 child，环内保留单一 canonical ledger 并报错，累计调用次数保持未知 0 |
| Hermes Agent | 当前 `session_model_usage` 每个合法 task/billing row + session residual；visible-output/reasoning 拆分；API call count | 官方源码已验证 + multi-task/null-model/bad-row/residual/cost-status E2E | 实现为第 25 个 source；不使用 Tokscale 的 message_count，aggregate 只扣成功细分行并补剩余差额 |
| Reasonix | `<state>/stats/YYYY-MM-DD.jsonl` provider-call ledger；cache/reasoning 子集拆分；完整 USD quote 优先 | 官方源码已验证 + env-priority/malformed/cost/request-count E2E | 实现为第 26 个 source；不扫描 transcript，不伪造 project/session，不把负数 clamp 为零 |
| Vercel Fx | `~/.fx/usage.jsonl` generation facts + recovery registry 标记、canonical commit boundary/state replacement 可重放且 sidecar 完整有效的 publication backlog；ID 去重；inclusive cache/reasoning 拆分；显式零成本 | 官方源码已验证 + duplicate/conflict/canonical-marker/state-replacement recovery/sidecar-only rejection/private-file boundary/sidecar-distractor/zero-cost E2E | 实现为第 27 个 source；projection 不一致时保留 recovery hints 并报 completeness，拒绝泛扫或脱离 canonical session 的 sidecar 导致双计、伪造和日期漂移 |

## 本轮采用、适配、拒绝决策

### 采用

- Qwen 官方 usage ledger schema、目录优先级和 UUID 去重键。
- Codex 同时扫描 active 与 archived session roots。
- 官方字段语义优先于任何第三方 parser。

### 适配

- Tokscale 的 Amp 双视图合并顺序。
- Tokscale、Codeburn、Tokenleak 的 Gemini cache 归一化测试向量。
- CodexBar 的“本地 cost”和“在线 quota”分层概念，但不引入其桌面账户管理层。
- Tokscale 的 OpenCode 双表发现与跨表去重思路，但以当前 OpenCode 官方 schema 为字段语义权威。
- Tokscale 的 Pi assistant 记录筛选；额外统计官方 schema 已证明的 compaction / branch summary usage。
- Copilot 使用 Tokscale 已观察到的 file-export JSONL 外形和 trace/span 去重思路，但记录选择与字段语义以当前 GitHub/OTel 文档为准。
- Goose 采用官方 usage ledger，而不是适配 Tokscale 的 session aggregate query。
- MiMo/Kilo 复用已验证的 OpenCode payload 映射，但分别适配官方路径、双表和 creation-time fork reconciliation。
- Senpi 复用 Pi v3 的四类 usage carrier；Kimchi 在 sibling child 存在时关闭 parent rollup，缺失/remote 时解析 `details.tokenUsage`。
- GJC 适配 v5 patch 与部分 task residual；Prime 按 attribution 重建 parent own usage；OMP 按 active profile 发现并只统计递归 transcript，不累计 task rollup。
- OpenClaw 适配 current JSONL/SQLite/cold archive、entry identity、cache TTL 与 cost origin；Xum 只让合法无环 parent 压掉 child，并在五桶成本完整时锁定 ledger cost；Hermes 保持复合计费维度、补 session residual，并按 cost status 分流 actual/included 与 estimated。
- Reasonix 采用官方 per-call stats，但只在 quote complete 且存在可信 USD valuation 时锁定历史成本；Fx 采用 profile generation ledger，并把 pending/conflict/incident 暴露为数据质量问题。

### 拒绝

- 直接复制 Tokscale parser：会把已确认的 Qwen 旧路径和重复计数带进来。
- 使用一个通用字段别名表处理所有供应商：相同名称在不同 API 中可能是包含值或独立值。
- 根据 prompt/response 文本重新 tokenizer：模型 tokenizer、隐藏 system prompt、tool schema 和服务端缓存都不可完整重建。
- 在没有验证 USD 单位和调用归属时导入 credits/reported cost：即使模型支持 provenance，也会把不同口径的值混在一起。
- 把 Copilot `invoke_agent` 和 child `chat` 一起相加，或把未声明 currency 的 `github.copilot.cost` 直接标成 USD。
- 把 Goose `total-input-output` 猜成 reasoning，或把累计 session snapshot 伪装成创建日的一次调用。
- 把 GJC v5 当普通 Pi v3、把 Prime parent aggregate 与 child/fork transcript 全相加、或把 Oh My Pi task rollup 与 child transcript 全相加；三者已用独立状态重建与 fixture 代替这种通用 parser。
- 把 OpenClaw 本地估值当 provider invoice、把 Xum parent 与 rolled-up child 再次相加、或用 Hermes session message count 代替 `api_call_count`。
- 把所有 Fx `usage-v2.json` 当全局账本泛扫：recovery 会复制累计 snapshot，跨日 usage 还会全部漂移到 session 更新时间。只接受官方 recovery registry 指定、canonical commit boundary 可重放且 sidecar 快照自身完整有效的 `publication_backlog`；projection 不一致报告不完整，sidecar-only 明确失败。也拒绝把 Fx inclusive input/output 与 cache/reasoning 五桶直接相加。
- 把 Kiro/Command Code 文本估算、Crush cost-only 或 Warp quota aggregate 注册成 authoritative token ledger。

## 后续数据源批次

新增来源必须先取得官方 schema 或两个独立 fixture，再进入实现。建议顺序：

1. Batch 1（已完成）：OpenCode + Pi + discovery→parse→aggregate→CLI 端到端矩阵。
2. Batch 2（已完成）：GitHub Copilot CLI OTel + Goose per-call SQLite ledger。
3. Batch 3（已完成）：MiMo Code、Kilo CLI、Senpi、Kimchi；逐个验证写入端、目录、fork/child reconciliation 和端到端输出。
4. Batch 4（已完成）：GJC、Prime Agent、Oh My Pi；验证 patch、child attribution、task rollup、profile/XDG 与 recursive fork。
5. Batch 5（已完成）：OpenClaw、Xum、Hermes Agent；验证 JSONL/SQLite/zstd、fork/store dedup、child roll-up/环、完整成本、session residual、task/billing 维度与调用次数。
6. Batch 6（已完成）：Reasonix + Fx；补全 occurrence-time USD provenance，改用 Fx profile generation ledger，并修复 inclusive cache/reasoning 双计。
7. Batch 7：Unsloth；按官方 fork clone keeper 规则做跨 thread reconciliation，并同时覆盖 chat/API 两条 lane。
8. 后续 authoritative candidates：Droid、Codebuff、Zed、Junie、DSH、LM Studio 等；没有官方 schema 时必须取得匿名化真实 fixture 和第二实现交叉验证。
9. Product track：桌面应用、quota、实时观测与多机器历史；与 authoritative token ledger 保持 provenance 隔离。

每一批独立提交与 PR，包含官方证据链接、最小真实格式 fixture、RED/GREEN 测试记录、数据路径和已知限制。这样覆盖面可以持续增长，但不会用错误数字换取“支持数量”。
