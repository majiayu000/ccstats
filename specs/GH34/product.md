# Product Spec

## Linked Issue

GH-34

## 用户问题

Claude 流式响应会为同一 `message.id` 写多条记录，ccstats 通过去重保留完成条目。但去重范围是单个 jsonl 文件：同一 `message.id` 出现在两个文件时（resume 会话、项目目录迁移、主会话与 subagent 各自落盘）token 与成本被计两次，daily/monthly 总量与预算预测虚高，且没有任何 skipped 计数提示。同类工具（ccusage）按全局 messageId 去重，用户跨工具对账会发现 ccstats 偏高。

## 目标

- 同一 Claude `message.id` 在一次统计中最多计入一次
- 与 ccusage 等工具的统计口径可对账
- 去重决策可观测（skipped 计数如实反映跨文件去重）

## 非目标

- 不改变 Codex/Cursor/Grok 的去重语义（Codex 已有 source-wide 机制，行为不变）
- 不改变"保留 stop_reason 完成条目"的候选选择逻辑

## Behavior Invariants

1. 前置验证：先用真实 `~/.claude/projects/**` 日志实证同一 `message.id` 跨文件重复确实发生；若实证不成立，本 spec 降级为文档化 per-file scope 的设计决定并关闭 issue。
2. （实证成立时）同一 `(message_id, request_id?)` 的条目无论分布在多少个文件中，聚合结果只计一次，保留规则仍为"优先 stop_reason 完成条目，否则最新时间戳"。
3. 跨文件被去掉的条目计入 skipped 计数，与文件内去重的计数口径一致。
4. 无重复场景下的统计结果与变更前完全一致。
5. 内存与运行时间在大历史日志（数百 MB）下不出现数量级退化。

## 验收标准

- [ ] 附带实证记录（真实日志中跨文件重复的样本或"未发现"结论）
- [ ] 集成测试：同一 message id 分布于两个文件 → 只计一次且 skipped +1
- [ ] 现有单文件去重测试不改断言全过
- [ ] 与变更前对比基准：无重复数据集输出逐字节一致

## 边界情况

- 同 message id、两文件条目 token 数不同（一条截断一条完成）→ 按候选规则保留完成条目
- message id 缺失的条目 → 维持现状（不去重）
- 相同 message id 但确属不同请求（若 request_id 可区分）→ 以 `message_id + request_id` 为键避免误杀

## 发布说明

统计口径变化：有跨文件重复历史的用户会看到 daily/monthly 总量下降（更准确）。CHANGELOG 需明确说明这是修正而非丢数据。
