# Tech Spec

## Linked Issue

GH-91

## Product Spec

`specs/GH91/product.md`

## Codebase Context

| Area | Files | Current behavior | Why relevant |
| --- | --- | --- | --- |
| Source 插件架构 | `src/source/mod.rs`, `src/source/registry.rs` | `Source` trait + 注册表；claude/codex/cursor/grok 四个实现 | 新数据源只需新增模块并注册，满足解耦要求 |
| 参照实现 | `src/source/grok/{config,parser}.rs` | env 覆盖（`GROK_HOME`）+ 默认目录 + 逐文件解析 + 单元测试模式 | Kimi 实现完全对称的模板 |
| 加载与聚合 | `src/source/loader.rs`, `src/core/aggregator.rs` | 按 `find_files`/`parse_file` 并行加载、日期过滤、聚合 | 新源无需改动，由 capabilities 驱动 |
| 子命令路由 | `src/cli/commands.rs`, `src/lib.rs` | `codex`/`grok` 有专属子命令并携带 source hint；冲突在 `resolve_overridden_command_source` 校验 | `kimi` 子命令按 Grok 模式接入 |
| 定价 | `src/pricing/resolver/{parse,resolve,fallback}.rs` | LiteLLM 解析 CN vendor（含 `moonshot/`、`kimi`）条目；fallback 无 kimi 分支返回 None | `kimi-code/k3` 无公开键，需要 fallback 分支 |
| SDK | `src/sdk.rs` | `UsageSource` 与注册表 parity 由测试强制 | 必须同步新增 `UsageSource::Kimi` |

## 设计方案

新增 `src/source/kimi/{mod.rs,config.rs,parser.rs}`：

- `config.rs`：`KimiSource` 实现 `Source` trait。`name = "kimi"`，`display_name = "Kimi Code"`，别名 `["km"]`。capabilities：`has_projects = true`、`has_cache_creation = true`，其余 false。
- `parser.rs`：
  - `find_kimi_files`：`$KIMI_CODE_HOME/sessions/*/*/agents/*/wire.jsonl`（env `KIMI_CODE_HOME`，默认 `~/.kimi-code`），glob 排序去重。
  - `parse_kimi_wire_file_with_debug`：逐行处理；先 `contains("usage.record")` 预过滤，再校验 `type` 字段；仅接受 `usageScope` 缺失或为 `"turn"` 的记录；`time`（epoch 毫秒）转 UTC，再经 `Timezone` 得出本地 `date_str`；token 字段缺失按 0、负数截断为 0；全零记录跳过。
  - 会话身份：wire 路径 ancestors 第 3 级为 session 目录，目录名为 `session_id`，目录路径为 `session_key`；主 agent 与子 agent 因而聚合到同一会话。
  - 项目路径：读取 ancestors 第 6 级根目录下的 `session_index.jsonl`，建 `sessionId → workDir` 映射；缺失时回退 `workDirKey`（`wd_<slug>_<12 hex>`）的 slug。
  - 条目 `cost_kind = Real`（产品记录的真实 API 用量，非上下文快照估计）。
- `registry.rs`：注册 `KimiSource`，更新数量/属性/能力/建议词测试。
- `commands.rs`：`Commands::Kimi` + `KimiCommands`（daily/weekly/monthly/today/session/project/statusline），复用现有冲突校验。
- `fallback.rs`：新增 `moonshot_pricing` 与 `kimi` 分支（input $0.95/M、output $4/M、cache-read $0.16/M、cache-create $0），取 LiteLLM 中 Moonshot 官方 `moonshot/kimi-k2.6` 条目值。
- `sdk.rs`：`UsageSource::Kimi`（VARIANTS 4→5）。
- 文档：README、CHANGELOG（Unreleased）、`docs/ARCHITECTURE.md` env 表、Cargo.toml/lib.rs/args.rs 描述串。

## Product-to-Test Mapping

| Product invariant | Implementation area | Verification |
| --- | --- | --- |
| P1 子命令与 --source/别名 | `commands.rs`, `registry.rs` | `parse_command_kimi_sets_source_hint`；`source_flag_can_select_kimi_without_subcommand`；`suggest_source("kim")` |
| P2 字段映射 | `parser.rs` | `parses_turn_usage_records` |
| P3 子 agent 同会话聚合 | `parser.rs`（ancestors 派生） | `sub_agent_wire_files_share_session_identity`；`kimi_counts_sub_agent_usage_in_same_session` |
| P4 项目路径 | `parser.rs`（session_index + slug 回退） | `falls_back_to_work_dir_key_slug_without_index`；`kimi_subcommand_supports_project_view` |
| P5 过滤规则 | `parser.rs` | `skips_lines_that_only_mention_usage_record`、`skips_non_turn_scopes_to_avoid_double_counting`、`skips_all_zero_usage_records` |
| P6 解析错误计数 | `parser.rs` | `counts_malformed_usage_record_as_error` |
| P7 负数截断 | `parser.rs` | `clamps_negative_token_counts` |
| P8 fallback 定价与标记 | `fallback.rs` | `test_fallback_kimi_code_model`；`kimi_daily_json_prices_fallback_model` |
| P9 env 覆盖 | `parser.rs` | 集成测试全部经 `KIMI_CODE_HOME` 注入 |
| P10 子命令冲突 | `lib.rs`（既有校验） | `kimi_subcommand_conflicts_with_different_source_flag` |
| P11 其他源不受影响 | 无改动 | 既有 598 项测试全绿 |

## 数据流

输入：`$KIMI_CODE_HOME/sessions/<workDirKey>/<sessionId>/agents/<agent>/wire.jsonl`（逐行 JSON）+ `$KIMI_CODE_HOME/session_index.jsonl`。
处理：`find_files` → 并行 `parse_file` → `RawEntry` → 日期过滤 → 聚合（daily/session/project/…）→ 定价（LiteLLM → fallback）→ 输出（table/json/csv/statusline）。
输出：现有输出层零改动。持久化：无。外部调用：无新增（沿用定价缓存机制）。

## 备选方案

- 仅 `--source kimi`、不加专属子命令：改动更小，但与 codex/grok 的既有 UX 模式不一致，放弃。
- 会话目录按文件 mtime 聚合成单条摘要（Grok 模式）：丢失逐轮粒度与 cache 字段，放弃。
- 为 `kimi-code/*` 增加 resolver 前缀归一化（strip `kimi-code/`）：归一化后仍无 LiteLLM 键可命中，属投机泛化，放弃。

## 风险

- Security: 仅读取本地 wire 日志，不引入网络调用或凭证访问；索引/日志解析均只取所需字段。
- Compatibility: `KIMI_CODE_HOME` 为官方公开环境变量；wire 格式属未公开协议，若 Kimi Code 变更字段名将计入解析错误并提示，不影响其他源。
- Performance: 行级 `contains` 预过滤 + rayon 并行；本机 26 万行 wire 日志全量解析 <10ms。
- Maintenance: 格式假设集中于 `parser.rs` 顶部注释与常量；`usageScope != "turn"` 防御性跳过，避免未来累积型记录造成重复计费。

## 测试计划

- [x] Unit tests: parser 10 项（映射/过滤/错误/回退/截断/子 agent/slug 校验）、registry 4 项、commands 1 项、fallback 1 项、sdk parity 既有强制测试
- [x] Integration tests: `tests/cli_kimi.rs` 7 项（daily 默认、fallback 定价、--source、project、子 agent 聚合、冲突、sources 列表）
- [x] Manual verification: 本机真实数据 `ccstats kimi daily/project/today` 输出正确（2 天 242 轮、4 项目 6 会话）

## 回滚方案

纯新增代码，无既有行为改动：revert 本 PR 即可完整移除；`--source kimi`/`ccstats kimi` 随之消失，其余数据源不受影响。
