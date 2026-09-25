# Session details JSON, schema 1

`ccstats session --json --details --source claude|codex` is an opt-in machine export for consumers such as Looper. Regular session JSON retains its existing array shape. `--details` requires JSON, the session command, and a single Claude/Codex source. It exports the first user prompt and therefore should only be requested by consumers that need that local content.

```sh
ccstats session --json --details --source codex --since 2026-09-24 --until 2026-09-24 --timezone local --offline --strict-pricing
```

The schema has this structure:

```json
{
  "schema_version": 1,
  "source": "codex",
  "currency": "USD",
  "cost_kind": "api_equivalent_estimate",
  "parse_errors": 0,
  "unattributed_files": 0,
  "dedup_skipped_entries": 0,
  "sessions": [{
    "session_id": "full-native-id",
    "project_path": "/work",
    "first_timestamp": "2026-09-24T10:00:00Z",
    "last_timestamp": "2026-09-24T10:00:00Z",
    "first_user_prompt": "/x-post",
    "is_subagent": false,
    "requests": 1,
    "breakdown": [{
      "model": "gpt-5.4",
      "input_tokens": 80,
      "output_tokens": 20,
      "reasoning_tokens": 10,
      "cache_creation_tokens": 0,
      "cache_creation_1h_tokens": 0,
      "cache_read_tokens": 20,
      "requests": 1,
      "cost": 0.000655,
      "pricing_source": "fallback"
    }]
  }]
}
```

All numeric costs in this schema are USD API-equivalent estimates, independent of display currency or cost visibility preferences. `--strict-pricing` leaves an unresolved model's `cost` null and `pricing_source` `unknown`; it does not reject the export, allowing the consumer to apply a configured rate first. Consumers must not interpret null as zero. Without `--strict-pricing`, existing ccstats fallback policy applies and remains labeled by `pricing_source`.

In details mode, missing Codex model metadata is labeled `unknown-model` instead of assuming GPT-5; its cost stays null unless a consumer supplies an explicit override.

Usage comes from the same source reader, deduplication, date filter, model normalization, and pricing calculation as ordinary ccstats reports. Counters are exclusive: input excludes cache; output excludes reasoning. `cache_creation_1h_tokens` is a subset of cache creation, not an additional bucket. Costs retain per-request long-context and cache TTL accounting. `requests` counts deduplicated usage calls represented in the selected range.

`first_timestamp` and `last_timestamp` bound selected usage, not the full lifetime of the transcript. Day selection uses usage timestamps in the requested timezone. The prompt comes from the first nonempty text block of the first user message with text, across the whole file, so attribution survives midnight. It is null when absent. Exact native `cwd` is preferred; the ordinary source project path is used when absent. `is_subagent` uses a `subagents` path component or native Codex subagent origin. Claude sidechain messages inside a main transcript do not independently reclassify its file.

Metadata is projected through agent-sessions; ccstats does not add another native JSON parser. Scanning stops once a prompt and cwd are known. Metadata is not stored in the usage cache. Sessions remain file-scoped even when native IDs collide. Consumers should scope IDs with source/workdir when combining exports.

`--details-workdir PATH` is repeatable and limits files before usage parsing. `--details-exclude-subagents` excludes child sessions before their errors are collected. Both require `--details`. Claude directory slugs preserve selection of older logs without cwd; native cwd can also match. Codex selection uses the shared metadata reader. Unrelated project files and excluded subagents cannot contribute parsing failures to a scoped report. Damage in selected files still contributes errors. The cache partition includes canonical sorted/deduplicated workdirs and the exclusion policy.

Codex discovery covers both live `sessions` and `archived_sessions`; both obey identical workdir and usage-date filters.

When a scoped Codex file has no recoverable cwd, it cannot be assigned to the selected workdirs. It is excluded and counted in `unattributed_files`, across discovered files (not asserted to be inside the requested day). Consumers must display that incomplete-coverage warning rather than treating it as a proven zero or an in-scope error. This preserves the old workdir selection without hiding the uncertainty.

Empty ranges return `sessions: []` in the same envelope. `parse_errors` includes usage parse, discovery, and metadata projection errors. It is a diagnostic count, not a count of unique physical bad lines (one line can affect more than one projection). Consumers requiring complete accounting must reject a nonzero count. Claude usage without a timestamp is counted as incomplete accounting in details mode; normal reports retain their existing skip policy. Codex nonzero `last_token_usage` without a cumulative `total_token_usage` is counted as incomplete accounting in details mode. No speculative last-only ledger is added; ordinary ccstats reports keep their existing ignore behavior. The diagnostics mode has a separate usage-cache partition. Invalid root configuration fails the command. Unknown new record types remain subject to the shared reader's format-tracking policy.

Tests: `cargo test --test cli_session_details` covers default output isolation, first prompt outside the date range, native cwd with hyphens, deduplication, mixed known/unknown prices, Claude subagent tagging, Codex cumulative deltas/cache/reasoning, empty ranges, malformed input, and invalid invocation.
