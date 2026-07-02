# Product Spec

## Linked Issue

GH-43

## 用户问题

pricing cache 路径硬编码为 `~/.cache/ccstats/pricing.json`，在 macOS/Windows 上不符合平台缓存目录惯例。用户清理系统缓存或排查文件位置时会困惑，代码也与 `config.rs` 的平台目录策略不一致。

## 目标

- 新的 pricing cache 默认写入 `dirs::cache_dir()/ccstats/pricing.json`。
- 平台 cache dir 不可用时有明确 fallback。
- 旧硬编码路径已有缓存不会造成离线模式突然全部失效。

## 非目标

- 不改变 pricing JSON 内容格式。
- 不改变 exchange rate cache（除非作为单独后续 issue）。
- 不改变 online fetch/fallback 定价语义。

## Behavior Invariants

1. 在支持平台 cache dir 的系统上，新写入使用 `dirs::cache_dir()/ccstats/pricing.json`。
2. `dirs::cache_dir()` 不可用时，fallback 行为明确且可测试。
3. 如果新路径没有缓存但旧 `~/.cache/ccstats/pricing.json` 存在，读取路径应有兼容策略或明确迁移提示，避免离线用户静默失效。
4. 成功在线拉取后的保存目标是新平台路径。
5. cache path 逻辑集中在单一 helper，避免读写路径分叉。

## 验收标准

- [ ] cache path helper 使用 `dirs::cache_dir()`。
- [ ] 读写路径共享同一目标选择逻辑。
- [ ] 旧路径兼容/迁移行为有测试。
- [ ] macOS/Linux/Windows 路径决策有单元测试或可注入 helper 测试。

## 边界情况

- 平台 cache dir 缺失。
- 新旧 cache 同时存在。
- 只有旧 cache 存在且 `--offline`。
- cache 目录创建失败。

## 发布说明

pricing cache 位置改为平台标准缓存目录；如需清理旧缓存，可手动删除旧 `~/.cache/ccstats/pricing.json`。
