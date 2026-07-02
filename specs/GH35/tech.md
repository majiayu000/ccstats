# Tech Spec

## Linked Issue

GH-35

## Product Spec

`specs/GH35/product.md`

## Codebase Context

| Area | Files | Current behavior | Why relevant |
| --- | --- | --- | --- |
| Grok 解析 | `src/source/grok/parser.rs:149-152,353` | context tokens 报为 input_tokens，output 0 | 合成语义的源头（不改） |
| Grok 定价 | `src/pricing/resolver/fallback.rs:58-61` | 按计费单价折算 | 估算成本产生点 |
| all 聚合 | `src/app.rs:579-647` | `handle_all_sources_command` 合并每源 daily map 并加总成本 | 语义混合发生地，主要改动点 |
| 能力位 | `src/source/mod.rs`（`Capabilities`） | 无成本语义标记 | 建议加 `cost_is_estimated: bool` 避免硬编码源名 |
| 预算 | `src/output/budget.rs` | 基于聚合成本预测 | 需排除估算成本 |

## 设计方案

1. `Capabilities` 增加 `cost_is_estimated: bool`（grok=true，其余 false）——遵循能力驱动模式（与 GH-45 方向一致但不依赖它）。
2. `handle_all_sources_command` 聚合时按该位分流：真实成本进总数；估算成本累入独立字段 `estimated_cost`。
3. 输出层：表格总计行下方追加 "Grok (estimated): $X" 行；JSON 顶层 `{"total_cost": ..., "estimated_cost": {"grok": ...}}`；CSV 独立行。
4. 单源 grok 视图：表格脚注 + JSON `cost_estimated: true`。
5. `monthly-budget` 只用真实成本。

## Product-to-Test Mapping

| Product invariant | Implementation area | Verification |
| --- | --- | --- |
| P1 all 不混入 | `app.rs` | 集成测试：claude+grok fixtures，断言总数与独立行 |
| P2 单源标注 | `output/*` | grok 单源测试断言标记 |
| P3 token 呈现不变 | `app.rs` | 现有断言 |
| P4 机器可区分 | `output/json.rs`,`csv.rs` | 字段断言 |

## 数据流

parser 不变 → 聚合按 `cost_is_estimated` 分流两个累加器 → 输出层双字段渲染。无持久化变化。

## 备选方案

- 完全排除 Grok 成本不显示：丢信息，用户失去上下文规模的成本感——拒绝
- 仅加脚注不分流总数：总数仍失真，未解决核心问题——拒绝
- 硬编码 `name() == "grok"` 判断：与 GH-45 反模式一致——拒绝，用能力位

## 风险

- Security: 无
- Compatibility: `all` 总成本变小；JSON 新增字段。CHANGELOG 说明
- Performance: 可忽略
- Maintenance: GH-18（Grok 真实 usage）落地时把 grok 的能力位翻回 false 即完成切换

## 测试计划

- [ ] Unit tests: 聚合分流逻辑
- [ ] Integration tests: claude+grok 混合 fixtures 三种格式；仅 grok 场景；budget 排除断言
- [ ] Manual verification: 真实数据运行 `ccstats all monthly` 对比变更前后

## 回滚方案

revert 单 PR；无迁移。
