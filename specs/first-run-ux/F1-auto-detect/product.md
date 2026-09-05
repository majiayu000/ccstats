# Product Spec — F1 Auto-detect default source

## Linked Issue

https://github.com/majiayu000/ccstats/issues/164

## 用户问题

无参数或未传 `--source` 时，`resolve_source_name` 固定返回 `"claude"`。本机只有 Codex / Grok / Cursor 的用户第一次运行得到空表或「没有 Claude 数据」，会认为产品坏了。README 已经让人先跑 `doctor` 再 `daily --source all`，但默认命令与文档不一致。

## 目标

- 未显式选源时，使用本机 **已检测或已配置** 的源。
- 零就绪源时，自动展示 doctor，并给出下一步。
- 单源就绪时直接出该源报告（不必写 `--source all`）。
- 多源就绪时走现有 `--source all` 合计。
- 显式 `--source`、源子命令、config `source` 行为不变。

## 非目标

- 不改变 SDK 缺省；SDK 调用方必须继续传 `UsageSource`。
- 不改变 `--source all` 的会计规则（real-only token、estimated-proxy 处理）。
- 不在本 PR 改表头结论行、login、limits、README 大改（只允许改与新默认相关的一两句 Quick Start）。
- 不扫描或上传日志内容；检测继续只用现有只读 `diagnose()`。

## Behavior Invariants

1. 选择顺序：CLI `--source` > 源子命令 hint > config `source` > 自动检测。
2. `doctor` / `sources` 命令本身不走自动检测（它们列出全部注册源）。
3. 自动检测的就绪集合 = `diagnose().status` ∈ {Detected, Configured}。
4. 就绪 0 个：stdout 打印与 `ccstats doctor` 相同的表（或 JSON/CSV 等价物），文案说明「没有检测到用量数据」，退出 0。
5. 就绪 1 个：`dispatch_command` 使用该源名。
6. 就绪 ≥ 2 个：源名为 `all`，走 `handle_all_sources_command`。
7. `--source-breakdown` 仍只在最终源名为 `all` 时合法。
8. 现有 `ccstats daily --source claude` 与 `ccstats codex daily` 金测试必须继续通过。
9. 对未就绪源，自动检测不得去 parse 其日志、不得打 Cursor API。

## 验收标准

- [ ] 空 HOME、无凭证：`ccstats` / `ccstats daily` 输出 doctor 诊断，不出现 Claude 空用量表。
- [ ] 只有 Codex fixture：无 `--source` 的 `daily`/`today` 产出 Codex 报告。
- [ ] Claude + Codex 同时就绪：无 `--source` 的 `daily` 走 all-sources 合计。
- [ ] `--source claude` 在只有 Codex 数据时仍只查 Claude（可为空），不改道。
- [ ] config `source = "codex"` 在多源机器上仍只出 Codex。
- [ ] `ccstats doctor` 输出与今日一致（不因 F1 改变诊断语义）。
- [ ] JSON/CSV 空检测不把 doctor 行混进用量 schema；用量命令在 0 源时输出 doctor 的 json/csv 或明确的空状态对象（测试钉死一种）。

## 边界情况

- 仅 Cursor `Configured`（有 token、doctor 未联系 API）：算就绪，默认会打 Cursor API（与今天显式 `--source cursor` 相同）。
- `CURSOR_USAGE_FILE` 指向存在的文件：Detected，算就绪。
- statusline 无源：自动检测；0 源时 statusline 应保持安静（单行空/占位），不要把 29 行 doctor 打进 prompt。
- `--source all` 显式传入：即使只有 1 个就绪源，仍走 all 路径（与今天一致）。
- 源子命令与 `--source` 冲突：保持现有错误退出。

## 发布说明

Breaking 默认：无参数 CLI 不再假设 Claude。钉死源的用户应设 `--source` 或 config `source`。
