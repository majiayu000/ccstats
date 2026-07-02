# Product Spec

## Linked Issue

GH-36

## 用户问题

Codex `token_count` 事件可能缺失 `total_tokens`，但仍携带递增的 `input_tokens`、`cached_input_tokens`、`output_tokens` 或 `reasoning_output_tokens`。当前重复事件判定只比较 `total_tokens`，缺失时等价为 0，导致后续真实增量被静默跳过，daily/monthly 总量偏低。

## 目标

- Codex 重复事件判定基于完整 usage 向量，而不是单一 `total_tokens`。
- 缺失 `total_tokens` 但分项 token 递增时必须计入增量。
- 真正重复的 `token_count` 事件仍被跳过，避免重放日志双计。
- 现有 `last_token_usage` 优先逻辑保持不变。

## 非目标

- 不改变跨文件重放去重机制（GH29/PR30）。
- 不改变 OpenAI output/reasoning token 口径。
- 不改变未知模型、成本计算或输出格式。

## Behavior Invariants

1. 两个连续 `token_count` 事件只有在完整累计 usage 向量等价时才可判为重复。
2. 当 `total_tokens` 缺失或为 0，但任一分项累计 token 增长时，事件必须产生非零 delta。
3. 当 `last_token_usage` 存在时，仍优先使用 `last_token_usage` 作为 delta。
4. 当完整累计 usage 向量完全相同，事件继续被跳过且不改变输出。
5. 现有跨 session/source-wide dedup 行为保持不变。

## 验收标准

- [ ] 缺失 `total_tokens`、分项递增的 Codex fixture 在 `daily --json` 中计入全部增量。
- [ ] 完整 usage 向量相同的重复事件仍不会双计。
- [ ] 现有 Codex reasoning/cache 口径测试继续通过。
- [ ] debug/skipped 行为不引入新的静默降级。

## 边界情况

- `total_tokens` 缺失但 `last_token_usage` 存在。
- `total_tokens` 为 0 但分项 token 非零。
- 分项 token 缺失但 `total_tokens` 递增。
- 所有字段缺失或为 0 的空 usage。

## 发布说明

修复 Codex 日志变体下的静默少计。受影响用户可能看到 Codex token/cost 统计上升到正确值。
