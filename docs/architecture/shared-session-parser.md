# Shared Claude/Codex session parsing

Use agent-sessions 0.2 for discovery, record framing, native parsing and token
state. ccstats retains model normalization, timezone/date projection, dedup IDs,
pricing, scope selection and RawEntry/ToolCall application models.

The explicit UsageStatistics policy preserves existing negative-Claude clamping,
cache-TTL capping and cumulative-reset behavior. Native missing fields remain
unknown in the library; ccstats applies its existing zero aggregation policy.
Native timestamp spelling is retained. Server tool calls are excluded from the
existing client-tool count. Current SDK title APIs retain native index sources.

IDE provenance maps into the existing interactive scope. Main/default all-source
reports otherwise retain their existing contracts. Codex first reads the legacy
ledger; only an error-free file with no usable legacy usage and observed response
records is retried in Response mode. The two ledgers are never added together.
This allows response-only logs without changing the established ledger for mixed
historical files. Cache partitions include the shared parser version.

Verification: existing CLI/SDK regressions remain intact; parser helper unit tests
move to equivalent native fixture tests rather than keeping dead parser code.
Compare sealed real-data reports and five-repeat cache-disabled performance with
baseline 2e2a766. Publish only after the dependency itself is available.

## Local evidence

All six sealed-data reports matched after ignoring elapsed pricing-cache age,
ordering equivalent session/tool ties, and floating-point roundoff. A direct
comparison of every Claude raw entry against 2e2a766 also matched. The original
binary has pre-existing price-alias cost variation; the final comparison used
matching observed tariffs and does not claim that issue is fixed.

Five interleaved old/new runs on the same 7.2 GB snapshot: median wall 5.01 /
4.73 s; median user CPU 6.45 / 6.68 s. Application cache disabled; OS page cache
retained. Desktop workloads were active, so these are paired local measurements.
