# Choose a ccstats report

Use ccstats to answer a usage question from the data available on your machine.
[Install the CLI](../README.md#install) first. These examples use an illustrative
date range; replace it with the period you want to inspect.

## How much did Claude Code and Codex use in a date range?

```sh
ccstats daily --source claude --since 20260901 --until 20260915
ccstats daily --source codex --since 20260901 --until 20260915
ccstats weekly --source all --since 20260901 --until 20260915
```

`weekly` groups the filtered history; it does not mean “this week” unless you
bound the dates. `--source all` includes source subtotals. Start with one source
when investigating a missing or unexpected total, then compare the combined
report. [Source paths and overrides](sources.md) explain where each input comes
from; `ccstats doctor` checks availability without parsing conversations.

## Is an estimated dollar total the bill or remaining quota?

No. Usage cost, source-recorded cost, and provider quota are different evidence.
Missing pricing stays `unknown` / `N/A`, rather than becoming zero.
`--strict-pricing` disables pricing fallbacks; `--cost-source both` shows the
local and Claude Code session-cost views where available. Use `ccstats verify`
for the source-recorded cost comparison.

```sh
ccstats limits
ccstats watch --once
```

These commands show available quota windows and label estimates. `watch --once`
can exit non-zero when a window is hot; do not interpret that alone as a parser
failure. For a desktop menu-bar view backed by this SDK, see
[QuotaBar](https://github.com/majiayu000/quotabar). It is a separate application.

## How do I export a report without opting into prompt text?

```sh
ccstats daily --source codex --since 20260901 --until 20260915 --json
ccstats daily --source claude --since 20260901 --until 20260915 --csv
```

Ordinary usage reports export accounting metadata. Session IDs, project paths
and model names can still be sensitive. The separate opt-in
`session --json --details` envelope includes the first user prompt; inspect its
[schema and scoping flags](architecture/session-details-json.md) before sharing.
See the [privacy and network boundary](PRIVACY.md), including the distinction
between offline local sources and Cursor's API-backed source.

[All commands and flags](../README.md#usage) ·
[Rust SDK and local HTTP API](api.md) ·
[Report a reproducible issue](../CONTRIBUTING.md#reporting-issues)

## Why did Claude Code usage increase?

Run `ccstats diagnose` for a rolling five hours, `--window today` for the elapsed
local day, or `--window 7d`. Add `--session ID` to include one session and its
encoded child files. `--versions` limits text output to version comparisons;
`--json` always returns the whole evidence report. This command uses Claude
logs and saved statusline observations, and does not fetch pricing.
[Diagnosis contract and limitations](diagnose.md).
