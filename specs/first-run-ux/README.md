# First-run UX specs

Parent product spec: [`product.md`](product.md)

This pack is the implementation contract for the first-run and decision-surface work. Each feature is a separate GitHub issue and PR.

| Feature | Specs | Suggested PR base | Parallel? |
| --- | --- | --- | --- |
| F1 Auto-detect default source | #164 [F1-auto-detect/](F1-auto-detect/product.md) | `origin/main` | Yes |
| F2 README + GitHub identity | #165 [F2-identity/](F2-identity/product.md) | after #164 for the default-command sentence | Docs-only |
| F3 Conclusion line | #166 [F3-conclusion/](F3-conclusion/product.md) | `origin/main` | Yes |
| F4 `login cursor` | #167 [F4-cursor-login/](F4-cursor-login/product.md) | `origin/main` | Yes |
| F5 `limits` | #168 [F5-limits/](F5-limits/product.md) | #172 login branch, then retarget to `main` after #172 merges | Requires F1/F4 integration checks |

## Command grammar (this wave)

Canonical form: `ccstats <view> [--source <name>]`.

Keep `ccstats codex|grok|kimi` as aliases. Do **not** add `ccstats cursor` or other new source subcommands. F4 uses `ccstats login cursor`.

## Out of scope

Menu bar, desktop IA collapse, long-tail sources, Tokscale-style leaderboards, `ccstats why`.
