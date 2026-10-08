# MCP server (`ccstats mcp`)

`ccstats mcp` is a read-only [Model Context Protocol](https://modelcontextprotocol.io)
server on stdio. Coding agents can ask it how much of a quota window is used
before they start heavy work, and how many tokens and dollars a source has used
today, this week, or this month.

- Transport: newline-delimited JSON-RPC 2.0 on stdin/stdout. Stdout carries
  protocol messages only; diagnostics go to stderr. The server exits when stdin
  closes.
- Protocol versions: `2025-11-25` (preferred), `2025-06-18`, `2025-03-26`,
  `2024-11-05`. Methods: `initialize`, `ping`, `tools/list`, `tools/call`.
- No listening socket and no writes to provider files. Data access is the same
  as the matching CLI command: pricing refresh and Cursor API calls behave as in
  the CLI, and global flags apply (`ccstats --offline mcp` skips pricing and
  exchange-rate downloads). See [PRIVACY.md](PRIVACY.md).

## Setup

Claude Code:

```bash
claude mcp add ccstats -- ccstats mcp
```

Codex CLI:

```bash
codex mcp add ccstats -- ccstats mcp
```

or in `~/.codex/config.toml`:

```toml
[mcp_servers.ccstats]
command = "ccstats"
args = ["mcp"]
```

Add global flags before or after `mcp`, for example `ccstats mcp --offline
--timezone Asia/Shanghai`. The optional `config.toml` is read as for any other
command.

## Tools

All tools are read-only. Results are a text content block holding the JSON
payload; object payloads are also returned as `structuredContent`. Tool
failures (bad arguments, unavailable source) come back with `isError: true`.

| Tool | Arguments | Result |
|------|-----------|--------|
| `get_limits` | `source`: `all` (default) / `claude` / `codex` / `cursor` | Same JSON as `ccstats limits --json` |
| `get_usage_summary` | `source` (required, any `ccstats sources` name), `period`: `today` (default) / `week` / `month` | SDK `CostSummary` |
| `diagnose` | `window`: `5h` (default) / `today` / `7d`; optional `session` ID | Same JSON as `ccstats diagnose --json`; local evidence, not subscription billing |
| `doctor` | none | SDK source diagnostics, the `ccstats doctor --json` fields plus `source` |

`get_limits` reads `windows[]`: `provider`, `window`, `used_pct` (0-100, null
when unknown), `resets_at`, `source` (`official` or `estimated`), and `stale`.
Claude official windows appear only after `ccstats statusline` has received
Claude Code `rate_limits`; otherwise Claude has an `estimated_5h` activity
window that is not an official reset.

`get_usage_summary` periods are current periods in the configured timezone:
`week` is Monday through today and `month` is the 1st through today (SDK
`UsageRange::ThisWeek` / `ThisMonth`). Unknown cost stays `null`.

Smoke test:

```bash
printf '%s\n' \
  '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"sh","version":"0"}}}' \
  '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
  '{"jsonrpc":"2.0","id":2,"method":"tools/list"}' \
  | ccstats mcp --offline
```

## Hook recipe: warn when a window is hot

Example only; check the [Claude Code hooks reference](https://code.claude.com/docs/en/hooks)
for your version. A `SessionStart` hook's plain-text stdout is added to
Claude's context, so the agent sees the warning before the first prompt. This
script prints nothing when every official window is below the threshold.

`~/.claude/hooks/ccstats-limits.sh`:

```bash
#!/bin/sh
# Warn when any official quota window is at least $CCSTATS_WARN_PCT percent used.
threshold="${CCSTATS_WARN_PCT:-80}"
ccstats limits --json --offline 2>/dev/null | jq -r --argjson t "$threshold" '
  .windows[]
  | select(.source == "official" and .used_pct != null and .used_pct >= $t)
  | "ccstats: \(.provider) \(.window) window is \(.used_pct)% used (resets \(.resets_at // "unknown")). Avoid starting long tasks."
'
```

`~/.claude/settings.json`:

```json
{
  "hooks": {
    "SessionStart": [
      {
        "hooks": [
          { "type": "command", "command": "~/.claude/hooks/ccstats-limits.sh" }
        ]
      }
    ]
  }
}
```

The same script works as a `UserPromptSubmit` hook to re-check on every prompt.
Without `jq`, `ccstats watch --once --warn-pct 80` exits 1 when an official
window is at least 80% used.
