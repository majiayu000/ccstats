# Product Spec — First-run UX and decision surface

## Linked Issue

https://github.com/majiayu000/ccstats/issues/163

Child issues: #164 F1, #165 F2, #166 F3, #167 F4, #168 F5.

## 用户问题

ccstats 在 v0.5.2 已经是 29 源本地账本 + CLI + SDK + 桌面，但第一次运行仍默认 Claude。没有 Claude 日志的用户看到空表；Cursor / Codex 用户必须先读 README。默认输出是宽表，没有一句能带走的结论。Cursor 授权要手贴 cookie。额度散落在 `quota`（只属于 Codex）和 `blocks`（Claude 语义）里。

竞品（ccusage、CodeBurn、Tokscale、CodexBar、Claude-Code-Usage-Monitor）把「本机有什么就出什么」和「还剩多少额度」做成了默认路径。ccstats 的会计正确性没有进入这 30 秒。

## 目标

让新用户在不读文档的情况下完成：

1. 第一次运行看到 **自己机器上已有的源**，而不是一张 Claude 空表。
2. 没有任何源时看到 **可执行的安装/授权提示**（doctor），而不是空结果。
3. 默认表输出带一句 **人话结论**（今天多少、是否完整/下限/未知）。
4. Cursor 用户用一条命令完成凭证写入，不必去文档里找 cookie 名。
5. 订阅用户用一条命令看到 **Codex 周额度 + Claude 估算窗口**，措辞不把估算当成官方账单。
6. GitHub / README 第一屏与真实产品一致（不再写「只支持 Claude 和 Codex」）。

## 非目标

- 不删除 `ccstats codex` / `ccstats grok` / `ccstats kimi` 别名（保留，不再为新源增加对称子命令）。
- 不把 SDK `summarize_cost` 的缺省源改成 auto-detect（SDK 必须显式传 `UsageSource`）。
- 不在本轮做菜单栏、桌面信息架构收缩、长尾第 30 个源、排行榜/社交。
- 不把估算额度伪装成官方 Anthropic / Cursor 账单余量。
- 不在未签名桌面安装包上投入获客主路径。

## 功能拆分

| ID | 功能 | 用户可感知结果 | 依赖 |
| --- | --- | --- | --- |
| F1 | 默认使用已检测源；空数据走 doctor | `ccstats` / `ccstats daily` 无 `--source` 时不再默认 Claude | 无 |
| F2 | README 第一屏 + GitHub 仓库身份 | 访客 30 秒内知道装什么、跑什么、产品是什么 | F1 的命令语义（文档可并行起草，合并前对齐） |
| F3 | 默认表输出结论行 | 表上方一句人话 + floor/unknown 标记 | 无（可与 F1 并行；F1 之后 all-source 结论更有价值） |
| F4 | `ccstats login cursor` | 向导写入凭证，doctor 显示 Configured | 无 |
| F5 | `ccstats limits` | Codex quota + Claude blocks 同屏；`quota` 保留为别名 | 无 |

每个功能单独 issue + PR。实现不得把两个功能塞进同一个 PR，除非后者只改前者引入的文档口误。

## 全局不变量

1. CLI `--source`、源子命令 hint（`codex`/`grok`/`kimi`）、config `source` 的优先级高于自动检测。
2. 自动检测只把 `diagnose()` 为 `Detected` 或 `Configured` 的源当作就绪。`Missing` 不进入默认合计。
3. 就绪源为 0 → 打印 doctor 表（与 `ccstats doctor` 相同内容），进程退出码 0（这是引导，不是故障）。JSON/CSV 模式下空检测输出 doctor 的结构化结果，不假装有用量。
4. 就绪源为 1 → 该源，行为与今天 `--source <name>` 相同。
5. 就绪源 ≥ 2 → 与今天 `--source all` 相同（含 real-only token 合计、`--source-breakdown` 规则）。
6. `statusline` 若未指定源，也走自动检测，以便 hook 显示本机真实用量；config 里可钉死 `source`。
7. 未知、下限、估算窗口必须保持可见；禁止把 `null` 显示成 `$0.00`。
8. 不新增网络端点，除非 F4 只打开用户本机浏览器到已文档化的 Cursor dashboard URL（不代发 cookie）。

## 成功标准（90 天产品，本轮交付切片）

- [ ] 新机器只敲 `ccstats`，若本机有 Codex 或 Claude 日志，不必读文档即可看到自己的数。
- [ ] 没有数据时，输出是 doctor 提示，不是空 Claude 表。
- [ ] Cursor 用户可用 `ccstats login cursor` 完成授权（非交互 flag 也必须可用，便于测试）。
- [ ] `ccstats limits` 能同时回答 Codex 周额度和 Claude 估算窗口，并标明估算 vs 官方。
- [ ] GitHub description、README 首屏、默认命令说的是同一件事。

## 发布说明方向

这是一次面向采用的默认路径修复，不是新数据源。changelog 应强调：无参数 CLI 现在汇总本机已检测源；空库进入 doctor；Cursor 增加 login；新增 limits 视图。
