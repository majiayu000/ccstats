# Tech Spec

## Linked Issue

GH-45

## Product Spec

`specs/GH45/product.md`

## Codebase Context

| Area | Files | Current behavior | Why relevant |
| --- | --- | --- | --- |
| registry | `src/source/registry.rs` | 注册四源；`get_source` 按名/别名解析 | 目标单一事实来源 |
| 各源别名 | `src/source/*/config.rs` `aliases()` | "cc"/"cx"/"cur"/"gx" | 与 SDK 重复的一份 |
| SDK 枚举 | `src/sdk.rs:27-55` | 手写 `UsageSource` + `as_str` + `FromStr`（重复别名） | 平行维护点 |
| tools 门控 | `src/app.rs:528` | `source.name() != "claude"` | 字符串门控 |
| tools 加载 | `src/source/loader.rs:505-520` | 绕过 trait 硬编码 `~/.claude/projects` | trait 旁路 |
| 能力位 | `src/source/mod.rs:28`（`Capabilities`） | has_projects/has_billing_blocks/... | 扩展点 |

## 设计方案

1. **别名去重**：`UsageSource::from_str` 先 trim 输入，再委托 registry lookup（或 registry 的 concrete-source lookup）解析源名/别名；成功后按 concrete `name()` 映射到枚举变体。删除 SDK 内的别名字面量。映射用一个双向辅助（`UsageSource::from_name(&str) -> Option<Self>`），并加一致性测试：遍历 registry concrete sources（明确跳过 pseudo-source `all`），断言每个名字与别名都能 `from_str`，且每个 `UsageSource` 变体的 `as_str` 能 `get_source`。枚举保留（SDK 类型安全有价值），是否 `#[non_exhaustive]` ：加上，为未来新源留出非破坏空间；新增 SDK-exposed source 仍需要新增 enum variant，但漏改必须被一致性测试捕获。
2. **能力位**：`Capabilities` 增加 `has_tool_calls: bool`（仅 claude=true）；`handle_tools` 改为能力位门控，报错文案不变。
3. **trait 化 tools 加载**：`Source` trait 增加 tool-call loading capability（命名由实现决定，如 `load_tool_calls(...)` 或 `tool_call_entries(...)`），默认返回 unsupported/empty，Claude 覆写文件发现与 parsing。`load_tool_calls` 不再硬编码 Claude parser 或 `~/.claude/projects`，路径逻辑回到 Claude 模块，与 GH-42（CLAUDE_CONFIG_DIR）天然汇合。

## Product-to-Test Mapping

| Product invariant | Implementation area | Verification |
| --- | --- | --- |
| P1 集合一致 | `sdk.rs` + `registry.rs` | concrete source 一致性遍历测试，跳过 `all` |
| P2 from_str | `sdk.rs` | 现有 + 新增单测，覆盖 trim 后 registry lookup |
| P3 tools 门控 | `app.rs` | 集成测试文案断言 |
| P4/P5 路径与 parser 统一 | `loader.rs` + `claude/` + Source trait | tools 集成测试 |

## 数据流

身份解析路径从"两份 alias 表"变为"registry alias 表 + SDK enum 映射"。Tool-call 数据流从 loader hardcoded Claude path/parser 变为 source trait dispatch。

## 备选方案

- 删除 `UsageSource` 枚举、SDK 全用 `&str`：消除映射但丢类型安全，且是破坏性 API 变更——拒绝
- build script 从 registry 生成枚举：编译期强一致但引入 build 复杂度，四个源规模不值——拒绝，测试期一致性足够
- 删除 `UsageSource` 枚举改成 registry-backed string type：可做到新增源无需 SDK variant，但破坏现有 SDK 类型安全——拒绝，本 issue 改为测试捕获漏改。

## 风险

- Security: 无
- Compatibility: `#[non_exhaustive]` 对做穷举 match 的下游是一次性破坏（当前 SDK 公开不久、消费者少，代价最低时机）；其余无
- Performance: 无
- Maintenance: alias 接入点减少；新增 SDK-exposed source 仍需 enum variant，但漏改必被测试抓住

## 测试计划

- [ ] Unit tests: from_str 委托、一致性遍历、能力位
- [ ] Integration tests: tools 命令各源行为；SDK 现有套件
- [ ] Manual verification: `ccstats --source cx daily` 与 SDK 同别名调用结果一致

## 回滚方案

revert 单 PR；无迁移。`#[non_exhaustive]` 回滚同样是源级变更，放最后一个 commit 便于单独取舍。
