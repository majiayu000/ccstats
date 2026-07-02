# Product Spec

## Linked Issue

GH-39

## 用户问题

Claude 与 Cursor 解析器会接受负 token 值，导致 daily/monthly 总量被静默拉低，甚至出现负成本。Codex 与 Grok 已经对负值做 `.max(0)` 处理，四源行为不一致，用户无法信任跨源聚合结果。

## 目标

- 所有源都禁止负 token 进入 `RawEntry` 和聚合层。
- Claude/Cursor 与 Codex/Grok 的边界行为保持一致。
- 负值处理可测试、可审查，不依赖输出层补救。

## 非目标

- 不改变正常正数 token 的解析结果。
- 不改变未知模型或定价 fallback。
- 不新增 schema 兼容层或历史数据迁移。

## Behavior Invariants

1. 任一源解析出的 `input_tokens`、`output_tokens`、`cache_creation`、`cache_read`、`reasoning` 都不得为负。
2. Claude 记录中的负 usage 字段按与 Codex/Grok 一致的策略钳制为 0。
3. Cursor 两张表解析出的负 token 字段按同一策略钳制为 0；仅有负 token 且没有正 token 的记录不应产生有效用量。
4. 正常正值、缺失字段和现有 0 值行为保持不变。
5. all-source 聚合不得因为单源负值出现负 total/cost。

## 验收标准

- [ ] Claude parser 单测覆盖负 input/output/cache 字段。
- [ ] Cursor parser 单测覆盖 bubble 与 generation 两路负 token 字段。
- [ ] Codex/Grok 既有负值钳制测试或新增一致性断言通过。
- [ ] CLI/integration 层证明负值不会进入 JSON 输出总量。

## 边界情况

- `{input_tokens: -100, output_tokens: 50}` -> input 0、output 50。
- `{input_tokens: -100, output_tokens: -50}` -> 不产生正用量或产生全 0 后被现有空记录逻辑过滤。
- cache token 为负。
- 字段缺失与字段为负同时出现。

## 发布说明

修复异常日志中的静默少计/负成本。正常日志输出不变。
