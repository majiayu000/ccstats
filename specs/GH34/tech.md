# Tech Spec

## Linked Issue

GH-34

## Product Spec

`specs/GH34/product.md`

## Codebase Context

| Area | Files | Current behavior | Why relevant |
| --- | --- | --- | --- |
| 去重核心 | `src/core/dedup.rs` | key = `(dedup_scope, message_id)`；`dedup_scope()` 返回 `session_key`；PR #30 引入 `SOURCE_WIDE_DEDUP_PREFIX`（message_id 带前缀时 scope=None 即全源去重） | 现成的跨文件去重机制，Claude 可复用 |
| Claude 解析 | `src/source/claude/parser.rs:122` | `session_key = path.display().to_string()` | scope 即文件路径的来源 |
| 加载聚合 | `src/source/loader.rs:195` 附近 | `load_deduped_entries_incremental` 收集全部条目后 finalize | 跨文件去重要求 finalize 在全文件集合上进行（现状已满足） |
| Codex 先例 | `src/source/codex/parser.rs`（PR #30 diff） | 重放条目使用 source-wide 前缀 | 实现模式参照 |

## 设计方案

阶段一（验证）：写一次性脚本或测试辅助，扫描真实 `~/.claude/projects/**`，统计同一 `message.id` 出现在多个文件的频次与 token 影响，结论记入 issue。

阶段二（实证成立时）：Claude 条目改用 source-wide 去重——方案 A：`claude/parser.rs` 为 message_id 加 `SOURCE_WIDE_DEDUP_PREFIX`（复用 PR #30 机制，改动最小）；方案 B：`Deduplicatable::dedup_scope` 对 Claude 返回 `None`（语义更直白，但需要区分源）。倾向方案 A，零核心改动。会话/blocks 等按 session 维度的视图仍用 `session_key` 分组展示，只有跨文件的重复 token 计数被合并。

## Product-to-Test Mapping

| Product invariant | Implementation area | Verification |
| --- | --- | --- |
| P2 跨文件只计一次 | `claude/parser.rs` + `dedup.rs` | 集成测试：两文件同 id |
| P3 skipped 口径 | `loader.rs` | 断言 skipped 计数 |
| P4 无重复不变 | 全链路 | 基准对比测试 |
| P5 性能 | `dedup.rs` HashMap 容量 | 现有大数据集测试运行时间 |

## 数据流

parser 产出 RawEntry（message_id 加前缀）→ loader 收集 → dedup finalize（全局 HashMap，跨文件同 id 合并）→ 聚合。无持久化/外部调用变化。

## 备选方案

- 全局 `(message_id)` 去重不带 request_id：实现更简单，但存在误杀理论风险——若实证未见同 id 不同请求，可接受
- 文档化现状不修：若实证显示跨文件重复不存在，即采用此项

## 风险

- Security: 无
- Compatibility: 统计口径变化（数字变小），需 CHANGELOG 说明；session 视图分组不受影响需回归确认
- Performance: 全局 HashMap 已是现有结构，内存增量为零（scope 字符串反而变短）
- Maintenance: 依赖 PR #30 前缀机制的稳定性，建议给前缀机制补 Claude 侧测试

## 测试计划

- [ ] Unit tests: dedup 对带前缀 Claude 条目的合并行为
- [ ] Integration tests: 两文件同 message id（一截断一完成）→ 计一次、保留完成条目、skipped+1
- [ ] Manual verification: 在真实日志上对比变更前后 `ccstats monthly` 差值与阶段一实证数据吻合

## 回滚方案

revert 单 PR 即恢复 per-file scope；无数据迁移。
