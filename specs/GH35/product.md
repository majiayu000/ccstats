# Product Spec

## Linked Issue

GH-35

## 用户问题

Grok 源报出的 "input tokens" 实为上下文规模代理（`context_tokens_used + total_tokens_before_compaction`），其折算"成本"不是真实账单。`ccstats all monthly` 把它与 Claude/Codex 的真实开销加总为单一总成本，用户基于该总数做预算判断会系统性高估真实支出。

## 目标

- `--source all` 的成本总数只包含同一计费语义的数字，或明确区分两种语义
- 单独 `ccstats grok` 视图保持现有信息量（上下文规模仍有观察价值）

## 非目标

- 不改变 Grok 解析器的 token 提取逻辑（等上游稳定 usage 字段，见 GH-18）
- 不移除 Grok 源

## Behavior Invariants

1. `--source all` 的聚合总成本行不再混入 Grok 的合成成本；Grok 的估算以独立行/字段呈现并带 `estimated` 标记。
2. 单源 `ccstats grok` 输出行为不变，但成本列标注为估算（表格列头或脚注、JSON 字段 `cost_estimated: true`）。
3. Grok token 计数（非成本）在 all 聚合中的呈现方式保持不变。
4. JSON/CSV 中真实成本与估算成本可由机器区分。

## 验收标准

- [ ] `all monthly` 总成本 = Claude+Codex+Cursor 真实语义之和，Grok 估算独立可见
- [ ] 三种输出格式一致区分两种语义
- [ ] 现有 Grok 单源测试仅追加标注断言
- [ ] 预算/预测（monthly-budget）不把 Grok 估算计入真实开销

## 边界情况

- 只有 Grok 一个源有数据时 `all` 的总成本行 → 显示 0 真实成本 + 独立估算行，而非空白
- Grok 未来提供真实 usage（GH-18 落地）→ 标注机制按源能力位切换，而非硬编码 grok 名字

## 发布说明

`all` 聚合总成本口径变化（变小、更真实），CHANGELOG 明确说明 Grok 估算的呈现位置。
