# Product Spec — F4 `ccstats login cursor`

## Linked Issue

https://github.com/majiayu000/ccstats/issues/167

## 用户问题

Cursor 用量来自官方 API，必须提供 `CURSOR_API_KEY` 或 `CURSOR_SESSION_TOKEN`。今天唯一路径是读 README、打开 dashboard、从 cookie 抠 `WorkosCursorSessionToken`、export 环境变量。cookie 过期后失败信息不够行动化。产品选择不偷读本地 SQLite，这是对的，但缺少引导式授权。

## 目标

增加 `ccstats login cursor`：

- 说明两种凭证（企业 API key vs 个人 session cookie）的区别。
- 打开（或打印）已文档化的 dashboard / API 页面 URL。
- 接受粘贴的 token（交互 stdin 或 flag），写入 **本地配置文件**，权限仅当前用户可读。
- 之后 `doctor` 将 Cursor 标为 Configured，无需再 export（环境变量仍优先，便于 CI）。
- 明确：ccstats 不会代用户登录 Cursor，也不会把 cookie 传到第三方。

## 非目标

- 不实现 OAuth 服务器、不拦截浏览器 cookie、不读取 `state.vscdb`。
- 不做 `login claude` / `login codex`（那些是本地文件源）。
- 不在本 PR 改默认源检测（F1）或结论行（F3）。
- 不把 token 写入 README 示例或 debug stdout。

## Behavior Invariants

1. 命令：`ccstats login cursor`。
   - `--api-key <value>` 非交互写入 API key。
   - `--session-token <value>` 非交互写入 session token。
   - 两者同时提供：报错退出 1（一次只存一种）。
   - 皆无且 stdin 是 TTY：打印说明 + URL，提示选择 1/2，隐藏回显读入一行。
   - 皆无且 stdin 非 TTY：报错，要求 flag（测试走这条）。
2. 存储路径：与 config 搜索同根的 credentials 文件，例如 `~/.config/ccstats/credentials.toml`（若本次运行已有权威 config 目录，用同一目录）。不把 secret 写进 `config.toml`。
3. 文件权限：Unix 0600；写时先写 temp 再 rename。
4. 读取顺序：环境变量 `CURSOR_API_KEY` / `CURSOR_SESSION_TOKEN` > credentials 文件 > 无。`CURSOR_USAGE_FILE` 仍独立。
5. `ccstats login cursor --check`：不打印 secret，只打印是否已配置以及来源（env vs file）。
6. `ccstats login cursor --clear`：删除文件中的 Cursor 字段或整个 credentials 文件（若已空）。
7. doctor / diagnose：文件里有非空凭证时与 env 一样视为 Configured；detail 不得包含 token 内容或文件绝对路径中的 token。
8. `--debug`、错误信息、JSON doctor 不得出现 token 值。
9. 打开浏览器：能开则开文档中的 URL（API key → cursor.com/dashboard/api，session → cursor.com/dashboard/usage）；失败则打印 URL，不视为命令失败。

## 验收标准

- [ ] 非交互 `--session-token` 写入后，去掉 env，`doctor --json` 中 cursor status 为 `configured`。
- [ ] 同一 doctor JSON 字符串不包含该 token。
- [ ] env 已设置时覆盖文件，`--check` 报告 `env`。
- [ ] `--api-key` 与 `--session-token` 同时给出 → exit 1。
- [ ] `--clear` 后 doctor 回到 missing（无 env 时）。
- [ ] 现有 `CURSOR_USAGE_FILE` 测试不受影响。

## 边界情况

- Windows：尽力设置 ACL；若做不到，仍写入并在 stderr 警告权限。
- 空 token / 只有空白：拒绝保存。
- 只读 HOME：明确 IO 错误，exit 1。

## 发布说明

新增 Cursor 登录向导；凭证只留在本机 ccstats credentials 文件。仍不读取 Cursor SQLite。
