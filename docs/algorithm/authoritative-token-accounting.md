# Token Accounting Algorithm

ccstats 从本地 JSONL、SQLite 和 usage API 中统计 token 用量和费用。不同 AI 工具的字段语义有本质差异，每个数据源的 parser 负责将原始字段**归一化为互不重叠的 5 个维度**，之后统一计算。

> 本文档为 ccstats 统计算法的权威参考。所有数值均为本地日志的最佳近似，绝对准确值以服务端账单为准。

---

## 配置与数据源根目录

ccstats 启动时先读取可选的 TOML 配置文件，再根据命令行参数和数据源环境变量寻找本地日志。配置文件搜索顺序：

1. `~/.config/ccstats/config.toml`
2. 平台配置目录，例如 macOS 的 `~/Library/Application Support/ccstats/config.toml`
3. `~/.ccstats.toml`

第一个存在的配置文件生效。如果该文件无法读取、TOML 语法错误或字段类型错误，命令直接报错退出，不会继续尝试低优先级配置，也不会回落默认值。没有配置文件时使用默认值。

支持的配置键：

| 键 | 类型 | 取值 |
|----|------|------|
| `offline` | boolean | `true` / `false` |
| `compact` | boolean | `true` / `false` |
| `no_cost` | boolean | `true` / `false` |
| `no_color` | boolean | `true` / `false` |
| `breakdown` | boolean | `true` / `false` |
| `debug` | boolean | `true` / `false` |
| `strict_pricing` | boolean | `true` / `false` |
| `order` | string | `asc` / `desc` |
| `color` | string | `auto` / `always` / `never` |
| `cost` | string | `show` / `hide` |
| `timezone` | string | IANA 时区，例如 `UTC` / `Asia/Shanghai` |
| `locale` | string | 数字格式 locale，例如 `en` / `de` |
| `currency` | string | 货币代码，例如 `USD` / `CNY` / `EUR` |
| `source` | string | 已注册 source 名或别名，例如 `claude` / `codex` / `opencode` / `pi` / `copilot` / `goose` / `dsh` / `all` |

示例：

```toml
source = "codex"
timezone = "Asia/Shanghai"
currency = "USD"
offline = true
strict_pricing = true
order = "desc"
color = "auto"
cost = "show"
```

数据源根目录由环境变量覆盖，不属于 TOML 配置键：

| 数据源 | 环境变量 | 含义 | 默认值 |
|--------|----------|------|--------|
| Claude Code | `CLAUDE_CONFIG_DIR` | 包含 `projects/` 的 Claude 配置根目录 | `~/.claude` |
| OpenAI Codex | `CODEX_HOME` | 包含 `sessions/` 和 `archived_sessions/` 的 Codex 根目录 | `~/.codex` |
| Cursor | `CURSOR_API_KEY` / `CURSOR_SESSION_TOKEN` | Cursor usage API credentials | 无默认值；可用 `CURSOR_USAGE_FILE` 回放 |
| Grok | `GROK_HOME` | 包含 `sessions/` 的 Grok 根目录 | `~/.grok` |
| Kimi Code | `KIMI_CODE_HOME` | 包含 `sessions/` 的 Kimi 根目录 | `~/.kimi-code` |
| Gemini CLI | `GEMINI_CLI_HOME` | 包含 `tmp/` 的 Gemini 根目录 | `~/.gemini` |
| Amp | `XDG_DATA_HOME` | 包含 `amp/threads/` 的用户数据根目录 | `~/.local/share` |
| Qwen Code | `QWEN_RUNTIME_DIR`，其次 `QWEN_HOME` | 包含 `usage/` 的 Qwen 根目录 | `~/.qwen` |
| Cline CLI | `CLINE_SESSION_DATA_DIR`，另支持 `CLINE_DATA_DIR`、`CLINE_DIR` | Cline session 目录或数据根目录 | `~/.cline/data/sessions` |
| OpenCode | `OPENCODE_DB`，数据根目录遵循 `XDG_DATA_HOME` | 数据库绝对路径，或 OpenCode 数据目录内的相对文件名 | 平台 data dir 下的 `opencode/opencode*.db` |
| MiMo Code | `MIMOCODE_DB`；`MIMOCODE_HOME`；数据根目录遵循 `XDG_DATA_HOME` | 数据库绝对路径，或包含 `data/` 的 MiMo home | `~/.local/share/mimocode/mimocode*.db` |
| Kilo CLI | `KILO_DB`，数据根目录遵循 `XDG_DATA_HOME` | 数据库绝对路径，或 Kilo data directory 内的相对文件名 | `~/.local/share/kilo/kilo*.db`，并扫描 legacy channel 数据库 |
| Pi | `PI_CODING_AGENT_SESSION_DIR`，其次 `PI_CODING_AGENT_DIR` | sessions 目录，或包含 `sessions/` 的 agent 目录 | `~/.pi/agent/sessions` |
| Senpi | `SENPI_CODING_AGENT_SESSION_DIR`，其次 `SENPI_CODING_AGENT_DIR` | sessions 目录，或包含 `sessions/` 的 agent 目录；展开 `~` | 最近的项目 `.senpi/agent/sessions`，其次 `~/.senpi/agent/sessions` |
| Kimchi | 无 | launcher 固定目录 | `~/.config/kimchi/harness/sessions` |
| Gajae Code | `GJC_CODING_AGENT_DIR`；`GJC_CONFIG_DIR`；数据根目录遵循 `XDG_DATA_HOME` | 包含 `sessions/` 的 agent 目录，或 config 目录名 | `~/.gjc/agent/sessions` 或迁移后的 `$XDG_DATA_HOME/gjc/sessions` |
| Prime Agent | `PRIME_AGENT_SESSION_DIR`，其次 `PRIME_AGENT_CODING_AGENT_DIR`；另读取当前 cwd/global `settings.json` | sessions 目录、agent 目录或 settings `sessionDir` | `~/.prime/agent/sessions` |
| Oh My Pi | `PI_CODING_AGENT_SESSION_DIR`；`OMP_PROFILE`，其次 `PI_PROFILE`；`PI_CODING_AGENT_DIR`；`PI_CONFIG_DIR`；数据根目录遵循 `XDG_DATA_HOME` | 显式 sessions 目录、当前 active profile 或非 profile 派生 agent 目录 | `~/.omp/agent/sessions` 或 profile/XDG 对应目录 |
| GitHub Copilot CLI | `COPILOT_OTEL_FILE_EXPORTER_PATH` | OTel file exporter 的 JSONL 文件 | 另扫描 `~/.copilot/otel/**/*.jsonl` |
| Goose | `GOOSE_PATH_ROOT`，数据根目录遵循 `XDG_DATA_HOME` | 包含 `data/sessions/sessions.db` 的绝对 path root | `~/.local/share/goose/sessions/sessions.db` |
| Unsloth Studio | `UNSLOTH_STUDIO_HOME`，其次 `STUDIO_HOME` | 包含 `studio.db` 的 Studio 根目录；展开 `~` | `~/.unsloth/studio` |
| DeepSeek Harness | `DSH_HOME` | 包含 `sessions/` 的 DSH 根目录；相对路径以当前工作目录为基准，展开 `~` | `~/.dsh` |

---

## 统一数据模型

所有数据源解析后输出统一的 `RawEntry`，其中 token 字段**互不重叠**：

| 字段 | 含义 | 说明 |
|------|------|------|
| `input_tokens` | 非缓存输入 | 不含 cache_read 部分 |
| `output_tokens` | 非推理输出 | 不含 reasoning 部分 |
| `reasoning_tokens` | 推理输出 | 仅推理/思考 token |
| `cache_creation` | 缓存写入 | 首次写入缓存的 token |
| `cache_read` | 缓存读取 | 命中缓存的输入 token |

### 汇总公式

```
total_tokens = input_tokens + output_tokens + reasoning_tokens + cache_creation + cache_read
```

少数权威载体会把 `total_tokens` 作为独立计数器保存，且明确允许它与分量之和不同。此时 ccstats 保留原始分量用于定价，同时用内部 `reported_total_adjustment` 保持展示总量等于来源报告值；不会把差值伪装成 input token。

## Unsloth Studio

Unsloth Studio 的当前 `studio.db` 同时包含两条互斥的推理 usage lane：内部 chat 的 assistant `metadata_json`，以及经过 API key 调用后写入的 content-free `api_usage_events`。训练表的 `num_tokens` 是训练进度，不属于推理统计。

- chat 的 `promptTokens` 包含 `cachedTokens` 与 `cacheWriteTokens`，parser 在边界拆成互斥 input/cache 桶；persisted metadata 不提供独立 reasoning counter，因此 completion 全部进入 output。
- 模型优先采用 `responseDetails.responseModelId`，其次才是请求时的 `contextUsage.modelId`；线程当前模型不能代表历史响应模型。
- 本地 llama usage chunk 缺失时，按官方规则从 `serverTimings.prompt_n/predicted_n/cache_n` 与 `timing.tokenCount` 恢复。
- fork 会以新 message ID 复制旧 ancestry。ccstats 使用官方 `(source thread, created_at, role)` keeper 规则：原件存在时忽略 copy；原件删除后只保留字典序最小的 sibling fork copy。
- API `id` 是幂等键，`total_tokens` 为独立权威值；不同 `subject` 使用不可逆摘要分隔 session，避免把身份字符串暴露到输出。
- 两张表都没有 source-recorded USD。ccstats 不强制写入 `$0`，而是按模型价格能力进行本地估价并保留未知状态。

## DeepSeek Harness

DeepSeek Harness 的权威持久化载体是
`<DSH_HOME>/sessions/<project-key>/<encoded-session-id>/session.jsonl.zstd`，
未启用压缩时同一位置为 `session.jsonl`。一个 sessions root 只允许一种编码；
parser 只扫描这两层目录，不把备份、临时文件或更深的噪声当会话。

- header 必须是 format version 0。子会话继承的 `seq < seedLength` 归父会话所有，只有 suffix 属于子会话；packed `text-chunks`、reasoning/tool rows 没有 usage，不参与统计。
- assistant chunk 的 usage 是当前 attempt 的暂存样本；同 turn/step 的最终 assistant message usage 替换它，而不是再次相加。最终 message 没有 usage 时保留 chunk token，但把时间与模型重归属到已验证的最终 message route。`llm/retry-started` 关闭当前样本，下一次 usage 新增一个 attempt，因此失败但已返回 usage 的调用仍可计费。
- chunk 的 provider/model 来自最近的 request header；成功 message 采用 `message.source`。只有通过 pi-ai replay envelope 版本与 provider/model 一致性校验时，`responseModel` 才可覆盖 route model。
- compaction/summary 是独立模型调用，使用事件自己的 provider、model、usage 与 compaction ID。`shadowedTokenCount` 是上下文状态，不是新的 token 调用。
- DSH 原生 input、cache read、cache write、output 相互独立；reasoning 是 output 的子集，所以 ccstats 的 visible output 为 `output - reasoning`。`totalTokens` 是独立权威总量，允许与分量和不同，只通过 `reported_total_adjustment` 校准展示总数。
- 普通 JSONL 只忽略未换行的最终半行。zstd 是多个独立、带 checksum 的 frame 串接：完整 frame 必须全部通过结构、解压和 checksum 校验；仅未完成且压缩前缀本身有效的最后一帧可保留其中已经完整换行的 JSONL 行。单个压缩载体及其解压结果各有 128 MiB 显式保护上限，超限作为该会话的 parse error。读取前后还会核对文件 revision，避免把并发 append 的中间态当稳定账本。
- 每个逻辑事件的 seq 必须从 0 连续递增，packed row 按成员数推进序号；header 的 cwd/id 必须重新编码后指向当前物理文件。当前官方事件词汇之外的记录只有显式带 `ignorable: true` 才可跳过。
- 会话没有 source-recorded USD，费用由统一价格层估算。负 token、cache/reasoning 越界、无效时间、未知 format、混用编码或已提交 frame 损坏都会成为 parse error，不静默降级为零。

### 费用公式

```
cost = input_tokens   × input_price
     + output_tokens   × output_price
     + reasoning_tokens × reasoning_price
     + cache_creation   × cache_create_price
     + cache_read       × cache_read_price
```

每种 token 只计费一次，不存在重叠。

---

## Claude Code (Anthropic)

### 日志位置

```
~/.claude/projects/<project>/*.jsonl
~/.claude/projects/<project>/subagents/*.jsonl
```

可通过 `CLAUDE_CONFIG_DIR` 覆盖 Claude 配置根目录：

```
CLAUDE_CONFIG_DIR=/path/to/claude-config ccstats daily --source claude
```

### 原始字段

Anthropic API 的 `message.usage` 中，**每个字段独立、互不包含**：

```json
{
  "message": {
    "id": "msg_xxx",
    "model": "anthropic.claude-3-5-sonnet-20241022",
    "stop_reason": "end_turn",
    "usage": {
      "input_tokens": 100,
      "output_tokens": 50,
      "cache_creation_input_tokens": 10,
      "cache_read_input_tokens": 20
    }
  }
}
```

### 字段映射（无需变换）

```
input_tokens       ← usage.input_tokens           (直接使用)
output_tokens      ← usage.output_tokens           (直接使用)
reasoning_tokens   ← 0                             (Claude 无推理 token)
cache_creation     ← usage.cache_creation_input_tokens
cache_read         ← usage.cache_read_input_tokens
```

Anthropic 的字段天然互不重叠，parser 直接映射即可。

### 去重算法

Claude Code 的流式响应会为同一个 `message.id` 写入多条日志（每个 chunk 都可能更新 usage）。去重规则：

1. 以“源日志文件 + `message.id`”作为去重键
2. 同一去重键的多条记录，选择规则：
   - 优先选有 `stop_reason` 的（表示完成），取最新的一条
   - 若都没有 `stop_reason`，取最晚的一条（最佳近似）
3. 没有 `message.id` 的条目：仅当有 `stop_reason` 时才计入

这样可以避免不同日志文件中碰巧复用同一 `message.id` 时发生误去重，同时仍然保留同一文件内流式 chunk 的合并行为。

### 模型名归一化

```
"anthropic.claude-3-5-sonnet-20241022"
  → 去掉 "anthropic." 前缀
  → 去掉 "claude-" 前缀
  → 去掉 "-YYYYMMDD" 日期后缀
  → 结果: "3-5-sonnet"
```

---

## OpenAI Codex CLI

### 日志位置

```
~/.codex/sessions/*.jsonl
~/.codex/archived_sessions/*.jsonl
```

可通过 `CODEX_HOME` 环境变量覆盖。

### 原始字段

OpenAI API 的 token 字段**存在包含关系**：

```json
{
  "type": "event_msg",
  "payload": {
    "type": "token_count",
    "info": {
      "total_token_usage": {
        "input_tokens": 1000,
        "cached_input_tokens": 200,
        "output_tokens": 500,
        "reasoning_output_tokens": 200,
        "total_tokens": 1500
      },
      "last_token_usage": { ... }
    }
  }
}
```

**关键差异**：OpenAI 的字段有嵌套包含关系：

```
input_tokens (1000) ⊇ cached_input_tokens (200)
output_tokens (500) ⊇ reasoning_output_tokens (200)
```

即 `input_tokens` 已包含 `cached_input_tokens`，`output_tokens` 已包含 `reasoning_output_tokens`。

### 字段映射（需要减法分离）

```
input_tokens       ← (input_tokens - cached_input_tokens).max(0)     = 800
output_tokens      ← (output_tokens - reasoning_output_tokens).max(0) = 300
reasoning_tokens   ← reasoning_output_tokens                          = 200
cache_creation     ← 0  (Codex 不支持)
cache_read         ← cached_input_tokens                              = 200
```

分离后各字段互不重叠，可安全求和：
```
total = 800 + 300 + 200 + 0 + 200 = 1500 ✓
```

若不做减法（bug 行为）：
```
total = 800 + 500 + 200 + 0 + 200 = 1700 ✗ (reasoning 被重复计算)
```

### 累积值转增量

Codex 日志中 `total_token_usage` 是**累积值**（session 内单调递增），需要转换为每次调用的增量：

1. 如果 `last_token_usage` 存在，直接使用（它就是本次调用的增量）
2. 否则，用当前 `total_token_usage` 减去上一条的 `total_token_usage` 得到增量
3. 如果 `total_tokens` 未变化，跳过（重复事件）

### 去重

Codex parser 先把累计值转换成增量，再用逻辑 session、模型、完整累计向量和增量向量生成稳定事件 ID。loader 在 source 范围去重，因此同一个 session 同时出现在活动目录和归档目录时不会重复统计；只比较完整 token 向量，不用单个 `total_tokens` 判断重复。

### 模型获取

模型名从多个位置按优先级获取：
1. `payload.info.model`
2. `payload.info.model_name`
3. `payload.info.metadata.model`
4. `payload.model`
5. 上一条 `turn_context` 事件中的模型
6. 默认 `"gpt-5"`

---

## Qwen Code

### 日志位置与优先级

Qwen Code 的权威本地计量源是按月追加的 usage ledger，不是旧版 project chat：

```text
<qwen-root>/usage/token-usage-YYYY-MM.jsonl
```

根目录依次取 `QWEN_RUNTIME_DIR`、`QWEN_HOME`、`~/.qwen`。显式设置的根目录不会因目录暂时不存在而回退到低优先级位置。

### 字段映射

当前只接受官方 `schemaVersion = 1`。每行是一条完整 API 调用，核心字段为 `id`、`timestamp`、`sessionId`、`model`、`inputTokens`、`outputTokens`、`cachedTokens`、`thoughtsTokens` 和 `totalTokens`。

Qwen 的 `cachedTokens` 已包含在 `inputTokens` 中，因此必须分离：

```text
input_tokens       ← inputTokens - cachedTokens
output_tokens      ← outputTokens
reasoning_tokens   ← thoughtsTokens
cache_creation     ← 0
cache_read         ← cachedTokens
```

官方 total fallback 是 `inputTokens + outputTokens + thoughtsTokens`，不会再次加 `cachedTokens`，这也是 cache 属于 input 子集的直接证据。ledger 的 UUID 表示独立调用，不做流式去重。

未知 schema、无效时间、缺少身份字段、负 token 或 `cachedTokens > inputTokens` 会计入解析错误并跳过，不会生成错误统计。

---

## OpenCode

### 数据库与双 schema

OpenCode 默认写入平台 data directory 下的 `opencode/opencode.db`；非标准安装 channel 可能写入 `opencode-<channel>.db`。`OPENCODE_DB` 可指定绝对路径，或指定 OpenCode data directory 内的相对文件名。

当前数据库可能同时包含两代消息表：

- `message`：role、`modelID`、`providerID` 位于 JSON payload。
- `session_message`：role 位于表的 `type` 列，model/provider 位于 payload 的 `model` 对象。

两表中的 assistant payload 都持久化已经归一化的独立 token 桶：

```text
input_tokens       ← tokens.input
output_tokens      ← tokens.output
reasoning_tokens   ← tokens.reasoning
cache_creation     ← tokens.cache.write
cache_read         ← tokens.cache.read
```

OpenCode 写入端已经从 inclusive input 中扣除了 cache read/write，并从 output 中扣除了 reasoning；ccstats 不再次减法。正的 `cost` 保存为 client-recorded USD，零值视为缺少可用价格并允许价格层估算。

同一个消息可能同时存在于两张表或多个 channel 数据库。ccstats 使用 source-wide message ID 去重，优先完成记录；session directory 用于项目聚合。损坏 JSON、负 token、无效时间或数据库读取失败都会计入 parse error。

### MiMo Code 与 Kilo CLI

MiMo Code 和 Kilo CLI 复用 OpenCode 消息族的五个独立 token 桶，但目录、schema 和 fork 行为分别由各自当前源码确定。MiMo Code 读取 `message`；Kilo CLI 同时读取 legacy `message` 与当前 `session_message`。官方 fork 不持久化 parent ID，但复制消息会保留原 `message.time.created`，而新 session 有更晚的 `session.time_created`。ccstats 只把“消息早于所属 session”视为 copy 候选，并要求同数据库中同模型、同毫秒时间和同五桶的唯一原 session 同时存在才去重。directory 不参与 key，因为官方允许跨目录 fork；原 session 后来被删除时，两份以上相同 copy 仍归一为一份。这样保留 fork 后的新调用，也避免普通的相似调用被误删。

MiMo Code 和 Kilo CLI 的持久化 `cost` 是来源记录值，可能来自上游响应或本地计算；ccstats 保留其 provenance，不把它描述成 provider 发票。与 OpenCode 不同，这两个 fork 的显式零值有格式语义并会保留。数据库 schema 缺失、session creation time 读取失败、非法成本或 token 会明确计入 parse error。

---

## Pi

### JSONL 与独立调用

Pi 默认写入 `~/.pi/agent/sessions/<encoded-cwd>/*.jsonl`。显式 `PI_CODING_AGENT_SESSION_DIR` 优先，其次是 `PI_CODING_AGENT_DIR/sessions`。

统计四类带 usage 的真实 LLM 调用：

1. assistant message；
2. compaction summary；
3. branch summary；
4. `toolResult.message.usage` 中的子调用汇总。

```text
input_tokens       ← usage.input
output_tokens      ← usage.output
reasoning_tokens   ← 0
cache_creation     ← usage.cacheWrite
cache_read         ← usage.cacheRead
```

Pi 官方定义明确说明 `usage.reasoning` 已包含在 `usage.output` 中，所以不能再放入 additive reasoning 桶。summary 记录没有自己的 model 字段时，使用此前最近一次 `model_change` 或 assistant 明确记录的模型；仍无法确定时明确归入 `unknown`。

Pi 的“创建分支 session”会把已有路径复制到新 JSONL，但保留原 entry ID。ccstats 以 source-wide entry ID 去重，既保留新分支之后产生的调用，也不会把复制的历史再次计费。正的 `usage.cost.total` 保存为 client-recorded USD。

### Senpi 与 Kimchi

Senpi 使用 Pi v3 的四类 usage carrier 和相同的 branch-copy entry ID，因此沿用相同的 token 归一化与 source-wide 去重。发现顺序支持显式 session/agent env、从当前目录向上查找的非 symlink 项目 `.senpi/agent`、project/global `settings.jsonc` 或 `settings.json` 的 `sessionDir`，以及 home 默认目录；JSONC 支持 BOM、注释和尾逗号，project 的 null/空串会重置 global 值，路径会展开 `~`。一次性的 `--session-dir` 不会持久化，需把同值传给 `SENPI_CODING_AGENT_SESSION_DIR`。

Kimchi 的 child transcript 是真实独立调用，而 parent tool result 的 `details.tokenUsage` 是同一 child 的累计回传。`details.sessionFile` 指向且 child 能通过正式 session/header/timestamp/cost 校验产出 `RawEntry` 时，ccstats 统计 child 并忽略 parent rollup；只有 header、无有效 usage、child 未落盘或 remote agent 没有 session file 时，统计 parent details 作为权威 fallback。Kimchi launcher 固定 session 路径，因此不暴露一个实际上不会生效的目录覆盖项。

### Gajae Code、Prime Agent 与 Oh My Pi

三者虽然继承 Pi JSONL 外形，但计量所有权不同，因此只共享基础字段映射，不共享 rollup 决策。

- Gajae Code 只接受 v5 session，先顺序重放 `header_patch` 与 `entry_patch`。assistant 的 `reasoningTokens` 是 output 子集；task child transcript 位于父 JSONL 同名目录。只从 task 总量扣除已有直接 child 的 `results[].usage`，孙级 transcript 独立计数；缺失 child 留在无调用次数的 `unknown` residual。fork 没有复制 artifact 时沿 `parentSession` 查找原 child，避免重复计入 residual。
- Prime Agent 只接受 v3 session。对同一个 `targetId`，父调用 own usage 等于最后一条 `aggregateUsage` 减去全部 `childUsage`；child transcript 仍递归独立统计。若 fork 在 attribution 记录之前创建，先沿 `parentSession` 从原文件恢复 own usage。完成状态重建后，以 entry id、message timestamp、provider、model、token 向量和 recorded cost 组成 source-wide fork 指纹。
- Oh My Pi 只接受 v3 session，按 `OMP_PROFILE`（其次 `PI_PROFILE`）选择 default 或 named profile；显式 default 不继承较低优先级 named profile 派生的 `PI_CODING_AGENT_DIR`。`message.details.usage` 和 `results[].usage` 都是 child rollup，递归扫描时一律忽略；只统计 child/advisor transcript 的 assistant。`orchestration.input/output/cacheRead` 是额外可计费桶，分别加到主 usage，reasoning 仍从 output 中拆除。

三者的 `cttl.ephemeral1h` 是 cache write 子桶，不额外增加 token 总量。GJC 的 `cost.total` 是必填计算结果，显式零值也保留；Prime/OMP 只保留正值，零值交给价格层。它们都是客户端按模型目录计算的 API 等价成本，不声明为供应商最终账单。负 token、reasoning/TTL 越界、无效 cost、错误 session 版本或非法 profile 都进入 data-quality parse error。

---

## GitHub Copilot CLI

### OpenTelemetry chat span

Copilot CLI 默认不写 token 文件。设置 `COPILOT_OTEL_FILE_EXPORTER_PATH` 后，官方 file exporter 把所有 OTel signal 写成 JSONL。ccstats 只统计 `type = "span"` 且 `gen_ai.operation.name = "chat"` 的记录，因为官方定义一条 `chat` span 对应一次 LLM 请求；`invoke_agent` 是多个 child call 的累计摘要，不能再次相加。

OTel 的 cache 与 reasoning 是总桶的子集，进入 ccstats 前必须拆成互不重叠的桶：

```text
input_tokens       ← input_total - cache_read - cache_creation
output_tokens      ← output_total - reasoning
reasoning_tokens   ← gen_ai.usage.reasoning.output_tokens
cache_creation     ← gen_ai.usage.cache_creation.input_tokens
cache_read         ← gen_ai.usage.cache_read.input_tokens
```

若 cache read/write 之和超过 input，或 reasoning 超过 output，说明权威字段互相冲突；该记录计入 parse error，不做静默 clamp。模型优先使用 response model，其次 request model；session 使用 conversation ID，缺失时才退回合法 W3C trace ID。合法 trace/span identity 组成 source-wide 去重键，避免 exporter 文件副本或轮转产生重复。

官方还提供 `github.copilot.cost` 与 AIU，但文档没有声明 monetary cost 的货币代码。当前不会把它无条件写成 USD；费用列继续显示模型 API 等价估算，不能解释成 Copilot 最终账单。

---

## Goose

### 当前 per-call usage ledger

Goose schema v15+ 的 `usage_ledger` 才是逐次调用账本。ccstats 将其与 `sessions` 连接以取得 working directory 和 model fallback，使用 ledger 自己的 timestamp、model、cache 与 cost provenance。`sessions.accumulated_*` 是累计状态，`sessions.input/output` 还会在 compaction 后变成当前上下文；它们不能再作为独立调用相加。

Goose 官方 `Usage.input_tokens` 包含 cache read/write，所以归一化为：

```text
input_tokens       ← ledger.input_tokens - cache_read_tokens - cache_write_tokens
output_tokens      ← ledger.output_tokens
reasoning_tokens   ← 0
cache_creation     ← ledger.cache_write_tokens
cache_read         ← ledger.cache_read_tokens
```

Goose 没有 reasoning 字段，不能像 Tokscale 一样用 `total-input-output` 猜测。`cost_source = provider_reported` 的正 cost 保存为 client-recorded USD；`estimated` cost 继续由 ccstats 价格层统一估算，避免把两套估值混加。数据库读取失败、未知 schema、负 token、cache 子桶越界或无效时间都计入 parse error。

旧会话只有 accumulated totals、但尚未被 Goose 自己写入 carried-forward ledger 时，无法恢复逐调用日期和模型。ccstats 不把这类累计值伪装成某天的一次调用；这是当前格式的明确边界。

---

## OpenClaw

OpenClaw 的当前 session transcript 可能位于
`$OPENCLAW_STATE_DIR/agents/<agent>/sessions/`，也可能写入
`agents/<agent>/agent/openclaw-agent.sqlite` 的 `transcript_events` 与
`session_transcript_archives`。默认 state root 是 `$OPENCLAW_HOME/.openclaw`，
其次使用系统 home；`OPENCLAW_STATE_DIR` 优先并按 effective home 展开 `~`。
文件冷归档只读取 primary `.jsonl` 以及 `.jsonl.reset.*`、
`.jsonl.deleted.*`（含 zstd），明确排除 checkpoint、trajectory 和 backup。
`openclaw.json` 中有效的 `session.store` 与 `agents.list[].agentDir` 也进入发现；
`OPENCLAW_CONFIG_PATH` 可覆盖配置文件位置。单个坏 archive 只增加 parse error，
同一 SQLite store 中已经成功读取的 active transcript 仍会保留。
ccstats 只读取 `type = message` 且 role 为 assistant 的 usage；
`model_change` 与 `custom/model-snapshot` 只更新后续消息的 model/provider
上下文。session header 的 `cwd` 归属项目。

OpenClaw 的五个 usage 字段已经互不重叠：

```text
input_tokens       ← usage.input
output_tokens      ← usage.output
cache_creation     ← usage.cacheWrite
cache_creation_1h  ← usage.cacheWrite1h（且必须不大于 cacheWrite）
cache_read         ← usage.cacheRead
reasoning_tokens   ← 0（当前持久化 schema 没有独立 reasoning 桶）
```

session fork 可能复制历史 entry，但 entry id 保持不变。因此 message id 使用
source-wide entry id，跨 transcript 只保留一次真实调用。费用只有在
`usage.cost.totalOrigin = provider-billed` 时作为 recorded USD；没有该标记的
`cost.total` 是 OpenClaw 根据本地 model catalog 计算的估值，继续由 ccstats
价格层统一处理。显式 provider-billed 零值保留。

## Xum

Xum 是 coder/mux 当前产品名；当前 canonical root 是 `XUM_ROOT`，默认
`~/.xum`。ccstats 读取 `sessions/<workspace>/session-usage.json`，不再扫描旧
Mux root。`byModel` 的五个 component 原生互斥：input、cached、cacheCreate、
output 和 reasoning 可以直接映射为 ccstats 的五个 additive bucket。

Xum 删除 child workspace 时会先把 child `byModel` 累加到 parent，再在
`rolledUpFrom` 写幂等记录。若 parent 与 child snapshot 同时存在，统计两者会
双计。ccstats 在同一个 Xum root 内建立 roll-up 引用关系，只允许 schema、
token 与 cost 都合法且确实含用量的 parent 压掉 child。无效 parent 不具有
所有权；roll-up 环会产生 parse error，并在每个强连通环中只保留字典序最小
的 canonical ledger，避免静默删空或双计。三层无环链只留下最高仍存在的
累计账本。

五个 bucket 的 `cost_usd` 是 Xum 持久化的计费结果：五桶成本全部存在、有限、
非负时始终求和锁定，即使 aggregate 同时带有 `costsIncluded = true`，因为同一
模型可能混合 included 与付费调用。缺桶时禁止部分成本与估价混加；此时
`costsIncluded = true` 才作为明确的订阅/路由覆盖语义保留 recorded zero。
累计 snapshot 没有逐调用次数和逐调用时间，因此
`call_count = 0`，整份累计值归到 `lastRequest.timestamp`；这是格式本身的
日期精度边界。

## Hermes Agent

Hermes 当前账本位于 `$HERMES_HOME/state.db` 的
`session_model_usage`，并以 `sessions` 的当前 aggregate 补齐尚未进入细分表的
residual。复合主键包含 session、model、billing provider、base
URL、billing mode 与 task；ccstats 每个持久化 row 独立产出一条记录，不把
这些计费维度过早合并。调用次数来自 `api_call_count`，时间优先使用
`first_seen`，项目来自 `sessions.cwd`。

Hermes `CanonicalUsage` 中 cache read/write 是 input 之外的独立 prompt 桶，
但 reasoning 是 output 的子集，因此：

```text
input_tokens       ← input_tokens
output_tokens      ← output_tokens - reasoning_tokens
reasoning_tokens   ← reasoning_tokens
cache_creation     ← cache_write_tokens
cache_read         ← cache_read_tokens
```

`cost_status = actual` 使用 `actual_cost_usd`，`included` 保存显式零；
`estimated` 与 `unknown` 交给 ccstats 价格层。当前实现只接受官方当前 schema；
缺表、字段漂移、负数、reasoning 越界、未知 cost status 或无效时间都作为
parse error。session residual 对 input/cache/calls/cost 逐项相减；output 先拆成
`visible = output - reasoning`，分别 reconcile visible 与 reasoning 后再重建
inclusive output，避免两边各自合法却生成 `reasoning > output` 的残差。只有
成功产出 entry 的细分 row 才能从 aggregate 扣除；空 session model 归为
`unknown`。已有合法细分 row 始终保留，因此不会把 task/billing 维度合并回
session。

---

## Reasonix

Reasonix 的权威数据源是 state root 下的
`stats/YYYY-MM-DD.jsonl`，不是 session transcript。state root 优先级为
`REASONIX_STATE_HOME` > `REASONIX_HOME` > `~/.reasonix`；环境变量支持
`${VAR}`、`${VAR:-default}`、`~` 和相对当前目录的路径。只有文件名是
合法日期的 JSONL 才会被发现；`turn=true` 是轮次标记，不是一次
provider call。该账本没有可验证的 session 或 project 归属，ccstats 不从
路径伪造这些维度。

```text
input_tokens       ← cache_miss > 0 ? cache_miss : prompt - cache_hit
output_tokens      ← completion - reasoning
reasoning_tokens   ← reasoning
cache_creation     ← 0（账本没有持久化 cache write）
cache_read         ← cache_hit
call_count         ← requests > 0 ? requests : 1
```

显式正数 `cache_miss` 是普通输入的权威值；旧记录没有该值时才用
`prompt - cache_hit`。request-only row 是合法记录。负数、reasoning 超过
completion、fallback 路径上 cache hit 超过 prompt、坏时间或坏 JSON
都计入 parse error，不钳制为零。Reasonix 明确允许 provider-specific `total`
不包含某些 cache token，因此它只用于判断记录是否含用量，不与互斥 bucket
强制相等。但若 `total > 0` 而所有可分解 bucket 均为零，该行无法安全归一化，
会明确报错。

Reasonix 的 cost quote 是记录发生时根据当时价格表生成的客户端估值，
不是 provider invoice。只有 `cost_complete=true` 时才使用，优先顺序是
`valuation_usd`、原始 USD amount、完整的 selected USD amount；显式零值保留。
它作为 source-recorded USD cost 锁定历史口径，避免后续价格表改变后重估。

## Vercel Fx

Fx 的权威全局账本是 `~/.fx/usage.jsonl` 中的 generation fact。
`sessions/*/usage-v2.json` 是会话恢复用的累计 snapshot/sidecar；扫描它会把同一
份 usage 重复统计，并把跨日用量漂移到 session 更新日期，所以不能泛扫。
唯一例外是官方 `~/.fx/usage-recovery/` registry 明确标记的 session。ccstats
先验证 `authority.json`，再按对应 generation 的 commit watermark 重放
`events.jsonl` 提交边界；`state_replacement_*` 事务必须通过连续序号、canonical
base64、chunk/整体 SHA-256、commit 和 immutable identity 校验。managed recovery
文件还必须是单链接的私有普通文件。只有 canonical durable usage 可加载、marker 时间边界合法，
并且 `usage-v2.json` 的完整快照通过聚合校验后，才读取 `publication_backlog`、
pending 和 incident，并以 generation ID 与 profile ledger 去重。billing projection
与 durable usage 不一致时，按 Fx 官方逻辑保留已验证的 recovery hints，但把连续性
标为不完整；缺少 canonical session 的 sidecar 会明确报错且不产生用量。
Fx 没有 `FX_HOME` 或 XDG 路径覆盖。

```text
input_tokens       ← fact.input_tokens - cache_read_tokens - cache_write_tokens
output_tokens      ← fact.output_tokens - reasoning_tokens
reasoning_tokens   ← reasoning_tokens
cache_creation     ← cache_write_tokens
cache_read         ← cache_read_tokens
recorded_cost_usd  ← total_cost（包括明示的订阅零成本）
```

generation ID 是全局去重键。完全相同的重复 fact 忽略；同 ID 冲突时
保留第一个合法 fact 并记一个 parse error。pending 在同 ID generation
到达后解除；未解除 pending 和 incident 都进入数据质量。coverage 记录
是账本完整性边界；没有 coverage 时不把 generation 伪装成完整历史。

Fx profile ledger 当前只保留约 35 天，因此 `all-time` 只能表示本地账本
仍保留的时间窗。官方源码还显示 child/subagent usage 的持久化链条可能不完整；
这是根据当前实现得出的推断，parser 无法从已丢失的 fact 反推。Tokscale 直接
相加 inclusive input/output 与 cache/reasoning，示例会把真实 155 计成 190；
ccstats 在 parser 边界先拆分父子 bucket。

---

## Cursor

### 数据来源

Cursor 用量来自 usage events API，不再读取本地 `state.vscdb`。

- Enterprise：`CURSOR_API_KEY` → `POST https://api.cursor.com/teams/filtered-usage-events`
- 个人 / self-serve：`CURSOR_SESSION_TOKEN`（`WorkosCursorSessionToken` cookie）→ `POST https://cursor.com/api/dashboard/get-filtered-usage-events`
- 测试或离线回放：`CURSOR_USAGE_FILE` 指向已保存的 JSON

```
CURSOR_API_KEY=... ccstats daily --source cursor
CURSOR_SESSION_TOKEN=... ccstats daily --source cursor
CURSOR_USAGE_FILE=/path/to/usage.json ccstats daily --source cursor
```

### 原始字段

```json
{
  "timestamp": "1770372000000",
  "model": "claude-4-sonnet",
  "conversationId": "composer-1",
  "tokenUsage": {
    "inputTokens": 100,
    "outputTokens": 40,
    "cacheWriteTokens": 8,
    "cacheReadTokens": 12
  },
  "chargedCents": 12.5
}
```

Parser 同时接受 Admin API 的 `usageEvents` 和 dashboard 的 `usageEventsDisplay`。

### 字段映射

```
input_tokens       ← tokenUsage.inputTokens
output_tokens      ← tokenUsage.outputTokens
cache_creation     ← tokenUsage.cacheWriteTokens
cache_read         ← tokenUsage.cacheReadTokens
reasoning_tokens   ← 0
recorded_cost_usd  ← chargedCents / 100（字段存在时）
session_id         ← conversationId
```

如果一条事件没有显式 token 且 `chargedCents` 也为 0 或缺失，parser 会跳过该记录。负 token 钳制为 0。

### 去重

同一页或跨页中 timestamp/model/token/cost 完全相同的事件按 `message_id` 去重。数据源能力中 `needs_dedup=false`。

### 限制

- 不支持项目聚合和估算 session 窗口（`blocks`）。
- 个人 dashboard API 不是公开稳定接口，session cookie 会过期。
- Self-serve 计划可能只返回 token、事件成本为 0；ccstats 记录该 billed 金额，不用 LiteLLM 估算 Cursor 订阅费用。
- 默认抓取当前计费周期（dashboard）或最近 90 天（Admin API）。

---

## 费用计算

### 价格来源

1. **LiteLLM 在线数据**：从 LiteLLM 获取所有模型的最新价格，缓存 24 小时
2. **内置 fallback**：离线或未匹配时使用内置价格表

### 价格字段映射

| 统一字段 | LiteLLM 字段 | 说明 |
|----------|-------------|------|
| `input_price` | `input_cost_per_token` | 每 token 输入价格 |
| `output_price` | `output_cost_per_token` | 每 token 输出价格 |
| `reasoning_price` | `reasoning_output_cost_per_token` | 推理 token 价格，未提供则回退到 output 价格 |
| `cache_create_price` | `cache_creation_input_token_cost` | 缓存创建价格 |
| `cache_read_price` | `cache_read_input_token_cost` | 缓存读取价格 |

### 模型匹配

1. 精确匹配模型名
2. 尝试加 `claude-` 前缀匹配
3. 子字符串模糊匹配（最长匹配优先）
4. 未匹配时使用 fallback 价格表（按模型系列分层）

---

## 时区与日期分桶

- 默认使用**系统本地时区**将 UTC 时间戳转为日期
- 可通过 `--timezone UTC` 指定 UTC 分桶
- 日期格式：`YYYY-MM-DD`

---

## 准确性说明

### 已知限制（无法通过本地日志解决）

- 如果 API 调用已计费但日志未写入磁盘（进程崩溃），本地无法恢复
- 如果流式响应中断且 `stop_reason` 缺失，使用最后一条记录近似

### 精度保证

| 机制 | 效果 |
|------|------|
| 全局 message.id 去重 (Claude) | 消除流式重复和 subagent 跨文件重复 |
| 累积值转增量 (Codex) | 避免重复计数 |
| 字段分离归一化 | 消除 API 字段包含关系导致的重复计算 |
| 每种 token 独立计价 | 精确匹配各 token 类型的单价 |

---

## 添加新数据源

实现 `Source` trait 时，parser 必须保证输出的 `RawEntry` 中 5 个 token 字段**互不重叠**：

1. 研究目标 API 的字段语义，确认是否存在包含关系
2. 在 parser 层做必要的减法分离
3. 添加集成测试验证 `total_tokens` 无重复计算
4. 设置 `Capabilities` 中的 `needs_dedup` 标志
