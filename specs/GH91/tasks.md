# Task Plan

## Linked Issue

GH-91

## Spec Packet

- Product: `product.md`
- Tech: `tech.md`

## 实现任务

- [x] `SP91-T1` Owner: investigation. Done when: Kimi Code 本地数据格式经官方文档与本机真实数据核实（`usage.record` 字段、目录布局、session_index 映射）。 Verify: 字段/变体统计与本机 wire 日志抽样一致。
- [x] `SP91-T2` Owner: implementation. Done when: `src/source/kimi/{mod.rs,config.rs,parser.rs}` 实现 `Source` trait，覆盖字段映射、过滤、错误计数、会话/项目身份与回退。 Verify: parser 单元测试 10 项通过。
- [x] `SP91-T3` Owner: implementation. Done when: `source/mod.rs` 与 `registry.rs` 完成注册，数量/属性/能力/建议词测试同步。 Verify: registry 测试通过。
- [x] `SP91-T4` Owner: implementation. Done when: `commands.rs` 增加 `kimi` 子命令族并接入既有冲突校验，`args.rs` 文案同步。 Verify: `parse_command_kimi_sets_source_hint` 与冲突集成测试通过。
- [x] `SP91-T5` Owner: implementation. Done when: `sdk.rs` 增加 `UsageSource::Kimi` 且注册表 parity 测试通过。 Verify: sdk 测试通过。
- [x] `SP91-T6` Owner: pricing. Done when: `fallback.rs` 增加 `moonshot_pricing` 与 `kimi` 分支（kimi-k2.6 参考费率）。 Verify: `test_fallback_kimi_code_model` 与 `kimi_daily_json_prices_fallback_model` 通过。
- [x] `SP91-T7` Owner: tests. Done when: `tests/cli_kimi.rs` 覆盖 daily 默认、fallback 定价标记、--source、project、子 agent 聚合、冲突、sources 列表。 Verify: 7 项集成测试通过。
- [x] `SP91-T8` Owner: docs. Done when: README、CHANGELOG（Unreleased）、`docs/ARCHITECTURE.md`、Cargo.toml/lib.rs 描述同步 Kimi 支持。 Verify: docs diff review。

## 并行拆分

- Parser 与单测 owns `src/source/kimi/`（独立新目录）。
- Docs owns `README.md`、`CHANGELOG.md`、`docs/ARCHITECTURE.md`。
- 接线（`registry.rs`、`commands.rs`、`sdk.rs`、`fallback.rs`）存在串行依赖，不宜并行。

## 验证

- [x] `SP91-T9` Owner: verification. Done when: 确定性门禁全部通过。 Verify: `cargo fmt --check`、`cargo clippy --all-targets -- -D warnings`、`cargo test`（598 passed / 0 failed）、`python3 checks/check_workflow.py --repo .`、`python3 checks/check_workflow.py --repo . --spec-dir specs/GH91`。
- [x] `SP91-T10` Owner: verification. Done when: 本机真实数据端到端输出正确且标记 fallback 定价。 Verify: `ccstats kimi daily/project/today` 手工确认（2 天 242 轮、4 项目 6 会话）。

## Handoff Notes

- 定价说明：`kimi-code/k3` 无公开单价，fallback 取 LiteLLM `moonshot/kimi-k2.6` 官方参考值（$0.95/$4.00/$0.16 per M，cache-create $0）；若 Moonshot 公布 kimi-code 订阅模型单价，更新 `fallback.rs` 分支即可。
- wire 格式属未公开协议，当前观测仅 `usageScope == "turn"`、模型 `kimi-code/k3`；parser 对未知 scope 跳过，若官方新增 turn 级 scope 变体需放开 `TURN_SCOPE` 校验。
- 未执行 merge：等待人工 review + CI 绿灯 + 维护者明确授权（SpecRail human gates）。
