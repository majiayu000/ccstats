# Authoritative Token Accounting (Claude Code JSONL)

This document defines the **most accurate local** token accounting method for Claude Code JSONL logs. It reflects how Claude Code **records usage** and how streaming produces duplicate rows. The algorithm is now the **default behavior in `ccstats`**.

> Scope: This is the best possible approximation **using local JSONL only**. Absolute truth still belongs to server-side billing/export logs.

---

## Source of Truth

### Where tokens originate
- Tokens are **not computed locally**. They are provided by the API response in `message.usage`.
- During streaming, partial usage fields are merged into the in-memory message, then written to JSONL.

### How logs are written
- JSONL is **append-only** and **sync-written** (`appendFileSync`).
- Each entry is written immediately; if the process exits mid-stream, partial entries remain.

**Code references (open-claude-code):**
- Usage fields updated during streaming: `src_v2.0.76/cli.readable.js` (message_delta handling)
- JSONL append logic: `src_v2.0.76/cli.readable.js` (`appendEntry`, `appendFileSync`)

---

## JSONL File Layout (Observed)

Claude Code stores logs under:
```
~/.claude/projects/<project>/*.jsonl
~/.claude/projects/<project>/subagents/*.jsonl
```

Each line is a JSON object. We only care about entries where:
- `message.usage` exists
- `message.model` is not `<synthetic>`

Common entry types in a session file:
- `type: "user"` (no usage)
- `type: "assistant"` (has `message.usage`)
- `type: "summary"`, `file-history-snapshot`, `tag`, etc. (no usage)

---

## Observed Failure Modes

1. **Streaming duplicates within a single file**
   - Same `message.id` appears multiple times (each stream chunk may re-emit usage).
2. **Cross-file duplicates (subagents)**
   - The same `message.id` can appear in both the main session file and a subagent file.
3. **Missing `requestId`**
   - Some entries lack `requestId`; any logic that requires it will overcount.
4. **Missing `stop_reason`**
   - If the process exits mid-stream, the final completed entry might never be written.
   - The last partial entry still contains usage tokens (may be incomplete).

---

## Goal

Produce the **closest possible approximation of billed tokens** using only local JSONL logs, while avoiding known overcounting traps.

---

## Authoritative Algorithm (Default in `ccstats`)

### Step 1) Input selection
Include only entries that satisfy all of the following:
- `message.usage` exists
- `message.model` is present and **not** `<synthetic>`
- `timestamp` is valid ISO8601

### Step 2) Model normalization
Normalize to match user-facing model names:
- Strip `claude-` prefix
- Strip trailing `-YYYYMMDD` suffix

### Step 3) Global deduplication key
Use **only `message.id`** as the primary key.
- Do **not** require `requestId`
- Deduplicate **globally across all files** (main sessions + subagents)

### Step 4) Final entry selection (per `message.id`)
For each `message.id`, choose **exactly one** entry:
1. If any entry has `stop_reason != null`, select the **earliest** such entry by timestamp.
2. Otherwise select the **latest** entry by timestamp.

Rationale:
- `stop_reason` indicates completion and is the most reliable signal.
- If no completion exists, the last entry is the best available approximation.

### Step 5) Entries without `message.id`
If `message.id` is missing, count **only if** `stop_reason != null`.

### Step 6) Summation
For each selected entry, sum:
```
input_tokens
output_tokens
cache_creation_input_tokens
cache_read_input_tokens
```
Total tokens = sum of the above (per entry) aggregated by day/week/month/etc.

---

## Timezone & Date Bucketing

- `ccstats` uses **local timezone** when converting timestamps to dates.
- This matches the CLI’s daily usage output.
- Use `--timezone UTC` if you want UTC bucketing for analysis consistency.

---

## Accuracy Notes

- This algorithm is **more accurate than per-file dedupe** because it removes duplicates across subagent files.
- It is **more accurate than requestId-based dedupe** because requestId is often missing.
- It is **more accurate than naive summation** because it collapses streaming chunks to a single final record.

### Known limits (cannot be solved locally)
- If usage was billed but **never written to disk** (crash before append), no local algorithm can recover it.
- If usage was written but **stop_reason missing**, we approximate by choosing the last entry.

---

## Optional Strict Mode (Most Conservative)

If you want to **avoid counting incomplete generations**:
- Require `stop_reason != null` for **all** entries (including those with `message.id`).
- This can undercount when usage was billed but final completion never logged.

---

## Implementation Checklist

- [ ] Scan all `~/.claude/projects/**/*.jsonl` files
- [ ] Parse JSON lines safely (skip invalid JSON)
- [ ] Apply input filters (usage/model/timestamp)
- [ ] Normalize model name
- [ ] Global dedupe by `message.id` across files
- [ ] Select final entry per message
- [ ] Aggregate tokens and counts

---

## Reference Implementation (ccstats)

The default `ccstats` loader now implements this algorithm.

Key implementation changes:
- Collect all entries across files before deduplication
- Global dedupe on `message.id`
- Prefer entries with `stop_reason` by timestamp
- Local timezone bucketing

