# Local HTTP API (`ccstats serve`)

`ccstats serve` binds **127.0.0.1 only**. It exposes the same serialized
types as CLI `--json` for analysis, diagnostics, and limits. Do not proxy it
off-loopback.

Default bind: `127.0.0.1:17890`. Override with `--bind`.

| Method | Path | Body / query | Same as |
|--------|------|--------------|---------|
| `GET` | `/v1/diagnostics` | — | `ccstats doctor --json` via `diagnose_usage_sources` |
| `GET` | `/v1/sources` | — | `ccstats sources --json` via `list_usage_sources` |
| `GET` | `/v1/analysis` | `source`, `since`, `until` (YYYY-MM-DD) | `usage_analysis_with_cli_config` |
| `GET` | `/v1/limits` | `source` (`claude`, `codex`, `cursor`, `all`) | `ccstats limits --json` |

Unknown costs are JSON `null`, never `0`. Responses are `application/json`.

Schema stability: handlers return the catalog/CLI structs directly. Tests
assert CLI `--json` and `/v1/*` decode with the same types.
