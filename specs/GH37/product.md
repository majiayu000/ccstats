# Product Spec

## Linked Issue

GH-37

## 用户问题

`merge_day_stats` 在 loader 与 app 两处一字不差重复。未来 `DayStats` 或 `Stats` 新增字段时，只改其中一处会导致某些命令静默漏合并，用户看到不同入口的 daily/all 统计不一致。

## 目标

- `merge_day_stats` 只有一个生产实现。
- 现有 loader 与 all-source 聚合路径共用同一个 helper。
- 现有合并语义与测试结果保持不变。

## 非目标

- 不重构 `LoadResult`、`DayStats` 或 `Stats` 数据结构。
- 不改变 all-source 聚合输出。
- 不拆分 loader/table/csv 大文件（GH47 单独处理）。

## Behavior Invariants

1. 相同日期的 `DayStats` 合并时，token、成本、计数和模型 map 按现有语义累加。
2. 不同日期的记录都保留。
3. 空 source 合并为 no-op。
4. loader 与 app all-source 路径调用同一实现。
5. 移动 helper 后现有公开 API 不新增兼容 shim。

## 验收标准

- [ ] `src/source/loader.rs` 和 `src/app.rs` 不再各自定义重复的 `merge_day_stats`。
- [ ] 原 loader 合并测试迁移到单一 helper 所在模块或继续覆盖共享 helper。
- [ ] all-source 聚合行为有至少一个回归测试或现有测试证明不变。
- [ ] `cargo test` 全部通过。

## 边界情况

- source map 为空。
- target 中不存在 source 日期。
- target/source 有相同日期但模型集合不重叠。
- target/source 有相同日期且模型名相同。

## 发布说明

内部维护性修复，无用户可见行为变化。
