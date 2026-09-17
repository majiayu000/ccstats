# Grok source

Grok support reports complete, globally deduplicated token totals from session
`turn_completed.usage` records. It separately prices each observed
`shell.turn.inference_done` request using xAI's public short- or long-context
API rate. Because Grok trims `unified.jsonl`, ccstats shows the exact observed
API equivalent, a coverage-adjusted estimate, and a short/long-context range.
It also displays `costUsdTicks` as a separate provider metric. Neither value
is labeled as the user's actual subscription charge, which is unavailable in
the local logs.

```bash
ccstats grok today
ccstats grok
ccstats daily --source gx
```

By default, ccstats uses:

- `~/.grok/sessions/**/updates.jsonl` for complete per-turn token totals
- `~/.grok/logs/unified.jsonl` for per-inference API-equivalent pricing
- `~/.grok/sessions/**/summary.json` for model, project, and session metadata
- the platform application-data directory under `ccstats/grok/` `inference-v1.jsonl` for the durable, deduplicated ledger

For Grok 4.5 and 4.6, requests below 200k prompt tokens use the short-context
rates. Requests at or above 200k use the long-context input, cached-input, and
output rates for the entire inference. `completion_tokens` already includes
reasoning, so ccstats does not charge reasoning twice. Rates follow the
[xAI pricing reference](https://docs.x.ai/developers/pricing).

Structured period output includes the backward-compatible
`api_equivalent_cost_coverage` object plus `grok_cost_summary`. The latter
separates `api_equivalent` observed/estimated/range values, the
provider-reported `costUsdTicks` metric and its token coverage, and
`actual_billed_usd: null`. If request telemetry and complete turn totals
cannot be reconciled, coverage is marked `mismatch` and ccstats does not
publish an estimate. If a session has no `turn_completed.usage`, ccstats
retains its explicitly labeled `estimated_proxy` context-snapshot fallback.

Override the Grok home directory with `GROK_HOME`.

Current limitations:

- The durable ledger starts when ccstats first observes an inference. It cannot recover records Grok trimmed before the first run or between ccstats runs, so incomplete request coverage produces an estimated API equivalent and a short/long-context range.
- `turn_completed.usage.costUsdTicks` is reported separately as a provider metric. xAI does not document this field as public API list price or as the user's actual subscription charge.
- Grok models without a published ccstats per-inference tier remain unpriced and reduce priced-token coverage.
- Grok estimated session windows (`blocks`) are not supported.
