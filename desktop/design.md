# ccstats Polar Workbench

## Product idea

ccstats is a local analytical instrument, not a report, landing page, terminal, or collection of dashboard cards. The interface should feel like a precise native desktop workspace for tracing AI usage back to sources, projects, sessions, models, and dates.

## Reader job

A developer opens the app to answer five questions quickly:

1. How much usage was recorded in this period?
2. Which token components, sources, or models explain the total?
3. Can the cost and records be trusted?
4. Which project, session, or day should be inspected next?
5. Is a remote-machine snapshot current enough to contribute to this window?

Missing prices, unsupported project identity, malformed records, and dates without activity remain explicit. Never fabricate continuity or certainty.

## Composition

- Use a permanent left rail for product identity, four task groups, source selection, and the local-only privacy state.
- Observe contains Overview, Live, and Top consumers. Explain contains Turns & tools, Projects, and History. Trust contains Cost evidence, Limits, and Diagnostics. Devices contains Machines.
- Use the remaining window as a task-specific workspace. The top toolbar owns the period and refresh action.
- Overview is organized around a Token Map. The total and token composition share one focal surface, while cost, records, and cache status form a narrow diagnostic column.
- Projects is a master-detail workspace. Selecting a project reveals its sessions in an adjacent inspector; do not expand rows inside a ledger.
- History is a chart workspace with export actions in its header and the exact daily ledger below it.
- Tables live inside their owning work pane. Long evidence scrolls locally instead of widening the whole window.
- On narrow screens, the left rail becomes a compact header and master-detail layouts stack.

## Visual language

- Use an ice-white workspace with a pale blue-gray navigation rail, ink-blue text, cobalt selection, amber unknown states, and red errors.
- Surfaces are functional panes with quiet elevation and 8px to 12px radii. Do not recreate the previous border-only report composition.
- Manrope is used for interface language. IBM Plex Mono is reserved for values, timestamps, paths, model names, and session IDs.
- Navigation labels and controls use sentence case. No fake terminal copy, tracked uppercase labels, neon color, grid texture, black report canvas, oversized editorial hero, or decorative effects.
- Density is controlled and desktop-native: 44px controls, 13px body text, 12px metadata, and tabular numeric alignment.
- Motion is limited to hover, selection, and loading state continuity.

## Evidence rules

- Token components share one total and one scale. Use a proportional map, not independent progress bars.
- Unknown cost is amber and always includes an explanation.
- Healthy quality is compact. Warnings become a dedicated diagnostic state.
- Text columns align left; quantities and monetary values align right.
- Project selection must be visible through shape, background, and text weight, not color alone.
- History bars share a zero baseline and reserve equal time slots. Exact values remain available in the table.
- Source and model attribution preserve every returned row and never hide zero or unknown values.
- All Sources aggregates only ledgers that discovery marks detected or configured. Empty registered sources do not change aggregate pricing provenance.
- A provider-reported total adjustment is first-class reconciliation evidence. Positive differences use an unallocated segment; negative differences remain an explicit note instead of distorting named buckets.
- Partial API-equivalent cost is always labeled as a lower bound with priced-token coverage. Fallback, stale-cache, proxy, and mixed evidence is marked for review rather than presented as exact spend.
- Machine Today, This week, and This month windows are checked independently against the CLI-configured timezone. A stale range stays visible but contributes to neither that range's token total nor its canonical USD cost total.
- Exact-range pages may attribute parse errors to their range. Combined multi-range scans must label parse errors as unattributed.

## Mechanical checks

- At 1440 by 1000, navigation, source, period, total usage, cost status, records, and quality are visible without scrolling.
- At 390px wide, the document has no horizontal page overflow; tables may scroll within their pane.
- All controls retain visible keyboard focus and expose native labels or accessible names.
- Every top-level request has an identity guard so a slower prior source, range, or page cannot overwrite the current selection.
- Overview, Projects, History, Live, Cost evidence, Limits, Diagnostics, Activity, and Machines use the real bridge data and preserve loading, empty, error, malformed, stale, partial-cost, and unsupported states.
