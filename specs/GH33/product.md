# Product Spec

## Linked Issue

GH-33

## 用户问题

ccstats 的成本数字有四种可能的定价来源：实时拉取的 LiteLLM 价格、新鲜本地缓存、过期本地缓存（离线模式下无年龄上限）、硬编码 fallback 价格表。用户（尤其是消费 `--json`/`--csv` 输出的下游程序和 statusline 集成）无法区分权威数字与降级估算——唯一信号是 stderr 的提示，且 quiet 路径下完全静默。基于降级估算做预算决策会失真。

## 目标

- 每一个输出的成本数字都能追溯其定价来源
- 结构化输出（JSON/CSV）以机器可读字段携带来源
- 表格输出在非 live 定价时给出人类可读提示
- 离线使用过期缓存时可见缓存年龄

## 非目标

- 不改变定价解析、匹配、fallback 的取值逻辑
- 不把 `--strict-pricing` 变为默认行为
- 不新增网络请求或缓存刷新策略

## Behavior Invariants

1. 任一运行会话确定唯一的 `pricing_source`，取值为 `live` | `cache` | `cache_stale` | `fallback` | `mixed`（当部分模型走 fallback 时为 `mixed`）。
2. JSON 输出在顶层 meta 携带 `pricing_source`；CSV 输出以注释行或独立列携带同一信息；两种格式的取值一致。
3. `pricing_source != live && != cache` 时，表格输出在表尾输出一行脚注说明来源与（若为缓存）缓存年龄。
4. statusline 输出不因来源标注而破坏现有单行格式；降级信息至少保留在其 JSON 变体中。
5. 未知模型行为不变：NaN → "N/A"，不受本变更影响。
6. 离线模式 + 缓存缺失/损坏时行为不变（报错），本变更只覆盖"有数字可显示"的路径。

## 验收标准

- [ ] 四种来源在 JSON/CSV/表格三种格式下均可区分，含 `mixed` 情形
- [ ] 现有消费者不破坏：只新增字段/脚注，不改动现有字段名与含义
- [ ] quiet/statusline 路径下结构化输出仍携带来源字段
- [ ] 集成测试覆盖：live、离线新鲜缓存、离线过期缓存、fallback、mixed 五种场景

## 边界情况

- 同一次运行中部分模型 LiteLLM 命中、部分走 fallback → `mixed`
- 缓存文件存在但 mtime 异常（未来时间）→ 按 `cache_stale` 处理并显示原始 mtime
- `--no-cost` 模式 → 不输出 pricing_source（无成本即无来源）

## 发布说明

JSON/CSV 新增字段，向后兼容；建议在 CHANGELOG 中说明字段语义，供 statusline/管道集成方采用。
