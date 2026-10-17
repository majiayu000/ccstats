# Claude Code / Codex model accounting audit — 2026-10-17

Baseline: ccstats 0.11.0, main `b52a4e123bd7a324dfa06184850c33277b6c1a6e`.
Tracking: [issue #211](https://github.com/majiayu000/ccstats/issues/211).

Recent model IDs are accepted by both source parsers; the reproduced failures concern API-equivalent cost estimates, not missing token totals. The patch adds three model prices and retains Haiku 5.5's per-request >100K token tier through aggregation.

## Verified model matrix

Tests cover 13 model IDs plus provider-prefix and dated-ID variants, using native synthetic usage records with known totals. Both the built-in fallback and an isolated external pricing cache are exercised.

| Model | Baseline token accounting | Baseline built-in pricing | Result after fix |
| --- | --- | --- | --- |
| GPT-6.1 Sol | Correct | Missing (`null` cost) | Correct standard and >272K rates |
| GPT-6 Astra / Sol / Luna | Correct | Correct for tested cases | Regression tests pass |
| GPT-5.6 Sol | Correct | Correct for tested cases | Regression tests pass |
| Claude Sonnet 5.5 | Correct | Older Sonnet rates | Correct own rates, including cache reads |
| Claude Haiku 5.5 | Correct | Older Haiku rates; missing >100K tier | Correct both tiers, including 5m/1h cache writes |
| Claude Opus 5.5 | Correct | Correct for tested cases | Regression tests pass |
| Claude Fable 5.1 / Mythos 5.1 | Correct | Correct for tested cases | Regression tests pass |
| Claude Opus 4.8 / Sonnet 5 / Haiku 4.5 | Correct | Correct for tested cases | Regression tests pass |

Mythos is a limited-access model. Fixture support does not establish account access. A dated GPT-6.1 model string is used only to test the existing date-suffix resolver, not to claim a published snapshot ID.

## Reproduced failures

For input=1,000, output=50, cache read=200 and cache writes=100 (60 at 5m, 40 at 1h), both Claude models retain the expected 1,350 total tokens:

| Model | Baseline cost (USD) | Correct cost (USD) |
| --- | ---: | ---: |
| Sonnet 5.5 | 0.004275 | 0.00283 |
| Haiku 5.5 | 0.00114 | 0.0001425 |

The Haiku short-request estimate was 8x too high. Four Haiku requests with input-side totals of 99,999 / 100,000 / 100,001 / 300,300 tokens cost $0.2203004 together; baseline fallback produced $0.48064. The threshold includes uncached input, cache reads and cache writes, and applies separately to each request, not to a daily total.

The downloaded LiteLLM price list already has all three new models. Consequently, fresh online/cached prices can resolve GPT-6.1 Sol and Sonnet 5.5 without the fallback fix. However, baseline ccstats ignored LiteLLM's `*_above_100k_tokens` fields, so Haiku long requests remained underpriced even with fresh prices. The patch fixes that ingestion path as well.

## Implementation and checks

- Add the three verified prices to the existing fallback resolver.
- Preserve the >100K subset alongside the existing >272K subset in `Stats` / `CostTokens`, before aggregation; carry it through addition, subtraction, serialization and real/proxy partitioning.
- Read the corresponding external price fields and include them in alias-conflict comparison. Keep recorded provider costs authoritative.
- Native source parsers, public CLI flags, error handling, unknown-model behavior and `--strict-pricing` semantics are unchanged.

`tests/cli_latest_models.rs` contains seven grouped CLI regressions. They cover the model matrix, cold/warm usage-cache reads, `--no-cache`, fallback versus external prices, Codex `token_count` and response-only `token_usage_record`, duplicate-counter/coexisting-response suppression, Claude cache TTLs, exact Haiku tier boundaries, and unknown/strict pricing. Three additional unit tests cover GPT-6.1's 272K boundary, Haiku serialization/recorded-cost/proxy partitioning, and conflicting 100K price aliases.

Completed locally on macOS with rustc 1.97.0:

- `cargo fmt --all --check`
- `cargo check --locked`
- `cargo clippy --locked --all-targets -- -D warnings`
- `cargo test --locked`: 1,085 passed; 0 failed; 0 ignored, including 804 library tests.
- `git diff --check`

A read-only sample of local native logs was also replayed from isolated temporary directories after removing prompt/tool content and hashing identifiers: 24 Claude files and 30 Codex files. Both returned zero parse errors. Observed Opus 5.5, GPT-6.1 Sol, GPT-6 Astra and GPT-6 Luna usage remained priceable. No private logs or local usage totals are committed. Sonnet 5.5 and Haiku 5.5 were validated with deterministic fixtures, not live paid inference.

## Interpretation and limits

These are standard-rate API-equivalent token estimates. They do not establish an exact subscription charge or include Fast/Ultrafast, regional, batch, negotiated or tool-use billing adjustments. Upstream model/price data can change after this audit. Missing log fields cannot be reconstructed by adding model prices.

Old exported sync aggregates lack the new >100K request subset; regenerate those snapshots from source logs with the updated build to use the new tier. Ordinary local usage caches store raw entries and recompute this subset without a schema migration.

## Evidence

- [OpenAI model availability and CLI IDs](https://learn.chatgpt.com/docs/models)
- [OpenAI standard and long-context API prices](https://developers.openai.com/api/docs/pricing)
- [GPT-6.1 Sol specifications](https://developers.openai.com/api/docs/models/gpt-6.1-sol)
- [Claude Sonnet 5.5 model and cache prices](https://platform.claude.com/docs/en/models/sonnet-5-5/overview)
- [Claude Haiku 5.5 prices and prompt-length threshold](https://platform.claude.com/docs/en/models/haiku-5-5/overview)
- [Claude pricing, cache multipliers and long-context rules](https://platform.claude.com/docs/en/about-claude/pricing)
- [LiteLLM pricing data](https://github.com/BerriAI/litellm/blob/main/model_prices_and_context_window.json), downloaded 2026-10-17; the minimal standard-rate extract used by tests is `tests/fixtures/pricing/latest-models.json`.
