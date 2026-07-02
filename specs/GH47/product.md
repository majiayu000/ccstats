# Product Spec

## Linked Issue

GH-47

## 用户问题

代码库出现多处维护性风险：若干文件超过或接近 AGENTS 的行数上限，公开 SDK `SummaryOptions` 与内部 output `SummaryOptions` 同名，集成测试文件过大，关键函数用 `#[allow(clippy::too_many_lines)]` 压住复杂度。这些不会立即改变用户输出，但会降低后续修复速度并增加误改风险。

## 目标

- 将 #47 列出的技术债拆成可独立 review 的实现 slice。
- 超过 800 行的文件降到上限以下，接近上限的文件有明确拆分计划。
- 消除公开/内部 `SummaryOptions` 命名冲突。
- 拆分超大集成测试文件，保持测试语义不变。
- 移除 issue 点名的 `too_many_lines` allow，或用可测试子函数替代。

## 非目标

- 不改变 CLI 输出、SDK API 行为或统计语义。
- 不顺手处理 issue 未点名的其他 clippy allow。
- 不做大规模架构重写。

## Behavior Invariants

1. 文件拆分后，所有公开命令输出逐字节保持不变。
2. SDK `SummaryOptions` 的公开名称和行为保持不变；内部 output 类型改名不得泄漏到 SDK。
3. 测试拆分后，原测试覆盖不减少，fixture/helper 不重复发散。
4. `parse_codex_file_with_debug` 与 `run_cli` 的行为保持不变，但复杂步骤拆成命名 helper 并可单测。
5. 每个实现 PR 都有明确文件所有权和回归验证。

## 验收标准

- [ ] `src/source/loader.rs` 与 `src/output/table.rs` 降到 800 行以下。
- [ ] `src/output/csv.rs` 有拆分或保持低于上限且有后续计划。
- [ ] 内部 output `SummaryOptions` 改名，SDK `SummaryOptions` 不变。
- [ ] `tests/cli_integration.rs` 拆为 source/feature 维度文件。
- [ ] issue 点名的 `parse_codex_file_with_debug` 与 `run_cli` 不再需要 `#[allow(clippy::too_many_lines)]`。
- [ ] `cargo test` 全部通过。

## 边界情况

- 测试 helper 共享导致模块循环。
- 重命名内部 type 影响 `pub(crate)` re-export。
- 文件拆分造成 snapshot/order 输出变化。
- 多个 refactor PR 并行修改同一大文件。

## 发布说明

内部维护性改进，无用户可见行为变化。
