# Tech Spec — F4 `ccstats login cursor`

## Linked Issue

https://github.com/majiayu000/ccstats/issues/167

## Product Spec

`specs/first-run-ux/F4-cursor-login/product.md`

## Codebase Context

| Area | Files | Current behavior | Why relevant |
| --- | --- | --- | --- |
| CLI commands | `src/cli/commands.rs` | Doctor, Sources, views, Codex/Grok/Kimi subcommands | Add `Login`. |
| Cursor client | `src/source/cursor/client.rs` | Reads `CURSOR_API_KEY` / `CURSOR_SESSION_TOKEN` env only | Insert file fallback after env. |
| Cursor diagnose | `src/source/cursor/config.rs` | `has_api_credentials()` | Must include file. |
| Config paths | `src/config.rs` | config.toml search order | Credentials sit beside the winning config dir **or** XDG config `ccstats/` — pick one and test it. |
| Doctor tests | `tests/cli_doctor.rs` | Asserts no credential leakage | Extend. |

## 设计方案

1. New module `src/credentials.rs` (or `src/login.rs`):
   - `struct CursorCredentials { api_key: Option<String>, session_token: Option<String> }`
   - TOML file `credentials.toml` with section `[cursor]` keys `api_key`, `session_token` (only one should be Some).
   - Path: `dirs::config_dir()/ccstats/credentials.toml` plus `~/.config/ccstats/credentials.toml` fallback, **independent** of which `config.toml` won. Document this. Do not store secrets in the first-found config.toml (that file is user-edited and logged as "Loaded config from …").
2. `has_api_credentials()` checks env then file.
3. Client HTTP still uses the resolved secret in memory; never logs it.
4. CLI:

   ```text
   ccstats login cursor [--api-key] [--session-token] [--check] [--clear] [--no-browser]
   ```

   Nested: `Commands::Login { target: LoginTarget::Cursor, ... }` so we do not invent `ccstats cursor` 源子命令（与「不再为新源加子命令」一致）。
5. Browser: `open` crate is extra dependency — prefer `std::process::Command` with `open` / `xdg-open` / `cmd /c start` and ignore failure. Or print URL only if adding a dep is undesirable; **prefer zero new deps**.
6. `login` is metadata-only: skip pricing load like doctor/sources.

## Product-to-Test Mapping

| Invariant | Test |
| --- | --- |
| File write | temp HOME, login --session-token, file mode 0600 on unix |
| Doctor configured | after login, doctor json status configured, token not in stdout/stderr |
| Env precedence | file + env, --check says env |
| Mutual exclusion | both flags → exit 1 |
| Clear | --clear then missing |
| Non-TTY | no flags → exit 1 |

## 备选方案

- Store in config.toml: rejected because load logs path and users copy config.
- Read macOS Keychain: nice later, out of scope.
- `ccstats cursor login` subcommand: rejected to avoid a fourth source grammar.

## 风险

- Cookie in a file is still a secret. chmod 0600 + never print. Mention in PRIVACY.md one short bullet.
- Path vs CLAUDE_CONFIG_DIR confusion: credentials are ccstats's, not Cursor's app dir.

## 测试计划

- [ ] CLI login tests with temp HOME (see `tests/common/mod.rs`; add `XDG_CONFIG_HOME` / `HOME`).
- [ ] Doctor leakage test extended.
- [ ] `cargo test`

## 回滚方案

Remove Login command and file fallback; env-only Cursor remains.
