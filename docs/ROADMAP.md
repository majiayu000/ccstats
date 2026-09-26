# ccstats roadmap

Direction confirmed 2026-09-27: trustworthy local statistics, pricing, CLI,
SDK and machine interfaces. QuotaBar owns the user-facing tray, desktop
analysis, alerts and settings. Keep the repositories independently usable.

## Ownership

| Layer | Owns | Changes go here |
|---|---|---|
| [agent-sessions](https://github.com/majiayu000/agent-sessions) | Native Claude/Codex framing, projections, original evidence and completion semantics | Native format and provenance fixes, synthetic format fixtures |
| ccstats | Source accounting, deduplication policy, pricing, reports, CLI/SDK/HTTP contracts | Model prices, source adapters, coverage/error reporting, output contracts |
| [QuotaBar](https://github.com/majiayu000/quotabar) | Provider quota connections, tray, desktop analysis, notifications, settings | User interaction, display, login recovery and OS integration |

CLI `limits`, `watch` and `statusline` stay available. This boundary does not
remove existing SDK APIs or commands. QuotaBar adopts published SDK versions
with its own lockfile and consumer tests; no copied parser or pricing table.

## Delivery priorities

1. Preserve provenance and accounting: missing data is not zero; API-equivalent
   estimates are not subscription bills; partial coverage and parse failures stay
   visible. Keep source-specific policies out of the native parser.
2. Maintain versioned machine contracts and parity between CLI, SDK and HTTP
   views of the same filtered input. Use synthetic fixtures with independent
   expected values for duplicate records, cache buckets, ledgers and partial tails.
3. Keep pricing and incremental parsing reliable for existing consumers. Add
   sources when backed by user demand, fixtures and upstream format evidence.
4. Consolidate desktop delivery through QuotaBar using the handover below.

## Desktop handover gate

The old plan to add a separate ccstats tray is withdrawn. Do not add another
tray, notification system or competing desktop settings surface here.

The current desktop and its signed macOS/unsigned Windows distribution remain
maintained. Do not remove them until QuotaBar can cover the required workflows:

| Existing ccstats desktop workflow | Handover evidence required |
|---|---|
| Source discovery and diagnostics across the registered inventory | Supported source/setup/error behavior compared using the same fixtures |
| Project, model, session and history investigation | Equivalent filters, unknown/partial pricing states and usable navigation |
| Devices: JSON snapshots and local SQLite rollup | Import/export, identity/deduplication, time ranges and user data preserved |
| Installation and existing settings | Migration instructions and fresh-install/upgrade checks on supported OSes |

This checklist is a gate, not a claim of completed parity. Keep existing desktop
checks until a tested handover and documented maintenance/migration decision.
New analysis should be exposed through the SDK for QuotaBar to render.

## Out of scope

- Source-count targets or public usage leaderboards
- LLM summaries inside the CLI
- Reading Cursor local SQLite credentials
- Treating estimated cost as billed cost
- Telemetry or hosted billing introduced as part of this boundary change
