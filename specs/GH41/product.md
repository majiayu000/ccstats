# Product Spec

## Linked Issue

GH-41

## 用户问题

Cursor 源当前同时读取 `cursorDiskKV` 与 `ItemTable`，但没有证明两表记录不会描述同一轮对话；若重叠会双计。只读 SQLite 连接也没有 `busy_timeout`，Cursor 正在写入时可能让整库读取失败。解析器还提取 `project_path`，但能力位声明 `has_projects: false`，用户永远无法通过 project 命令使用该信息。

## 目标

- Cursor 双表读取的去重/不重叠语义有证据或代码约束。
- SQLite 忙碌时给只读解析一个合理等待窗口，减少整库丢失。
- `project_path` 与 source capabilities 一致：要么可用，要么不再提取/承诺。

## 非目标

- 不把 Cursor 源从 experimental 改为稳定。
- 不引入写数据库或迁移 Cursor 数据。
- 不改变 Claude/Codex/Grok 源行为。

## Behavior Invariants

1. 若实证确认两表可能重叠，同一 Cursor 交互不得在一次统计中计两次。
2. 若实证确认两表不重叠，代码注释/测试必须记录不变量，避免未来误改。
3. 只读打开 Cursor DB 后应设置 `busy_timeout`，短暂写锁不应立即导致整库记录丢失。
4. Cursor `project_path` 的解析结果不得处于不可达状态。
5. Cursor 解析错误仍计入 errors，不被静默吞掉。

## 验收标准

- [ ] 有双表重叠调查记录或可复现 fixture。
- [ ] `busy_timeout` 行为有测试或最小可验证实现。
- [ ] `has_projects` 与 `project_path` 输出/解析策略一致。
- [ ] Cursor 现有测试继续通过。

## 边界情况

- 只有 `cursorDiskKV` 表存在。
- 只有 `ItemTable` 表存在。
- 两表都有相同或近似 token/timestamp 记录。
- DB 被短暂锁住。
- workspace/project path 缺失。

## 发布说明

Cursor 源仍为 experimental；本变更提升统计完整性与可解释性。
