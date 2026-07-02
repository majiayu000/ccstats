# Product Spec

## Linked Issue

GH-44

## 用户问题

LiteLLM pricing 解析的模糊匹配使用双向 `contains`，短名或命名相近的模型可能匹配到无关价格。错误价格会静默进入成本输出，用户无法发现预算数字已错配。

## 目标

- 定价匹配策略可解释、可测试，避免任意双向 substring 错配。
- 保留已知需要的模型规范化/版本变体匹配能力。
- 无法可靠匹配时返回 unknown，让输出显示 `N/A` 或 fallback，而不是套错 LiteLLM 价格。

## 非目标

- 不重写 hardcoded fallback pricing。
- 不改变 LiteLLM 原始数据下载格式。
- 不把 unknown model 强行匹配到相近价格。

## Behavior Invariants

1. Exact normalized match 优先于任何模糊候选。
2. 允许的非 exact 匹配必须来自明确规则（如 vendor prefix stripping、dot/hyphen version variant），不能是任意 `contains`。
3. 当多个候选都可能匹配且无法确定唯一性时，返回 unknown/None，而不是最长字符串获胜。
4. 已有合法场景（如 `sonnet-4` 对 `claude-sonnet-4-*`）继续可解析，前提是规则明确。
5. 错配风险样本有回归测试。

## 验收标准

- [ ] `resolve_pricing_known` 不再使用无约束双向 `contains`。
- [ ] 保留 exact、normalized、known variant 的成功测试。
- [ ] 新增至少一个相似命名但不应匹配的负例测试。
- [ ] unknown 不因短 substring 返回错误价格。

## 边界情况

- 空模型名。
- 多个候选共享同一短 token。
- vendor prefix 存在/缺失。
- dot version 与 hyphen version（如 `glm-5.2`/`glm-5p2`）。

## 发布说明

定价匹配更保守；少数之前被模糊匹配的模型可能变为 `N/A`，这是避免错价的预期变化。
