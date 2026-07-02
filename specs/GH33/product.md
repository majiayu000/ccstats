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
2. JSON 输出不得改变现有 root shape：当前 array-root 输出继续是 array，每个含成本的 row/object 以新增字段携带 `pricing_source`；statusline JSON 在现有 object 上新增字段。
3. CSV 输出不得插入前置注释行或改变 header 所在行；含成本的 CSV row 以追加列或既有兼容 row metadata 方式携带 `pricing_source`。
4. 任一非 `live` 的表格输出（包括 fresh `cache`）都在表尾输出一行脚注说明来源与（若为缓存）缓存年龄。
5. statusline 输出不因来源标注而破坏现有单行格式；任何 statusline 输出成本的 JSON 变体必须同时输出来源字段，即使命令同时给出 `--no-cost`。
6. 未知模型行为不变：JSON 中 NaN cost 继续序列化为 `null`，table/CSV 继续显示 `"N/A"`。
7. 离线模式 + 缓存缺失/损坏时行为按当前实现或 GH32 后续实现保持；本变更只为“已有数字可显示”的路径标注来源，不新增缓存错误语义。

## 验收标准

- [ ] 四种来源在 JSON/CSV/表格三种格式下均可区分，含 `mixed` 情形，且 JSON root/header 兼容性保持
- [ ] 现有消费者不破坏：只新增字段/脚注，不改动现有字段名与含义
- [ ] daily/monthly/session/project/blocks/top/budget/statusline 中所有含成本输出都携带来源或脚注
- [ ] quiet/statusline 路径下结构化输出仍携带来源字段
- [ ] 集成测试覆盖：live、离线新鲜缓存、离线过期缓存、fallback、mixed 五种场景

## 边界情况

- 同一次运行中部分模型 LiteLLM 命中、部分走 fallback → `mixed`
- 缓存文件存在但 mtime 异常（未来时间）→ 按 `cache_stale` 处理并显示原始 mtime
- `--no-cost` 模式 → 若该命令确实不输出成本，则不输出 `pricing_source`；若 statusline 等路径仍输出 cost，则必须同时输出 `pricing_source`

## 发布说明

JSON/CSV 新增字段，向后兼容；建议在 CHANGELOG 中说明字段语义，供 statusline/管道集成方采用。
