# ccstats roadmap

North star: **the most accurate local AI coding ledger, and one that can tell you how much is left.**

Keep: provenance (`Real` / `EstimatedProxy` / unknown is never a silent zero).
Fill: official quotas, burn rate, incremental parse performance, a reusable parse layer.
Do not chase source count or leaderboards.

## Phase 4 decision (2026-09-17)

**B first: TUI / `ccstats watch`, then a signed desktop tray.**

Developer ID and App Store Connect notarization secrets are now configured.
Desktop GitHub Releases fail closed without them:

- CLI + `ccstats watch` remains the real-time surface.
- macOS desktop installers on a `v*` tag are Developer ID signed and notarized.
- A tray/menu-bar app (option A) can follow after watch is solid.

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
