# Token Accounting Algorithm

ccstats 从本地 JSONL 日志中统计 token 用量和费用。不同 AI 工具的 API 在字段语义上有本质差异，每个数据源的 parser 负责将原始字段**归一化为互不重叠的 5 个维度**，之后统一计算。

> 本文档为 ccstats 统计算法的权威参考。所有数值均为本地日志的最佳近似，绝对准确值以服务端账单为准。

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

1. 以 `message.id` 为全局去重键（跨主文件和 subagent 文件）
2. 同一 `message.id` 的多条记录，选择规则：
   - 优先选有 `stop_reason` 的（表示完成），取最早的一条
   - 若都没有 `stop_reason`，取最晚的一条（最佳近似）
3. 没有 `message.id` 的条目：仅当有 `stop_reason` 时才计入

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
