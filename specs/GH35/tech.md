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
| 成本语义 | `src/core/types.rs`, `src/source/mod.rs`, SDK summary types | 当前 token/cost aggregate 没有 cost kind/provenance | 需要随 entry/aggregate 传播，不应只用 source-wide bool |
| 预算 | `src/output/budget.rs` | 基于聚合成本预测 | 需排除估算成本 |
| Statusline/top/SDK | `src/output/statusline.rs`, `src/output/top.rs`, `src/sdk.rs` | 也会暴露 cost 或 cost shares | 不能遗漏非 period 成本路径 |

## 设计方案

1. 引入 `CostKind`/`CostProvenance`（命名由实现决定），至少支持 `Real` 与 `EstimatedProxy`，并能在 aggregate 层合成 `Mixed`。
2. Grok 当前 parser 产出的 context-token proxy entries 标记为 `EstimatedProxy`；Claude/Codex/Cursor 当前 entries 标记为 `Real`。Source capability 可作为默认值，但不能是唯一事实来源。
3. all-source period 聚合按 cost kind 分流：real cost 进入现有 total cost；estimated proxy cost 进入同一 row schema 的新增字段/列，不替换 JSON array root，也不破坏 monthly-budget array post-processing。
4. 单源 grok 视图：table 脚注或列头、JSON row field、CSV column、SDK summary field 均标注 estimated/proxy cost。
5. `statusline --source all`、`top --source all`、budget/forecast、SDK totals 均使用 real cost 作为真实总额，并把 estimated proxy cost 作为独立字段或明确排除。

## Product-to-Test Mapping

| Product invariant | Implementation area | Verification |
| --- | --- | --- |
| P1 all 不混入 | `app.rs` + aggregate cost kind | 集成测试：claude+grok fixtures，断言总数与现有 row shape 中的 estimated 字段 |
| P2 单源标注 | `output/*`, `sdk.rs` | grok 单源 table/JSON/CSV/SDK 测试断言标记 |
| P3 token 呈现不变 | `app.rs` | 现有断言 |
| P4 机器可区分 | `output/json.rs`,`csv.rs`,`statusline.rs`,`top.rs`,`sdk.rs` | 字段断言 |

## 数据流

parser token 提取不变，但 entry/aggregate 带 cost kind → 聚合按 `CostKind` 分流 real/estimated 累加器 → 输出层在现有 envelope/row shape 中渲染双字段。无持久化变化。

## 备选方案

- 完全排除 Grok 成本不显示：丢信息，用户失去上下文规模的成本感——拒绝
- 仅加脚注不分流总数：总数仍失真，未解决核心问题——拒绝
- 硬编码 `name() == "grok"` 判断：与 GH-45 反模式一致——拒绝，用能力位

## 风险

- Security: 无
- Compatibility: `all` 总成本变小；JSON/CSV 新增字段但不改变 JSON array root 或 CSV header position semantics。CHANGELOG 说明
- Performance: 可忽略
- Maintenance: GH-18（Grok 真实 usage）落地时同源可按 entry cost kind 混合 real/proxy records，不需要全源 boolean 翻转

## 测试计划

- [ ] Unit tests: 聚合分流逻辑
- [ ] Integration tests: claude+grok 混合 fixtures 覆盖 JSON/CSV/table/statusline/top；仅 grok 场景；budget/SDK 排除或独立字段断言
- [ ] Manual verification: 真实数据运行 `ccstats all monthly` 对比变更前后

## 回滚方案

revert 单 PR；无迁移。
