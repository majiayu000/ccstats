# Product Spec

## Linked Issue

GH-38

## 用户问题

对用户不可见、对维护者高成本：输出格式选择以 `cli.csv`/`cli.json` 布尔值在 `app.rs` 约 8 个 handler 中各自三分支，新增一种输出格式需改 10+ 处，且各格式列集已出现漂移（`handle_sources` 的 CSV/text 缺 `has_cache_creation`/`needs_dedup` 两列而 JSON 有）。

## 目标

- 输出格式判定从各 handler 重复分支收敛为统一 `OutputFormat` 派生与 render entrypoints
- 同一数据形状在所有格式下列集一致
- 为 GH-33（pricing_source 注入）等横切输出需求提供单一注入点

## 非目标

- 不新增任何输出格式
- 不改变现有输出的内容、字段名、列顺序（列集漂移修复除外）
- 不改动 statusline 的单行特殊格式

## Behavior Invariants

1. 现有 CLI 标志（`--json`、`--csv`、默认表格）行为与输出逐字节不变（`handle_sources` 列集补齐除外）。
2. `--json` 与 `--csv` 同时给出时的现有优先级/报错行为保持不变。
3. `handle_sources` 三种格式呈现同一列集。
4. 内部实现：格式判定收敛为单一 `OutputFormat` 枚举，每个 handler 只出现一次格式分派。

## 验收标准

- [ ] 现有全部集成测试不改断言通过（列集修复对应的断言除外，单独列明）
- [ ] `rg "if cli.csv" src/app.rs` 无匹配（或等价证明三分支已收敛）
- [ ] 新增格式的后续路径被文档化：本 slice 先消除 handler 级重复分派；真正 trait/plugin 层留到出现第四种真实格式时再做

## 边界情况

- 部分数据形状若未来不支持某格式 → 枚举分派处显式报错，不静默回落表格；现有已支持的 `tools --csv` 必须保持支持
- quiet 模式与格式的组合行为不变

## 发布说明

纯内部重构，无用户可见变化（除 sources 列集补齐，CHANGELOG 一句话说明）。
