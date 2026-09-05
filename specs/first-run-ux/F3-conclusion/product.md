# Product Spec — F3 Human conclusion line

## Linked Issue

https://github.com/majiayu000/ccstats/issues/166

## 用户问题

默认成功输出是日期 × token 桶 × 费用的宽表。用户要带走的是：「今天大概花了多少、比平时快还是慢、这个数完不完整」。桌面设计文档已经要求 glance 结论；CLI 默认路径没有。

## 目标

在 **table** 模式的 `daily` / `today` / `weekly` / `monthly`（含 `--source all`）打印表 **之前** 输出一行结论，使用已有合计，不新算一套账。

结论必须能口头复述，并标明费用是完整、下限，还是未知。

## 非目标

- 不改变 JSON/CSV schema（机器输出不加散文行；如需字段，另开 issue）。
- 不加 LLM、不加「建议你换模型」类优化建议（那是 why/optimize，非本 PR）。
- 不在 `session` / `project` / `blocks` / `tools` / `top` / `statusline` / `quota` 上加结论行。
- 不把 `null` 费用格式化成 `$0.00`。

## Behavior Invariants

1. 仅 `OutputFormat::Table` 且命令为 Daily / Today / Weekly / Monthly。
2. `--compact` 时结论行仍在，但更短（费用 + 完整/下限/未知，可省略 7 日对比）。
3. `--no-cost` 时结论谈 token 总量，不编造美元。
4. 费用状态：
   - 所有计入成本的模型均为 recorded/live/fresh 且 coverage 完整 → 用精确金额，无「下限」字样。
   - 存在未定价或 coverage 标明 lower bound → `≥ $X` 或 “at least $X (floor)”。
   - 完全无费用（strict 全 N/A 或 show_cost false）→ 只报 token，写 `cost unknown` 或省略美元。
5. 多日 `daily`：结论用过滤窗口内合计，不假装是「今天」。
6. `today`：若能算 7 日日均（同源、本地时区、不含今天或含今天须在测试里钉死），给一句快/慢/持平；数据不足则不加对比，不得编造。
7. `--source all`：结论针对合计；不在结论里点名 29 个源。可用 `(N sources)`。
8. stderr 仍可有 pricing 警告；结论在 stdout，位于标题/表之前。
9. 语言：英文（与现有 CLI 表头一致），短句，不用营销词。

## 验收标准

- [ ] `ccstats today --source claude` 在表前有一行含 token 或 cost。
- [ ] 无法定价的 fixture 不出现 `$0.00` 充当真实花费。
- [ ] `--json` / `--csv` stdout 仍是纯结构化数据（第一字符 `{` 或 header 行）。
- [ ] `--compact` 仍有结论行。
- [ ] 现有 table 列与合计数字不变。

## 边界情况

- 空结果（有源但无记录）：不要结论装成 $0；沿用现有 no-data hint。
- 0 源走 doctor（F1）：无结论行。
- 货币转换：结论使用与表相同的 currency 格式化。

## 发布说明

Table 模式 period 报告增加一行人类可读摘要；数字与表一致。
