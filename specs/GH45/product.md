# Product Spec

## Linked Issue

GH-45

## 用户问题

新增一个数据源时，源身份要在两处平行维护：registry（含各源 `aliases()`）与 SDK 的手写 `UsageSource` 枚举 + `FromStr` 别名表。漏改 SDK 侧编译照过，但库 API 静默缺源——SDK 消费者拿不到新源且无报错。另外 tools 能力用 `source.name() != "claude"` 字符串门控而非 `Capabilities` 位，与 `has_projects`/`has_billing_blocks` 的既有能力驱动设计不一致，`load_tool_calls` 还绕过 Source trait 硬编码 `~/.claude` 路径。

## 目标

- 源名与别名单一事实来源（registry），SDK 不再维护第二份 alias 表
- 新增源若需要 SDK enum 暴露，必须有测试期一致性保证捕获漏改；不允许静默缺源
- tools 能力与其他能力同构（能力位门控）
- tool-call discovery 与 parsing 都通过 Source trait 边界，不再由 loader 硬编码 Claude schema/path

## 非目标

- 不新增源、不改各源解析行为
- SDK 现有公开函数签名不做破坏性变更（`UsageSource` 枚举是否保留见设计方案）

## Behavior Invariants

1. SDK 与 CLI 对 concrete sources 接受相同的源名与别名集合；任一 concrete source 新增后，SDK enum 未更新时一致性测试必须失败（禁止静默缺源）。
2. `UsageSource::from_str` 对 registry 中每个 concrete source 及其别名解析成功，对 pseudo-source `all` 不要求成功，对未知名返回与现在相同的错误类型。
3. `ccstats tools` 对无 tool-call 能力的源报错文案与现有一致；对 Claude 行为不变。
4. `UsageSource::from_str` 保持现有大小写/空白处理：先 trim，再通过 registry lookup 或等价逻辑解析。
5. `load_tool_calls` 尊重 GH-42 引入的路径覆盖逻辑（若已合并），且 tool-call 文件发现与解析都由 source trait 拥有，不再独立硬编码 Claude 路径或 parser。

## 验收标准

- [ ] 存在一个编译期或测试期的一致性保证：registry concrete source set == SDK 可解析 concrete source set；`all` 被明确排除
- [ ] 别名字符串在仓库中只出现一处（各源 `config.rs` 的 `aliases()`）
- [ ] `rg '"claude"' src/app.rs` 中不再有能力判断用途的匹配
- [ ] SDK 现有集成测试不改断言通过

## 边界情况

- SDK 消费者用 `UsageSource` 枚举做穷举 match → 若枚举保留且新增变体，属 minor 版本语义（Rust `#[non_exhaustive]` 决策在 tech spec 定）
- 大小写/空白输入解析行为保持现状

## 发布说明

若给 `UsageSource` 加 `#[non_exhaustive]`，对下游是一次性源级变更，需在 CHANGELOG 标注。
