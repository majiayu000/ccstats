# Product Spec

## Linked Issue

GH-40

## 用户问题

解析失败或被丢弃的记录只在非 quiet 模式写 stderr，JSON/CSV/statusline 等结构化消费者完全看不到数据是否不完整。增量加载路径还硬编码 `skipped: 0`，让 debug 和 summary metadata 误导用户。

## 目标

- 结构化输出携带本次加载的数据质量 metadata。
- 区分 malformed/parse error count 与 dedup skipped count。
- 增量路径不再硬编码 skipped 为 0；无法准确统计时必须显式建模，而不是静默假值。
- 现有表格 stderr warning 可保留，但不能作为唯一信号。

## 非目标

- 不改变 parser 对 malformed 行的容错策略。
- 不要求把每条错误明细输出到 JSON。
- 不依赖 GH38 OutputFormat 重构；若 GH38 先合并，可复用其输出注入点。

## Behavior Invariants

1. JSON 输出包含可机器读取的 parse/malformed count 与 dedup skipped count。
2. CSV 输出携带同等 metadata，形态需兼容现有列消费（注释行或明确 metadata 行）。
3. statusline JSON 变体保留数据质量字段；普通单行 statusline 不被破坏。
4. quiet 模式不再意味着结构化 metadata 丢失。
5. 增量路径只有在实际 skipped 为 0 时才报告 0；否则报告真实值或显式 unknown/unsupported 状态。

## 验收标准

- [ ] malformed jsonl fixture 在 `daily --json` 中显示 parse error count。
- [ ] dedup fixture 在结构化输出中显示 skipped count。
- [ ] 增量路径 debug/metadata 不再硬编码假 0。
- [ ] quiet/statusline JSON 路径保留 metadata。
- [ ] 现有 stderr warning 测试继续通过或按新 metadata 增补。

## 边界情况

- malformed 行很多但有效行也存在。
- 全部行 malformed。
- source 不需要 dedup。
- all-source 聚合多个源时 metadata 累加。

## 发布说明

JSON/CSV/statusline JSON 新增数据质量字段，向后兼容；可帮助管道消费者识别不完整统计。
