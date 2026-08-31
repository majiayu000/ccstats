# Privacy and data access

ccstats is local-first and does not require an account. It extracts token,
model, timestamp, project/session identity, tool-call, and cost metadata needed
for reports. It does not persist or upload prompt text, model responses, or
source-code content.

## Local data read

The 29 registered sources are Claude Code, OpenAI Codex, Cursor, Grok, Kimi
Code, Gemini CLI, Amp, Qwen Code, Cline, Roo Code, Kilo Code, OpenCode, MiMo
Code, Kilo CLI, Pi, Senpi, Kimchi, Gajae Code, Prime Agent, Oh My Pi, GitHub
Copilot CLI, Goose, OpenClaw, Xum, Hermes Agent, Reasonix, Vercel Fx, Unsloth
Studio, and DeepSeek Harness. Their default locations and overrides are listed
in the README's complete supported-source table.

`ccstats doctor` checks the registered sources' known locations and relevant
environment-variable presence. It does not parse session contents or contact
remote services.

## Network access

| Feature | Endpoint | Data sent |
|---------|----------|-----------|
| Pricing refresh | `raw.githubusercontent.com/BerriAI/litellm` | Standard HTTPS request; no session data |
| Currency conversion | `open.er-api.com` | Requested USD rate catalog; no session data |
| Cursor Admin usage | `api.cursor.com` | User-supplied API key and requested date range |
| Cursor dashboard usage | `cursor.com` | User-supplied session token and requested date range |

`--offline` disables pricing and exchange-rate downloads and uses cached data.
It does not make Cursor local because Cursor is an API-backed source; use
`CURSOR_USAGE_FILE` for an explicit offline replay.

## Local data written

ccstats writes only operational data needed to make repeated reports reliable:

- pricing cache under the platform cache directory and exchange-rate cache
  under `~/.cache/ccstats/`;
- a Grok inference ledger under the ccstats cache directory because Grok may
  trim its live log in place;
- no prompt, response, or source-code archive.

The optional TOML config is user-created. ccstats reads the first configured
path documented in the README and fails clearly if that file is malformed.

## Credentials

Cursor credentials are read from `CURSOR_API_KEY` or `CURSOR_SESSION_TOKEN` and
sent only to the corresponding Cursor endpoint. ccstats does not print them,
write them to its cache, or include them in doctor JSON/CSV output.

When reporting a bug, include `ccstats doctor --json` and the command's stderr.
Do not attach session logs, environment dumps, config files containing secrets,
or Cursor credentials.
