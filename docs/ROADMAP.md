# ccstats roadmap

North star: **the most accurate local AI coding ledger, and one that can tell you how much is left.**

Keep: provenance (`Real` / `EstimatedProxy` / unknown is never a silent zero).
Fill: official quotas, burn rate, incremental parse performance, a reusable parse layer.
Do not chase source count or leaderboards.

## Phase 4 decision (2026-09-17)

**B first: TUI / `ccstats watch`. The GUI/tray surface is
[QuotaBar](https://github.com/majiayu000/quotabar).**

- ccstats stays CLI + `ccstats watch` + SDK + `ccstats serve`.
- QuotaBar is the only GUI. It is a separate menu-bar app that consumes the
  published ccstats SDK.
- The ccstats desktop app under `desktop/` is deprecated. Its code stays in the
  repository, but it is no longer built in CI or attached to releases.

## Sequence

```
Phase 0 (repo hygiene) ──▶ Phase 1 (usage-facts cache)
                 │
                 └──▶ Phase 2 (statusline hook, quota snapshots, limits, watch)
                              │
                              └──▶ Phase 3 (verify / serve) ──▶ Phase 4 (TUI)
```

Phase 2a (statusline stdin) does not depend on the cache.

## Device sync (2026-10)

**Done:** `ccstats sync push|status` and `--devices all|this|<label>` combine
devices through a user-chosen sync directory. No cloud service, no network:
aggregate `date × source × model` files only. Sessions, projects, and blocks
stay per device.

## Explicitly out of scope

- 50+ sources or public rankings
- LLM summaries inside the CLI
- Reading Cursor local SQLite credentials
- Writing estimates as if they were billed cost
- A hosted sync service or account for multi-device rollups
