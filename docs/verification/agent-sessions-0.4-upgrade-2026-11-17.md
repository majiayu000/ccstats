# agent-sessions 0.4 consumer upgrade — 2026-11-17

Decision: adopt the published `agent-sessions = "0.4.0"` dependency in ccstats.
The upgrade must preserve existing accounting and visible session/tool reports.
A changed report or native SQLite link conflict would block adoption.

## Change

- Upgrade from crates.io 0.2.2 to 0.4.0; synchronize root and desktop lockfiles.
- Keep the existing explicit usage/meta/message/tool projections. Do not enable
  Content or snapshot APIs that these product paths do not consume.
- Add a CLI regression containing image, thinking, text and tool blocks plus
  repeated assistant usage: one visible prompt, two requests, 17 input tokens,
  four output tokens and one Read call must remain unchanged.
- Replace one pre-existing exact float assertion in the output-speed test with
  an epsilon assertion so strict Clippy passes. Production Rust code is unchanged.

## Local validation

- Rust 1.95: `cargo test --locked` — 1,095 tests passed, no failures or ignores.
- `cargo clippy --locked --all-targets -- -D warnings` and `cargo fmt --check` passed.
- After the float assertion change, five catalog analysis tests passed.
- Actual Rust 1.89: `cargo check --locked` passed; the declared minimum is unchanged.
- Native dependency graph contains one libsqlite3-sys 0.39.1 through rusqlite 0.41.1,
  shared by ccstats and agent-sessions. No dependency override or private path is used.

## Real-history comparison

The same private snapshot of one existing Claude transcript (1,990,216 bytes)
was processed by baseline 0.2.2 and registry 0.4.0 binaries. Both ran offline,
with caches disabled and timezone UTC. Daily, session details and tools output
were compared as complete canonical JSON. Only the set-like tool rows were
sorted by name; no field or value was dropped.

All three report hashes matched exactly. Details reported 93 requests and zero
parse errors; tools reported 95 calls across three tool names. Original input
bytes, size and modification time remained unchanged. No transcript content or
private source path is committed. The transient private snapshot was removed.

## Boundaries

Windows and macOS desktop integration use the repository PR CI; the local checks
above do not claim their results. No installed app was replaced or released.
One Claude transcript does not establish all-history compatibility or downstream
adoption of new Content/Pi/Kimi snapshot APIs. Synthetic regressions cover adjacent
Claude/Codex accounting cases; native desktop UI and long-running daily use remain
separate acceptance steps. No product feature or event projection was added.
