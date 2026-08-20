# Token Accounting Algorithm

ccstats 从本地 JSONL 日志中统计 token 用量和费用。不同 AI 工具的 API 在字段语义上有本质差异，每个数据源的 parser 负责将原始字段**归一化为互不重叠的 5 个维度**，之后统一计算。

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
| `source` | string | `claude` / `codex` / `cursor` / `grok` / `all` 或别名 |

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
| OpenAI Codex | `CODEX_HOME` | 包含 `sessions/` 的 Codex 根目录 | `~/.codex` |
| Cursor | `CURSOR_API_KEY` / `CURSOR_SESSION_TOKEN` | Cursor usage API credentials | 无默认值；可用 `CURSOR_USAGE_FILE` 回放 |
| Grok | `GROK_HOME` | 包含 `sessions/` 的 Grok 根目录 | `~/.grok` |

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

Codex 不需要去重。每条 `event_msg` 类型为 `token_count` 的记录已经是独立的计数事件。

### 模型获取

模型名从多个位置按优先级获取：
1. `payload.info.model`
2. `payload.info.model_name`
3. `payload.info.metadata.model`
4. `payload.model`
5. 上一条 `turn_context` 事件中的模型
6. 默认 `"gpt-5"`

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

- 不支持项目聚合和 5 小时 billing block。
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
