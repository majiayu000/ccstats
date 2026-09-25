# Weekly value SDK extraction

The QuotaBar vendored patch adds shared statistics behavior that belongs in
ccstats, not the transcript reader. This change upstreams that behavior before
removing QuotaBar's pinned SDK archive.

- Preserve general usage for gpt-reserve, while excluding that independent pool
  from subscription-week estimation. Do filtering after cumulative parsing.
- Ignore snapshots explicitly identifying a different quota pool.
- Calibrate per-model capacity only from unambiguous single-model spans with a
  sufficient percentage change, within the same reset window. Missing calibration
  returns None, never a made-up zero capacity.
- Expose model sample counts/percentages and API-price-equivalent Astra conversion
  through the SDK. These are local estimates, not provider-authoritative quotas.
- Preserve unknown-pricing errors and finite-value checks.

Source: QuotaBar vendor/ccstats-weekly-reserve.patch at 578667f. Its separate Grok
patch is already implemented in ccstats 2e2a766 and is not reapplied.

Public struct fields require a 0.x minor version change: candidate ccstats 0.9.0.
Verification: quota/weekly/cost unit tests, complete cargo test suite, fmt/check
and clippy. QuotaBar's adapter tests validate the actual downstream SDK contract.
