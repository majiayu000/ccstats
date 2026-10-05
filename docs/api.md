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

### Limit forecast

Every `/v1/limits` (and `limits --json`, `watch --json`) `windows[]` entry
carries a `forecast` object. The SDK exposes the same type as
`ccstats::LimitForecast`, computed by the pure `ccstats::forecast_limit`.

```json
"forecast": {
  "burn_pct_per_hour": 10.0,
  "projected_exhaustion_at": "2026-10-05T17:50:09Z",
  "exhausts_before_reset": true,
  "basis": "snapshot_history",
  "confidence": "medium",
  "samples": 3,
  "source": "estimated",
  "reason": null
}
```

- `basis`: `snapshot_history` is a least-squares slope over at least 3
  observations of the current reset window that cover at least 15 minutes.
  Its confidence is `medium`. `window_average` is `used_pct` divided by the
  time since the window started, used only after 15 minutes or 5% of the
  window. Its confidence is `low`.
- Data per provider: Claude uses opt-in statusline quota snapshots. Codex uses
  the newest weekly snapshot of each recent session file. Cursor has no saved
  history, so it uses the billing-cycle average.
- `reason` is set whenever `projected_exhaustion_at` is `null`:
  `missing_used_pct`, `missing_reset_time`, `window_reset`,
  `insufficient_history`, `not_increasing`, `already_exhausted`. If the
  forecast is unknown, the burn rate and run-out time are `null`, never `0`.
- Snapshots from an earlier reset window, or from before a drop in used
  percentage, are ignored.
- `watch --json` adds `exhaustion_warning: true` when an official window is
  projected to run out before reset. The `--warn-pct` exit code is unchanged.

Schema stability: handlers return the catalog/CLI structs directly. Tests
assert CLI `--json` and `/v1/*` decode with the same types.
