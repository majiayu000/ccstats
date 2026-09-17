# Kimi Code source

Kimi Code support reads per-turn `usage.record` entries from wire logs under
`~/.kimi-code/sessions/`, including sub-agent usage, and reports actual
input/output/cache token usage per turn.

```bash
ccstats kimi today
ccstats kimi
ccstats daily --source km
```

By default, ccstats reads:

- `~/.kimi-code/sessions/*/*/agents/*/wire.jsonl` (main and sub-agent per-turn `usage.record` entries)
- `~/.kimi-code/session_index.jsonl` for session-to-project mapping

Override the Kimi Code home directory with `KIMI_CODE_HOME`.

Current limitations:

- Kimi Code subscription models (e.g. `kimi-code/k3`) have no public per-token pricing; costs use fallback estimates based on Moonshot's official `kimi-k2.6` API rates and are marked as `fallback` in structured output. Use `--strict-pricing` to show N/A instead.
- Cache creation tokens are reported but priced at $0 by the Kimi fallback estimate (Moonshot does not publish a separate cache-creation rate).
- Kimi estimated session windows (`blocks`) and tool-call statistics are not supported.
