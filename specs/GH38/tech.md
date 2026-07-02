# Tech Spec

## Linked Issue

GH-38

## Product Spec

`specs/GH38/product.md`

## Codebase Context

| Area | Files | Current behavior | Why relevant |
| --- | --- | --- | --- |
| 分派 | `src/app.rs`（handle_session 62-95、handle_project 104-136、handle_blocks 145-177、handle_top 191-211、handle_tools 254-262、handle_statusline 359-376、render_period_result 389-470、handle_sources 271-353） | 每处 `if cli.csv / else if cli.json / else` | 全部收敛点 |
| 格式函数 | `src/output/mod.rs:14-34` 及各文件 | 每形状三件套函数（output_*_csv/_json/print_*_table） | 成为 trait 实现体 |
| CLI | `src/cli/args.rs` | `csv: bool`, `json: bool` | 派生 OutputFormat 的来源 |

## 设计方案

1. `src/cli/args.rs` 增加 `impl Cli { fn output_format(&self) -> OutputFormat }`，`enum OutputFormat { Table, Json, Csv }` 放 `src/output/mod.rs`（供 output 层使用，cli 依赖 output 方向不变）。
2. 每个数据形状定义一个渲染入口：`fn render_session(format, data, ctx)` 内部 `match format` 调既有三件套——第一步不动函数体只收敛分派；后续形状多时再考虑 trait。
3. handler 改为单行调用渲染入口。`handle_sources` 补齐 CSV/text 列集。
4. statusline 保持独立路径（其"格式"语义不同），仅文档注明。

刻意保守：先枚举+match 收敛（机械、可逐 handler 提交），trait 化留待新格式真实出现时（U-02：不为单次使用抽象）。

## Product-to-Test Mapping

| Product invariant | Implementation area | Verification |
| --- | --- | --- |
| P1 输出不变 | 全部 handler | 现有集成测试逐字节断言 |
| P3 列集一致 | `app.rs` handle_sources | 新增断言三格式列集 |
| P4 单一分派 | `app.rs` | `rg` 证明 + code review |

## 数据流

不变；纯控制流重构。

## 备选方案

- 直接上 `trait Render` 每形状实现：更"完整"但当前只有 3 格式 × 7 形状，trait 的间接性收益不足（过度设计风险）——列为二期
- 维持现状：GH-33 等横切需求将继续按 8 处改——拒绝

## 风险

- Security: 无
- Compatibility: 逐字节输出回归风险 → 依赖现有 1878 行集成测试做金标准，逐 handler 小步提交
- Performance: 无
- Maintenance: 显著改善

## 测试计划

- [ ] Unit tests: `output_format()` 派生逻辑（含 json+csv 冲突）
- [ ] Integration tests: 现有全量通过；sources 列集新断言
- [ ] Manual verification: 三格式抽查 daily/session/top

## 回滚方案

纯重构可整体 revert；逐 handler 提交使问题可二分定位。
