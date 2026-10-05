# Privacy and data access

ccstats is local-first and does not require an account. It extracts token,
model, timestamp, project/session identity, tool-call, and cost metadata needed
for reports. Ordinary reports and caches do not persist or upload prompt text, model responses,
or source-code content. The opt-in `session --json --details` export includes the
first user prompt on stdout; see its [schema and scoping flags](architecture/session-details-json.md)
before sharing that output.

## Local data read

The 29 registered sources are Claude Code, OpenAI Codex, Cursor, Grok, Kimi
Code, Gemini CLI, Amp, Qwen Code, Cline, Roo Code, Kilo Code, OpenCode, MiMo
Code, Kilo CLI, Pi, Senpi, Kimchi, Gajae Code, Prime Agent, Oh My Pi, GitHub
Copilot CLI, Goose, OpenClaw, Xum, Hermes Agent, Reasonix, Vercel Fx, Unsloth
Studio, and DeepSeek Harness. Their default locations and overrides are listed
in [docs/sources.md](sources.md).

`ccstats doctor` checks the registered sources' known locations and relevant
environment-variable presence. It does not parse session contents or contact
remote services.

The desktop session list also reads existing Codex thread names from
`CODEX_HOME/session_index.jsonl` (default `~/.codex`) and existing Claude
summaries from `CLAUDE_CONFIG_DIR/projects/*/sessions-index.json` (default
`~/.claude`). Titles may contain sensitive task descriptions. ccstats does not
generate them, use the `firstPrompt` field as a title, or send them to a model.
Unreadable/malformed title indices produce a visible error independently of
usage totals; absent indices simply have no source title.

## Network access

| Feature | Endpoint | Data sent |
|---------|----------|-----------|
| Pricing refresh | `raw.githubusercontent.com/BerriAI/litellm` | Standard HTTPS request; no session data |
| Currency conversion | `open.er-api.com` | Requested USD rate catalog; no session data |
| Cursor Admin usage | `api.cursor.com` | User-supplied API key and requested date range |
| Cursor dashboard usage | `cursor.com` | User-supplied session token and requested date range |

`ccstats mcp` talks to its MCP client over stdin/stdout only; it opens no
listening socket. Its tools make the same requests as the matching CLI
commands (`limits`, SDK summaries, `doctor`) and nothing else.

`--offline` disables pricing and exchange-rate downloads and uses cached data.
It does not make Cursor local because Cursor is an API-backed source; use
`CURSOR_USAGE_FILE` for an explicit offline replay.

## Local data written

ccstats writes only operational data needed to make repeated reports reliable:

- pricing cache under the platform cache directory and exchange-rate cache
  under `~/.cache/ccstats/`;
- a usage-facts cache at `<platform cache>/ccstats/usage-facts-v1.sqlite3`
  (with SQLite WAL/SHM sidecars). It stores file identities, timestamps, model
  names, session IDs, working directories, and token/cost facts. It does not
  store prompt text, completions, or source code. Unchanged files reuse these
  facts. `--no-cache` reparses. Deleting the file while ccstats is stopped
  forces a rebuild. Parser semantic changes bump the `v1` version in the
  filename;
- Claude quota snapshots at `<platform data>/ccstats/quota/claude.jsonl`
  (opt-in: written when `statusline` receives Claude Code hook JSON with
  `rate_limits`). Percentages and reset times only;
- a Grok inference ledger under the ccstats cache directory because Grok may
  trim its live log in place;
- desktop machine snapshots under the app data directory. These snapshots
  contain only aggregate source/model token and cost summaries. The optional
  JSON export uses the same aggregate-only payload so users can move it between
  their own devices;
- no prompt, response, or source-code archive.

Manual session names are stored separately in the desktop WebView's local
storage, keyed by source and session ID. They survive app restarts on that
device, are removed with **Use source title**, and are not included in machine
snapshot exports. Clearing the app's website data removes these manual names.
The source's indices and original transcripts are never modified.

The desktop app does not send machine snapshots itself. Cross-device rollups
require an explicit JSON export on one device and an explicit import on another.

## Device sync

`ccstats sync push` writes one file per device to
`<sync_dir>/ccstats/devices/<device_id>.json`. `sync_dir` comes from
`--sync-dir`, `CCSTATS_SYNC_DIR`, or config `sync_dir`. ccstats has no sync
service and makes no network request for sync; whatever already syncs that
folder (iCloud Drive, Dropbox, Syncthing, a NAS share) decides where the file
travels, so choose a folder you trust with usage metadata. `push` loads the same
sources as `--source all`, so Cursor's API is still contacted when Cursor
credentials are configured (see Network access).

The file contains, and only contains:

- `schema_version`, `ccstats_version`, `generated_at`, and the exporting
  device's UTC offset;
- `device_id`, a random 128-bit id stored at
  `<platform data>/ccstats/device-id` (not derived from hardware, user, or
  hostname), and `device_label`, from config `device_label` or the first eight
  characters of the id;
- rows of `date × source × model` with the same token buckets, record counts,
  source-recorded USD costs, and estimated-proxy subtotals that local daily
  reports aggregate.

It does not contain prompts, responses, tool calls, source code, session or
message ids, project names or paths, working directories, file paths,
hostnames, or credentials. Model names that are absolute file paths (for
example a local `.gguf`) are reduced to their file name. Because projects and
sessions are not exported, `--devices` only applies to `daily`, `weekly`,
`monthly`, and `today`.

Reports never write to the sync folder. With `--devices all` or a label,
ccstats reads the other devices' files, skips its own (live local data is
used instead), and prices their tokens with this device's pricing table, so
unknown prices stay unknown and estimates stay labeled. Files that are
unparseable, partially synced, or from another schema version are skipped
with a warning on stderr. Deleting a device's file removes it from every
combined report.

The optional TOML config is user-created. ccstats reads the first configured
path documented in the README and fails clearly if that file is malformed.

## Credentials

Cursor credentials are read from `CURSOR_API_KEY` or `CURSOR_SESSION_TOKEN`, or
from a local `credentials.toml` next to the ccstats config directory. They stay
on this machine and are sent only to the corresponding Cursor endpoint. ccstats
does not print them, write them to its cache, or include them in doctor JSON/CSV
output.

When reporting a bug, include `ccstats doctor --json` and the command's stderr.
Do not attach session logs, environment dumps, config files containing secrets,
or Cursor credentials.

Invalid local credentials are reported as an error in CLI and desktop source diagnostics; they are not treated as absent credentials. Interactive credential entry uses hidden terminal input on Unix and Windows.
