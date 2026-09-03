# ccstats desktop

The desktop app is a local investigation surface over the same source registry
and summary APIs as the CLI. It does not upload transcripts or substitute mock
data when a source fails.

Production installers (DMG, MSI, AppImage) are attached to
[GitHub Releases](https://github.com/majiayu000/ccstats/releases). Download
those if you only want to run the app. This file is for from-source development
and local packaging.

## Prerequisites

Node.js 20.19+ (or 22.12+), Rust 1.88+, and the platform prerequisites for
[Tauri 2](https://v2.tauri.app/start/prerequisites/).

## Development

From the repository root:

```bash
cd desktop
npm install
npm run tauri -- dev
```

The app discovers detected or configured sources at startup and opens the first
ready ledger. If none are ready, it opens Diagnostics instead of presenting an
empty default source. “All Sources” scans ready ledgers only; all 29 registered
sources remain available for explicit inspection and setup.

The workspace is organized by the questions a usage investigation asks:

- **Observe** — totals, live 15-second monitoring, top consumers, and spikes.
- **Explain** — model turns, tool calls, projects, sessions, and daily history.
- **Trust** — pricing provenance, API-equivalent coverage, Codex quota, budget,
  and source readiness.
- **Devices** — explicit JSON snapshot exchange and a local SQLite rollup.

Unknown, partial, fallback-priced, malformed, and provider-adjusted values stay
visible as evidence states. Live, History, Limits, and Machines treat only real
usage backed by recorded, live, or fresh cached pricing with complete coverage
as exact cost. Machine totals use canonical USD and evaluate Today, This week,
and This month freshness independently using the configured CLI timezone.

## Local packaging

```bash
cd desktop
npm ci
npm run tauri -- build
```

macOS produces a DMG, Windows an MSI, and Linux an AppImage. Production
installers are built from `desktop/` by the tag-triggered Release workflow. See
[docs/RELEASING.md](../docs/RELEASING.md) for GitHub Release artifacts.

## Tests

```bash
cd desktop
npm run build
npm run test:e2e
cargo test --manifest-path src-tauri/Cargo.toml
npm run test:e2e:native
```

Playwright exercises the renderer contract by injecting an explicit window bridge
before the page loads. Rust tests cover the command boundary, and the native test
launches a debug app through an embedded WebDriver before crossing Tauri IPC into
the real ccstats SDK. Production builds call those commands directly and have no
sample-data fallback.

Usage files and transcripts remain local; pricing refreshes and sources configured
with remote APIs may still make their documented network requests.
