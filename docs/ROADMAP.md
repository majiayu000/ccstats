# ccstats roadmap

North star: **the most accurate local AI coding ledger, and one that can tell you how much is left.**

Keep: provenance (`Real` / `EstimatedProxy` / unknown is never a silent zero).
Fill: official quotas, burn rate, incremental parse performance, a reusable parse layer.
Do not chase source count or leaderboards.

## Phase 4 decision (2026-09-17)

**B first: TUI / `ccstats watch`, not a signed desktop tray.**

Apple Developer ($99/year) plus Windows Authenticode is an operational cost, not a code problem. Issue [#154](https://github.com/majiayu000/ccstats/issues/154) stays open until a maintainer buys and configures signing secrets. Until then:

- CLI + `ccstats watch` is the real-time surface.
- Desktop installers may keep shipping ad-hoc/unsigned; they are not the default path.
- A tray/menu-bar app (option A) can follow after watch is solid and certificates exist.

## Sequence

```
Phase 0 (repo hygiene) ──▶ Phase 1 (usage-facts cache)
                 │
                 └──▶ Phase 2 (statusline hook, quota snapshots, limits, watch)
                              │
                              └──▶ Phase 3 (verify / serve) ──▶ Phase 4 (TUI)
```

Phase 2a (statusline stdin) does not depend on the cache.

## Explicitly out of scope

- 50+ sources or public rankings
- LLM summaries inside the CLI
- Reading Cursor local SQLite credentials
- Writing estimates as if they were billed cost
