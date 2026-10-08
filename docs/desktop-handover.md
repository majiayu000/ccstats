# Desktop handover evidence — 2026-10-07

The original attachment's QuotaBar/ccstats scope is retained in full. This is an
implementation and acceptance record, not approval to remove existing workflows.

## Product ownership

- agent-sessions 0.2.2: Claude/Codex native discovery, framing and parsing. This task does not change that repository.
- ccstats: usage deduplication, statistics, pricing, CLI/SDK and report contracts.
- QuotaBar: provider login/quota transport, tray, desktop analysis, notifications and settings.

QuotaBar PRODUCT.md at a7191407 explicitly preserves ccstats desktop until the
handover is verified. ccstats a93a659 had removed desktop CI/installers before
that verification. This change restores the existing CI matrix, five installer
builds, checksum staging, version checks and signed/notarized macOS release gates.
The original desktop diagnostics, model/tool investigation, limits, live monitoring
and SQLite/JSON machine snapshot code remains intact.

## Acceptance matrix

| Original requirement | Implementation / evidence | Remaining acceptance |
| --- | --- | --- |
| DPI, font scaling, multiple monitors (#186) | QuotaBar #194 already shipped 100/125/150% app zoom; existing placement tests cover system DPI, negative monitor origins, side/top/hidden taskbars; an explicit 2560×1440 zoom/DPI matrix is added | Windows 10, physical 27-inch 2K, mixed-DPI monitors and system text-only scaling remain unverified |
| Expired connection, stale data, partial reads, login recovery | Existing QuotaBar recovery/freshness suites retained; failed diagnostic sources remain report errors, daily costs/tokens stay unknown or lower bounds; native discovery errors now surface in ccstats diagnostics | Cross-repository changes require publication of the new SDK before installed QuotaBar receives its additional diagnostics |
| Cost → session → original source | Session drilldowns and `session --json --details` retain Claude/Codex file paths from the actual contributing usage; Codex display grouping preserves multiple files after filtering; pricing source/coverage remains visible | Other source adapters without reliable file identity return an explicit empty provenance list; no fabricated path. QuotaBar installed SDK needs the next ccstats release |
| Preserve diagnostics and device workflow | ccstats desktop CI and release pipeline restored; QuotaBar source diagnostics and cost/session analysis retained | Device snapshot exchange, live, tool-turn investigation and limits/budget do not yet have complete QuotaBar parity. Desktop remains available; handover is not accepted |
| Installation, use and release delivery | Restored release metadata/staging; README/development and release instructions updated | No new tag, signed installer, public release or maintainer/user confirmation is claimed |

## Source-path contract

`SessionDrilldown.source_paths` lists sorted unique native files contributing to
the selected range/model/project, after deduplication. Duplicate usage discarded
by deduplication does not establish a second contribution. Claude/Codex metadata
and titles still come through agent-sessions; this change introduces no parser or
price table in QuotaBar. Other sources return an empty list, meaning location is
unavailable, not that the usage has no source.

Rust callers constructing `SessionDrilldown` directly must supply `source_paths`.
Choose the next version according to this public SDK change before publication.

The CLI's opt-in `session --json --details` adds `source_paths`; ordinary session
JSON remains unchanged. Paths are local provenance, not transcripts. Do not
include private paths/transcripts in public screenshots or release evidence.

## Environment and delivery boundaries

Two task-start snapshots and clean isolated worktrees preserve both original
checkouts. Baselines: ccstats 687c6d9 and QuotaBar a7191407.

Tailscale status exposed a Windows peer, `win-2f0pbt20sgn`. The requested
`Tailscale ssh ... whoami` failed with `Host key verification failed` before
remote commands ran. No LAN/remote desktop substitute was used. A verified host
key and working SSH account are required to continue native device acceptance.
No Windows 10 or physical display result is inferred from geometry fixtures.

GitHub CLI GraphQL returned HTTP 401; REST subsequently hit its anonymous rate limit.
Git push could not obtain HTTPS credentials. The connected GitHub API read PR #194
and #206 and created the tested Git trees successfully. Delivery uses that API,
comparing remote tree hashes with the local commits before creating branch refs.

## Verification

Completed on this task branch:

- `cargo test --locked`: 1,049 passed across library/CLI/SDK suites.
- `cargo clippy --locked --all-targets -- -D warnings`: passed after moving the new test module to the end of its file.
- `cargo fmt --check` and desktop Rust formatting: passed.
- `cargo deny check`: advisories, bans, licenses and sources passed.
- `scripts/check-release.sh`: version lockstep passed for 0.9.1.
- `python3 scripts/stage-desktop-artifacts.py --self-test`: passed for DMG/MSI/AppImage staging and SHA-256 sidecars.
- `cargo publish --dry-run --allow-dirty --locked`: package verification passed; upload explicitly aborted by dry-run. The version is already published, so a new version is required for actual publication.
- Desktop `npm run build`: passed.
- Desktop `npm run test:e2e`: 36 passed using the explicit synthetic renderer bridge, including diagnostics and machine snapshot exchange.
- Desktop `cargo test --manifest-path desktop/src-tauri/Cargo.toml`: 15 passed.
- Desktop `cargo clippy --manifest-path desktop/src-tauri/Cargo.toml --all-targets -- -D warnings`: passed.
- QuotaBar `npm test`: 675 passed; `npm run build` and `npm run release:check` passed.
- QuotaBar native Rust: 166 passed, 10 pre-existing manual/ignored tests skipped.
- Final QuotaBar Rust regression: 166 passed, 10 skipped, including existing SDK alias selection.
- QuotaBar installation lifecycle: 15 passed. Full initial runs hit the fixture's 10-second process watchdog; the test harness now allows 30 seconds. Production stop/rollback behavior is unchanged.

QuotaBar's full Rust suite also passed with this local ccstats SDK override: 166
passed, 10 pre-existing manual/ignored tests skipped. The published dependency
lockfile was restored after verification. Source selection uses the SDK's parsed
source identity, retaining its existing aliases. Source diagnostic failures retain
any readable portion and mark the result incomplete.
The final targeted SDK integration also passed after the alias/readable-portion
review (`real_sdk_report_retains_session_identity_titles_and_usage`); the
published lockfile was restored and checked afterward.

The native IPC script was stopped during compilation before app launch after
review showed it drives the desktop UI through WebDriver. The task explicitly
does not authorize desktop UI operation; no native UI result is claimed. Native
Rust tests are separate and passed. Logs and original snapshots are retained under
`/tmp/quotabar-ccstats-20261007/`; they are task evidence, not published artifacts.

### Installed data observation

A temporary Rust executable linked this task's ccstats SDK and called its actual
`usage_analysis_with_cli_config` for Today in offline mode. It reconciled summary,
session and daily token totals and checked every returned source path with the
filesystem, without printing paths or transcript content. Codex: 32 sessions,
57 contributing files, 487,730,985 tokens, 0 parse errors. Claude: no records in
Today, so this observation provides no non-empty Claude acceptance. This is a
local data observation, not Windows UI acceptance or subscription-bill verification.

## Follow-up — 2026-10-08

QuotaBar #211 is merged. ccstats #207's Windows CLI job failed only because the
source-file assertion compared path strings with different Windows separators.
It now compares native path identity and still requires exactly one file.
The Check and macOS jobs never started: GitHub reported repeated runner acquisition
failures. The Windows desktop job, including its isolated native IPC test, passed;
this hosted-runner result does not establish Windows 10/physical-display acceptance.

The branch incorporates main's released 0.10.0 SDK and #206 accounting/WAL fixes,
preserving discovery-error counts alongside explicit source diagnostics. Release
metadata is aligned at 0.11.0 because the new public Rust struct field breaks
0.10.x literal construction. The desktop release pipeline stays intact.

Fresh checks: 1,075 Rust tests; 15 desktop Rust tests; 36 synthetic renderer E2E
tests; frontend build; root Clippy with all targets/features and warnings denied;
cargo-deny; formatting; release metadata; staging self-test; package dry-run.
The final remote head must also pass CI before release. QuotaBar must resolve the
published SDK from crates.io before its installed dependency handover is accepted.
Windows SSH identity, user-device UI testing and full desktop-workflow parity remain
unverified; no access-method or signing-gate workaround is introduced.
