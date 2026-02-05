# ccstats 架构文档

## 概述

ccstats 是一个快速的 CLI 工具，用于分析多种 AI CLI 工具的 token 使用统计。采用插件架构，便于添加新的数据源。

## 目录结构

```
src/
├── cli/                    # 命令行接口
│   ├── args.rs            # CLI 参数定义
│   ├── commands.rs        # 子命令定义
│   └── mod.rs
├── core/                   # 核心共享逻辑
│   ├── types.rs           # 统一数据类型
│   ├── dedup.rs           # 去重算法
│   ├── cache.rs           # 文件缓存管理
│   ├── aggregator.rs      # 聚合函数
│   └── mod.rs
├── source/                 # 数据源插件
│   ├── claude/            # Claude Code 数据源
│   │   ├── config.rs      # Source trait 实现
│   │   ├── parser.rs      # JSONL 解析逻辑
│   │   └── mod.rs
│   ├── codex/             # OpenAI Codex 数据源
│   │   ├── config.rs      # Source trait 实现
│   │   ├── parser.rs      # JSONL 解析逻辑
│   │   └── mod.rs
│   ├── loader.rs          # 统一数据加载器
│   ├── registry.rs        # 数据源注册表
│   └── mod.rs             # Source trait 定义
├── output/                 # 输出格式化
├── pricing/                # 价格计算
├── utils/                  # 工具函数
└── main.rs                 # 程序入口
```

## 核心概念

### Source Trait

所有数据源必须实现 `Source` trait：

```rust
pub trait Source: Send + Sync {
    /// 数据源名称 (用于 CLI 和注册表)
    fn name(&self) -> &'static str;

    /// 显示名称 (用于用户输出)
    fn display_name(&self) -> &'static str;

    /// 别名列表 (例如 "cc" 是 "claude" 的别名)
    fn aliases(&self) -> &'static [&'static str];

    /// 数据源能力声明
    fn capabilities(&self) -> Capabilities;

    /// 发现所有数据文件
    fn find_files(&self) -> Vec<PathBuf>;

    /// 解析单个文件，返回统一的 RawEntry 列表
    fn parse_file(
        &self,
        path: &PathBuf,
        filter: &DateFilter,
        timezone: &Timezone,
    ) -> Vec<RawEntry>;

    /// 缓存文件名
    fn cache_name(&self) -> &'static str;
}
```

### Capabilities (能力声明)

```rust
pub struct Capabilities {
    /// 是否支持项目聚合
    pub has_projects: bool,

    /// 是否支持 5 小时计费块
    pub has_billing_blocks: bool,

    /// 是否有推理 token (如 o1 模型)
    pub has_reasoning_tokens: bool,

    /// 是否需要去重 (流式响应)
    pub needs_dedup: bool,
}
```

### RawEntry (统一数据结构)

所有数据源将其原生格式转换为统一的 `RawEntry`：

```rust
pub struct RawEntry {
    pub timestamp: String,      // UTC 时间戳
    pub timestamp_ms: i64,      // 毫秒时间戳 (用于排序)
    pub date_str: String,       // 本地日期 (YYYY-MM-DD)
    pub message_id: Option<String>,  // 消息 ID (用于去重)
    pub session_id: String,     // 会话 ID
    pub project_path: String,   // 项目路径 (可为空)
    pub model: String,          // 模型名称
    pub input_tokens: i64,      // 输入 token
    pub output_tokens: i64,     // 输出 token
    pub cache_creation: i64,    // 缓存创建 token
    pub cache_read: i64,        // 缓存读取 token
    pub reasoning_tokens: i64,  // 推理 token
    pub stop_reason: Option<String>,  // 停止原因 (用于去重)
}
```

## 数据流

```
┌─────────────────────────────────────────────────────────────────────┐
│                           CLI (main.rs)                             │
└─────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────┐
│                      Source Registry                                 │
│                   (选择数据源: claude/codex)                          │
└─────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────┐
│                        DataLoader                                    │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐              │
│  │ File Discovery│  │   Parsing    │  │    Cache     │              │
│  │  find_files() │  │ parse_file() │  │  get/save    │              │
│  └──────────────┘  └──────────────┘  └──────────────┘              │
└─────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────┐
│                    Deduplication (if needed)                         │
│              (保留有 stop_reason 的完整消息)                           │
└─────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────┐
│                         Aggregation                                  │
│  ┌───────────┐ ┌───────────┐ ┌───────────┐ ┌───────────┐           │
│  │   Daily   │ │  Session  │ │  Project  │ │   Blocks  │           │
│  └───────────┘ └───────────┘ └───────────┘ └───────────┘           │
└─────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────┐
│                    Pricing + Output Formatting                       │
└─────────────────────────────────────────────────────────────────────┘
```

## 添加新数据源

### 1. 创建目录结构

```bash
mkdir -p src/source/newcli
touch src/source/newcli/{mod.rs,config.rs,parser.rs}
```

### 2. 实现 parser.rs

```rust
//! NewCLI JSONL parser

use crate::core::{DateFilter, RawEntry};
use crate::utils::Timezone;
use std::path::PathBuf;

/// 发现数据文件
pub fn find_files() -> Vec<PathBuf> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };

    let data_path = home.join(".newcli").join("sessions");
    let mut files = Vec::new();

    if let Ok(entries) = glob::glob(&format!("{}/**/*.jsonl", data_path.display())) {
        for entry in entries.flatten() {
            files.push(entry);
        }
    }
    files
}

/// 解析单个文件
pub fn parse_file(
    path: &PathBuf,
    filter: &DateFilter,
    timezone: &Timezone,
) -> Vec<RawEntry> {
    // 实现解析逻辑
    // 返回 Vec<RawEntry>
    Vec::new()
}
```

### 3. 实现 config.rs

```rust
//! NewCLI data source configuration

use std::path::PathBuf;
use crate::core::{DateFilter, RawEntry};
use crate::source::{Capabilities, Source};
use crate::utils::Timezone;
use super::parser::{find_files, parse_file};

pub struct NewCliSource;

impl NewCliSource {
    pub fn new() -> Self { Self }
}

impl Default for NewCliSource {
    fn default() -> Self { Self::new() }
}

impl Source for NewCliSource {
    fn name(&self) -> &'static str { "newcli" }
    fn display_name(&self) -> &'static str { "NewCLI" }
    fn aliases(&self) -> &'static [&'static str] { &["nc"] }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            has_projects: false,
            has_billing_blocks: false,
            has_reasoning_tokens: false,
            needs_dedup: false,
        }
    }

    fn find_files(&self) -> Vec<PathBuf> { find_files() }

    fn parse_file(
        &self,
        path: &PathBuf,
        filter: &DateFilter,
        timezone: &Timezone,
    ) -> Vec<RawEntry> {
        parse_file(path, filter, timezone)
    }

    fn cache_name(&self) -> &'static str { "newcli" }
}
```

### 4. 更新 mod.rs

```rust
mod config;
mod parser;

pub use config::NewCliSource;
```

### 5. 注册数据源

在 `src/source/registry.rs` 中添加：

```rust
use super::newcli::NewCliSource;

static SOURCES: LazyLock<Vec<BoxedSource>> = LazyLock::new(|| {
    vec![
        Box::new(ClaudeSource::new()),
        Box::new(CodexSource::new()),
        Box::new(NewCliSource::new()),  // 添加这行
    ]
});
```

### 6. 添加 CLI 命令 (可选)

在 `src/cli/commands.rs` 中添加新的子命令。

## Claude Code 解析算法

```
输入: ~/.claude/projects/**/*.jsonl

每行格式:
{
  "timestamp": "2026-02-05T10:30:00Z",
  "message": {
    "id": "msg_xxx",
    "model": "claude-3-opus-20240229",
    "stop_reason": "end_turn",
    "usage": {
      "input_tokens": 1000,
      "output_tokens": 500,
      "cache_creation_input_tokens": 0,
      "cache_read_input_tokens": 200
    }
  }
}

处理步骤:
1. 文件发现: glob("~/.claude/projects/**/*.jsonl")
2. 并行解析每个文件
3. 提取 session_id (文件名), project_path (父目录名)
4. 规范化模型名称 (去除前缀和日期后缀)
5. 去重: 相同 message_id 保留有 stop_reason 的条目
6. 聚合: daily/session/project/blocks
```

## Codex CLI 解析算法

```
输入: ~/.codex/sessions/**/*.jsonl

每行格式 (token_count 事件):
{
  "timestamp": "2026-02-05T10:30:00Z",
  "type": "event_msg",
  "payload": {
    "type": "token_count",
    "info": {
      "total_token_usage": {
        "input_tokens": 5000,
        "cached_input_tokens": 1000,
        "output_tokens": 2000,
        "reasoning_output_tokens": 500,
        "total_tokens": 7000
      },
      "last_token_usage": { ... },  // 可选
      "model": "gpt-5.2"
    }
  }
}

处理步骤:
1. 文件发现: glob("~/.codex/sessions/**/*.jsonl")
2. 并行解析每个文件
3. 处理 turn_context 事件获取模型信息
4. 处理 event_msg + token_count 事件
5. Delta 计算:
   - 如果有 last_token_usage, 直接使用
   - 否则: delta = total - previous_total
6. 跳过 total 未变化的重复事件
7. input_tokens 包含 cached_input_tokens, 需要减去
8. 无需去重 (Codex 内部已处理)
```

## 缓存机制

```
缓存位置: ~/.cache/ccstats/{source}.json

缓存结构:
{
  "files": {
    "/path/to/file.jsonl": {
      "mtime": 1707123456,
      "size": 12345,
      "entries": [RawEntry, ...]
    }
  }
}

缓存策略:
1. 检查文件 mtime + size
2. 匹配则使用缓存 (仍需按日期过滤)
3. 不匹配则重新解析
4. 处理完成后保存更新的缓存
```

## 性能优化

- 并行文件解析 (rayon)
- 文件级缓存 (mtime + size 验证)
- 延迟加载定价数据
- 流式 JSONL 解析
