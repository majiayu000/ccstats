# Tech Spec

## Linked Issue

GH-46

## Product Spec

`specs/GH46/product.md`

## Codebase Context

| Area | Files | Current behavior | Why relevant |
| --- | --- | --- | --- |
| Config loading | `src/config.rs` | Parse failures warn and return default. | Primary correctness gap. |
| CLI/app startup | `src/main.rs`, `src/app.rs` | Consumes config before command execution. | Needs error propagation. |
| Docs | `README.md`, `docs/algorithm/authoritative-token-accounting.md`, `docs/ARCHITECTURE.md` | Config and env docs incomplete. | Documentation gap. |
| Tests | `tests/cli_integration.rs` | Covers env vars and some errors, not config parse failure. | Need integration coverage. |

## 设计方案

1. Change config loader return shape from unconditional `Config` to `Result<Config, ConfigError>` or equivalent, preserving default only when no config file exists.
2. Distinguish:
   - no config found -> `Ok(Config::default())`
   - config found and valid -> `Ok(parsed)`
   - config found but invalid/unreadable -> `Err`
3. Propagate the error to CLI exit path, including quiet/statusline commands.
4. Add docs:
   - config search order
   - example `config.toml`
   - supported keys
   - env overrides
5. Add tests for missing config, invalid TOML, wrong field type, and quiet/statusline behavior.

## Product-to-Test Mapping

| Product invariant | Implementation area | Verification |
| --- | --- | --- |
| P1/P2 | docs | Docs review. |
| P3 | config loader | Missing config test. |
| P4/P5 | config loader + CLI | Invalid config integration tests. |

## 数据流

Startup -> config search -> `Result<Config, ConfigError>` -> CLI either runs with config or exits with clear error. No persistent data changes.

## 备选方案

- Keep warning but make it louder: rejected because quiet/statusline and automation still get wrong output.
- Ignore only unknown keys: not in scope; current issue is invalid parse/type failure.

## 风险

- Security: no secret handling changes.
- Compatibility: invalid config now fails fast; intentional user-visible correctness fix.
- Performance: negligible.
- Maintenance: typed error improves future config behavior.

## 测试计划

- [ ] Unit tests for config search/load result states.
- [ ] CLI integration tests for invalid config and missing config.
- [ ] Docs review.
- [ ] `cargo check`
- [ ] `cargo test`

## 回滚方案

Revert config error propagation and docs/tests. Existing default fallback returns.
