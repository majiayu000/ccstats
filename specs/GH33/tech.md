# Tech Spec

## Linked Issue

GH-33

## Product Spec

`specs/GH33/product.md`

## Codebase Context

| Area | Files | Current behavior | Why relevant |
| --- | --- | --- | --- |
| 定价加载 | `src/pricing/db.rs:55-132` | `load_internal` 四分支（fresh cache/live fetch/stale cache/empty+fallback），只以 `eprintln!` 提示且 quiet 时静默 | 来源判定发生地，需要把分支结果结构化返回 |
| fallback 定价 | `src/pricing/resolver/fallback.rs` | 未命中 LiteLLM 时按模型家族硬编码价格 | 每模型粒度的 fallback 命中需要上报以支持 `mixed` |
| 成本序列化 | `src/output/format.rs:109-143` | `cost_json_value`/`format_cost` 输出纯数字 | JSON/CSV 附加 meta 的注入点 |
| 各输出路径 | `src/output/{json,csv,table,statusline,budget,top}.rs` | 各自组装输出 | 需要统一携带 `pricing_source` |
| 缓存 | `src/pricing/cache.rs` | 无年龄查询接口 | 需要暴露缓存 mtime 以计算年龄 |

## 设计方案

1. `PricingDb` 增加字段 `source: PricingSource`（枚举 `Live | Cache | CacheStale | Fallback`），在 `load_internal` 各分支赋值；`cache.rs` 增加返回缓存 mtime 的辅助函数。
2. 逐模型解析时记录是否走了 fallback（`resolve` 返回带来源的结果，或 `PricingDb` 维护 `fallback_hits: bool`）；会话级来源 = db 级来源与逐模型 fallback 命中的合成，命中部分 fallback 时为 `Mixed`。
3. 输出层：`OutputContext`（或等价的现有参数结构）增加 `pricing_source` 字段，JSON 顶层 `"pricing_source": "..."`，CSV 首行注释 `# pricing_source: ...`，表格在 `render_period_result` 等表尾追加脚注行。
4. statusline：单行格式不变；其 JSON 分支携带字段。

## Product-to-Test Mapping

| Product invariant | Implementation area | Verification |
| --- | --- | --- |
| P1 来源枚举 | `pricing/db.rs` | 单测：四分支 + mixed 合成 |
| P2 JSON/CSV 一致 | `output/json.rs`, `output/csv.rs` | 集成测试断言两格式同值 |
| P3 表格脚注 | `output/table.rs` | 集成测试匹配脚注文案 |
| P4 statusline | `output/statusline.rs` | 现有格式断言不变 + JSON 字段新增断言 |
| P5/P6 现有行为 | 全链路 | 现有测试不改断言全过 |

## 数据流

`load_internal` 分支 → `PricingDb.source` + per-model fallback 标记 → 汇总为会话 `pricing_source` → 传入输出层 → JSON meta / CSV 注释 / 表格脚注。无新增持久化与外部调用。

## 备选方案

- 每行（每模型）输出粒度的来源标注：信息更细但输出膨胀，且 CSV 列集变化有破坏性——拒绝，会话级 + `mixed` 已满足决策需求
- 只加 stderr 警告不改结构化输出：不解决管道消费者的问题——拒绝

## 风险

- Security: 无
- Compatibility: JSON 新增顶层字段，严格 schema 的消费者可能报未知字段——在 CHANGELOG 声明；CSV 用注释行避免列集变化
- Performance: 可忽略（一个枚举 + 一次 mtime 读取）
- Maintenance: 输出路径较多，注意用单一注入点避免遗漏（与 GH-38 OutputFormat 抽象方向一致，先做本 issue 不依赖它）

## 测试计划

- [ ] Unit tests: `load_internal` 四分支来源赋值；mixed 合成逻辑
- [ ] Integration tests: 五种场景（live/fresh cache/stale cache/fallback/mixed）× 三种格式
- [ ] Manual verification: 断网 + 过期缓存运行 `ccstats daily --offline --json`，确认字段与脚注

## 回滚方案

纯增量字段与脚注，revert 单个 PR 即回滚，无数据迁移。
