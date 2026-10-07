# ccstats

[English](README.md) · [诊断说明与 JSON/MCP 合同](docs/diagnose.md)

把本地 AI 编程助手日志变成 token、成本和额度报告。支持 29 个数据源；
一个 Rust 命令行程序，不需要 ccstats 账号。未知成本不会显示成 `$0`，
估算和官方额度分开标记。

## 安装与开始

```sh
brew install majiayu000/tap/ccstats
ccstats
```

也可运行 `cargo install ccstats` 或 `cargo binstall ccstats`。默认检测本机
已准备好的数据源；没有可用数据时显示 `doctor`。

## Claude Code 额度为什么用得这么快？

诊断功能已准备在 **v0.10.0** 发布，目前尚未发布；可使用审查分支验证。

```sh
ccstats diagnose
ccstats diagnose --window today
ccstats diagnose --window 7d --json
ccstats diagnose --session SESSION_ID
ccstats diagnose --versions
```

看本地记录中的 cache 写入/读取、模型和端点、主要会话、子 agent、压缩时间。
默认最近 5 小时，对比此前 14 天有记录窗口的中位数。版本比较只在同一完整
模型 ID、同一端点内进行；两边各至少 100 个完整轮次才标记明显变化。
缺失字段不当成零，样本不足不下判断。

官方额度来自已保存的 statusline 快照，保留采集时间和过期标记。
诊断不能推测 Anthropic 的订阅扣费规则，也不能证明客户端版本或压缩导致
额度消耗增加。当前可读取的本机日志不足以复现规划中的历史异常。
[窗口定义、真实输出和全部局限](docs/diagnose.md)。

## 常用报告

```sh
ccstats today --source claude
ccstats weekly --source all --since 20261001 --until 20261007
ccstats limits
ccstats watch --once
ccstats doctor
ccstats session --json --source codex
ccstats mcp --offline
```

`weekly` 和 `monthly` 是对选定历史范围分组，不会自动限定“本周/本月”。
`--json` 导出结构化结果；一般报告支持 `--csv`，诊断只支持文字和 JSON。
`--timezone Asia/Shanghai` 控制本地日期窗口。`--no-cache` 强制重读日志。

## MCP

```sh
claude mcp add ccstats -- ccstats mcp --offline
codex mcp add ccstats -- ccstats mcp --offline
```

只读工具包括 `get_limits`、`get_usage_summary`、`diagnose` 和 `doctor`。
`diagnose` 接受 `window` 和可选 `session`，与 CLI 使用同一报告。
[配置与错误合同](docs/mcp.md)。

## 能做什么、不能做什么

| 需要 | 选择与局限 |
|---|---|
| 当前官方额度 | Claude Code `/usage` 是官方来源；ccstats 展示已采集的快照 |
| 会话和 5 小时窗口用量 | ccusage 有成熟的本地窗口/实时报告；ccstats 也提供 `session` / `blocks` |
| 把 cache、子 agent、压缩、版本放在一起 | `diagnose` 展示本地证据与样本数，不能解释未公开的订阅计费 |
| 菜单栏界面 | 单独的 [QuotaBar](https://github.com/majiayu000/quotabar)；旧 ccstats desktop 已停止发布 |

诊断不会输出 prompt、回答正文、凭证或原始文件路径，但会话 ID 仍需脱敏。
`session --json --details` 是另一个显式导出功能，会包含首条 prompt，分享前
请阅读其合同。[隐私](docs/PRIVACY.md) · [数据源路径](docs/sources.md)
· [发布流程](docs/RELEASING.md)。

## 开发与验证

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --locked
```

MIT License。Rust SDK、架构和完整数据源列表见 [英文 README](README.md)。
