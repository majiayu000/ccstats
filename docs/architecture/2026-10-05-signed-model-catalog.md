# Signed model catalog (spec)

Status: proposed, 2026-10-05. Needs owner decisions in [Open decisions](#open-decisions) before implementation.

## Problem

New models and renamed quota windows currently require a code change and a release in ccstats, and then a second release in QuotaBar.

Evidence (git history, 2026-07-01 → 2026-10-05):

- `src/pricing/resolver/fallback.rs` changed in 6 commits (for example `22b23e3` GPT fallback rates, `588368c` Claude fallback rates); Grok 4.7 pricing (`2e2a766`) needed a separate code change in `src/source/grok/`.
- QuotaBar `src-tauri/src/services/claude.rs` `FABLE5_QUOTA_KEYS` changed in 3 commits (`68abfb7`, `6b3c57f`, `e80bd90`) only to follow upstream key renames.
- QuotaBar 0.5.8 existed partly to pick up a refreshed price catalog (release notes, 2026-09-30).

Live prices already come from LiteLLM (`src/pricing/provider.rs`). The gap is the data LiteLLM does not carry or carries late: fallback rates for new models, model alias/family mapping, and quota window key aliases.

## Scope

In scope (data only):

| Section | Replaces | Shape |
|---|---|---|
| `pricing` | entries in `fallback.rs` | exact model id or family prefix → `ModelPricing` fields (USD per token), `source_url`, `effective_from` |
| `model_aliases` | hardcoded alias/family rules | exact string → canonical model id |
| `quota_window_aliases` | QuotaBar `*_QUOTA_KEYS` arrays | provider + canonical window → list of exact upstream keys |

Out of scope, on purpose:

- Endpoints, URLs, file paths, credential locations. A remote document must never decide where ccstats reads secrets or sends requests.
- Regexes or any executable rule. Only exact strings and plain prefixes, so a bad catalog can misprice but cannot change control flow.
- Anything that turns an unknown cost into a number without a provenance label.

## Format

`catalog.json` plus a detached Ed25519 signature `catalog.json.minisig` (minisign format).

```json
{
  "schema": 1,
  "sequence": 42,
  "issued_at": "2026-10-05T00:00:00Z",
  "expires_at": "2026-11-05T00:00:00Z",
  "pricing": [{ "match": { "exact": "grok-4.7" }, "input": 3e-6, "output": 15e-6, "cache_read": 0.75e-6, "source_url": "https://…", "effective_from": "2026-09-20" }],
  "model_aliases": [{ "from": "claude-fable-5", "to": "claude-fable-5-1" }],
  "quota_window_aliases": [{ "provider": "claude", "window": "fable5_weekly", "keys": ["seven_day_fable5", "seven_day_fable_5"] }]
}
```

- `sequence` increases with every publish. Clients reject a catalog whose sequence is lower than the last verified one (anti-rollback).
- `expires_at` bounds how long a stolen-but-old catalog can be replayed. Expired catalogs are ignored, not deleted.
- Hard limits enforced before parsing: 256 KiB file size, 2,000 entries per section, per-token prices in `[0, 1e-3]` USD.

## Trust

- One or two Ed25519 public keys compiled into the ccstats binary (two allows rotation without a gap). No key is ever fetched remotely.
- The private key is held by the owner. CI may sign only if the owner chooses that trade-off (see open decisions).
- What the signature protects against: tampering by the hosting/CDN path, and replay of old documents. What it does not protect against: compromise of whoever holds the private key. If the key lives in GitHub Actions, a GitHub account compromise defeats it; offline signing does not have that weakness.

## Client behavior

1. Fetch on the existing pricing refresh cadence (same live/cache/stale model as LiteLLM, same network opt-outs).
2. Verify in order: size limit → signature against compiled keys → schema version supported → `sequence >= last_verified` → not expired → per-field bounds. Any failure: keep the last verified catalog; if none, use compiled-in fallback. Never partially apply a catalog.
3. Store the verified file and its signature atomically (temp + rename) next to the existing pricing cache.
4. Precedence for a model's price: recorded cost → LiteLLM live/cache → signed catalog → compiled fallback. The compiled fallback stays as the offline floor and is refreshed at release time from the catalog.
5. Provenance: add `PricingSource::Catalog` (`"catalog"`) so JSON, `--debug` and `doctor` show which rows came from the catalog and its `sequence`.
6. SDK exposes `quota_window_aliases` so QuotaBar reads them instead of its hardcoded arrays.

## Publishing

- Separate repository `majiayu000/ccstats-catalog` holding `catalog.json` as source of truth, reviewed by PR.
- Publish job: validate with the same Rust validator (shared crate or `ccstats catalog verify`), sign, upload `catalog.json` + `.minisig` as release assets of that repo; clients fetch the `latest` release asset URL compiled into ccstats.
- A `ccstats catalog verify <file> <sig>` subcommand doubles as the CI check and a user-facing debugging tool.

## Rollout

1. ccstats: validator, verifier, `PricingSource::Catalog`, `catalog verify`; ship with an empty-catalog path tested end to end.
2. Catalog repo with current fallback entries; first signed publish.
3. ccstats reads `pricing` + `model_aliases`; release.
4. SDK exposes `quota_window_aliases`; QuotaBar drops `*_QUOTA_KEYS` arrays after adopting that SDK.

## Tests

- Signature: valid, wrong key, truncated, signature for a different file.
- Rollback: lower sequence rejected; equal sequence accepted (re-fetch).
- Expiry and clock skew (accept up to 10 minutes of future `issued_at`).
- Bounds: negative or absurd price rejected for the whole document.
- Precedence: LiteLLM value wins over catalog; catalog wins over compiled fallback; provenance labels match.
- Offline: no network → last verified catalog, then compiled fallback; no crash, no zero costs.

## Rejected

- Unsigned JSON over HTTPS: no rollback protection and no defense if the hosting path is tampered with.
- Remote regex/rule language: turns a data file into an attack surface on parsing logic.
- Fetching the public key remotely: makes the signature meaningless.
- Pushing model data through QuotaBar's own channel: duplicates trust logic; QuotaBar should keep consuming ccstats.

## Open decisions

1. **Signing custody**: offline signing by the owner (stronger, manual step per publish) or a protected GitHub Actions environment secret (convenient, tied to GitHub account security).
2. **Key generation**: the owner generates the minisign key pair locally (`minisign -G`) and provides only the public key; the private key never enters a repository or chat.
3. **Expiry window**: proposed 30 days.
4. **v1 sections**: proposed `pricing` + `model_aliases` first; `quota_window_aliases` in step 4.
