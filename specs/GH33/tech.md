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
| 成本序列化 | `src/output/format.rs:109-143` | `cost_json_value` maps NaN to JSON `null`; table/CSV uses `"N/A"`. | Must preserve unknown-cost shape while adding source fields. |
| 各输出路径 | `src/output/{json,csv,table,statusline,budget,top,session,project,blocks}.rs` | 各自组装输出 | 需要统一携带 `pricing_source`，不能只覆盖 period/top。 |
| 缓存 | `src/pricing/cache.rs` | 无年龄查询接口 | 需要暴露缓存 mtime 以计算年龄 |

## 设计方案

1. `PricingDb` 增加字段 `source: PricingSource`（枚举 `Live | Cache | CacheStale | Fallback`）和可选 `cache_age`/`cache_mtime`，在 `load_internal` 各分支赋值；`cache.rs` 增加返回缓存 mtime/age 的辅助函数，避免 renderer 之后重新读取可变 cache state。
2. 逐模型解析时记录 price source counts，而不是单个 `fallback_hits: bool`：至少区分 known-hit count、fallback-hit count、unknown count。会话级来源规则：只有 fallback 命中且无 known hit => `fallback`；known + fallback => `mixed`；known-only => DB source。
3. 输出层：`OutputContext`（或等价的现有参数结构）增加 `pricing_source` 与 cache age。JSON array-root 输出保持 array，在每个含成本 row/object 追加 `pricing_source`；object-root statusline 在现有 object 追加字段。CSV 不插入前置 comment，不移动 header，按追加列/兼容 metadata row 方案保留 line 0 header。
4. 表格：所有非 `live` 来源都追加脚注，包括 fresh `cache`、`cache_stale`、`fallback`、`mixed`。
5. statusline：单行格式不变；其 JSON 分支只要输出 `cost` 就必须输出 `pricing_source`。

## Product-to-Test Mapping

| Product invariant | Implementation area | Verification |
| --- | --- | --- |
| P1 来源枚举 | `pricing/db.rs` | 单测：四分支 + mixed 合成 |
| P2 JSON/CSV 一致 | `output/{json,csv,session,project,blocks,top,statusline}.rs` | 集成测试断言 root/header 兼容且来源值同义 |
| P3 表格脚注 | `output/table.rs` and table renderers | 集成测试匹配 live/cache/cache_stale/fallback/mixed 文案 |
| P4 statusline | `output/statusline.rs` | 现有格式断言不变 + JSON 字段新增断言 |
| P5/P6 现有行为 | 全链路 | 现有测试不改断言全过 |

## 数据流

`load_internal` 分支 → `PricingDb.source` + cache age + per-model source counts → 汇总为会话/row `pricing_source` → 传入输出层 → JSON row/object field / CSV appended field / 表格脚注。无新增持久化与外部调用。

## 备选方案

- 顶层 JSON envelope 或 CSV 前置 comment：拒绝，因为现有 JSON array root 与 CSV header line 0 是兼容性契约。
- 只加 stderr 警告不改结构化输出：不解决管道消费者的问题——拒绝

## 风险

- Security: 无
- Compatibility: JSON/CSV 只追加 row/object 字段或列，不改变 JSON root shape 或 CSV header position；在 CHANGELOG 声明新增字段。
- Performance: 可忽略（一个枚举 + 一次 mtime 读取）
- Maintenance: 输出路径较多，注意用单一注入点避免遗漏（与 GH-38 OutputFormat 抽象方向一致，先做本 issue 不依赖它）

## 测试计划

- [ ] Unit tests: `load_internal` 四分支来源赋值；mixed 合成逻辑
- [ ] Integration tests: 五种场景（live/fresh cache/stale cache/fallback/mixed）× JSON/CSV/table，覆盖 period/session/project/blocks/top/statusline 中含成本路径
- [ ] Manual verification: 断网 + 过期缓存运行 `ccstats daily --offline --json`，确认字段与脚注

## 回滚方案

纯增量字段与脚注，revert 单个 PR 即回滚，无数据迁移。
