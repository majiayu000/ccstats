# ccstats architecture

> **Source of truth for registered sources is** [`src/source/inventory.rs`](../src/source/inventory.rs) (`define_sources!`). That table expands `UsageSource`, canonical names, and boxed constructors. `registry.rs` only looks up those constructors by name/alias. The SDK re-exports the generated `UsageSource` enum — do not keep a second list in `sdk.rs`.
>
> The 2026-09-02 audit is a snapshot. This file wins for inventory, `--source all` token provenance, and Google pricing families after the follow-up PRs landed. Still-open product questions stay in GitHub issues, not here as invented behavior.
>
> - [Codebase audit (2026-09-02)](audits/2026-09-02-codebase-audit.md)
> - [Provenance, pricing families, and source inventory](architecture/2026-09-02-provenance-and-source-decoupling.md)

ccstats is a local-first CLI and Rust SDK for token and cost analytics from coding-agent logs. Each product implements `Source`, parses into one `RawEntry` shape, then shares loader, aggregation, pricing, and output.

Default CLI source is `claude` when neither a nested command, `--source`, nor `config.toml` `source` is set. Nested command and `--source` beat the config file. `--source all` is a registry sentinel (not an inventory row) that merges every registered source.

## Directory layout

```
src/
├── lib.rs                 # library crate, SDK exports, CLI dispatch
├── main.rs                # thin binary entry
├── cli/                   # clap: args, commands
├── config.rs              # optional TOML config
├── core/                  # RawEntry, CostKind, aggregator, dedup
├── source/
│   ├── inventory.rs       # define_sources! — registered source table
│   ├── registry.rs        # lookup by name/alias; ALL_SOURCES = "all"
│   ├── loader.rs          # discover → parse → filter → aggregate
│   ├── mod.rs             # Source, Capabilities, ParseOutput
│   └── <name>/            # per-source parsers (see table below)
├── pricing/               # LiteLLM ingest, families (Google included), cache
├── output/                # table / JSON / CSV / statusline
├── sdk.rs                 # public summarize_cost* + re-export UsageSource
└── utils/                 # timezone, date parsing, jq
```

Most sources are a single `src/source/<name>.rs` file. Claude, Codex, Cursor, Grok, and Kimi use a directory. `reasonix` is inlined in `source/mod.rs`. Do not treat a hand-written file list as the product surface — the inventory table is the list.

## Registered sources

Canonical `--source` names from `define_sources!` (29 sources). Aliases stay on `Source::aliases()` (examples: `cc`, `cx`, `cur`, `gx`, `km`, `gm`, `qw`).

| `--source` | Inventory variant | Typical data |
|------------|-------------------|--------------|
| `claude` | `Claude` | `~/.claude/projects` JSONL |
| `codex` | `Codex` | `~/.codex` sessions + archived sessions |
| `cursor` | `Cursor` | Cursor usage API / `CURSOR_USAGE_FILE` |
| `grok` | `Grok` | `~/.grok/logs/unified.jsonl` |
| `kimi` | `Kimi` | `~/.kimi-code/sessions` wire logs |
| `gemini` | `Gemini` | `~/.gemini/tmp` chats |
| `amp` | `Amp` | XDG Amp thread logs |
| `qwen` | `Qwen` | `~/.qwen/usage` ledger |
| `cline` | `Cline` | Cline CLI + VS Code task logs |
| `roocode` | `RooCode` | Roo Code VS Code task logs |
| `kilocode` | `KiloCode` | Kilo Code VS Code task logs |
| `opencode` | `OpenCode` | local OpenCode SQLite |
| `mimocode` | `MiMoCode` | local MiMo Code SQLite |
| `kilo` | `Kilo` | local Kilo CLI SQLite |
| `pi` | `Pi` | Pi JSONL sessions |
| `senpi` | `Senpi` | Senpi JSONL sessions |
| `kimchi` | `Kimchi` | Kimchi harness JSONL |
| `gjc` | `Gjc` | Gajae Code v5 JSONL |
| `prime` | `Prime` | Prime Agent JSONL |
| `omp` | `Omp` | Oh My Pi JSONL |
| `copilot` | `Copilot` | Copilot CLI OTel JSONL |
| `goose` | `Goose` | Goose SQLite usage ledger |
| `openclaw` | `OpenClaw` | OpenClaw v3 transcripts |
| `xum` | `Xum` | Xum per-workspace snapshots |
| `hermes` | `Hermes` | Hermes Agent usage ledger |
| `reasonix` | `Reasonix` | Reasonix provider ledger |
| `fx` | `Fx` | Vercel Fx generation ledger |
| `unsloth` | `Unsloth` | Unsloth Studio receipts |
| `dsh` | `Dsh` | DeepSeek Harness sessions |

`ccstats sources` / `ccstats sources --json` lists the same names plus the `all` sentinel.

## `Source` trait

Implemented by each product. Methods with default bodies are optional.

```rust
pub trait Source: Send + Sync {
    fn name(&self) -> &'static str;
    fn display_name(&self) -> &'static str { self.name() }
    fn aliases(&self) -> &'static [&'static str] { &[] }
    fn capabilities(&self) -> Capabilities;

    fn setup_hint(&self) -> &'static str { /* doctor copy */ }
    fn diagnose(&self) -> SourceDiagnostic { /* default: find_files().len() */ }

    fn find_files(&self) -> Vec<PathBuf>;
    fn find_files_for_filter(
        &self,
        filter: &DateFilter,
        timezone: Timezone,
    ) -> Vec<PathBuf> {
        self.find_files()
    }
    fn parse_file(&self, path: &Path, timezone: Timezone, debug: bool) -> ParseOutput;
    fn finalize_entries(&self, entries: Vec<RawEntry>) -> Vec<RawEntry> { entries }

    fn find_tool_call_files(&self) -> Vec<PathBuf> { Vec::new() }
    fn parse_tool_call_file(&self, path: &Path, timezone: Timezone) -> Vec<ToolCall> {
        Vec::new()
    }
}
```

Required for a new source: `name`, `capabilities`, `find_files`, `parse_file`. Put CLI aliases on `aliases()`. `finalize_entries` runs only on the **dedup path** (after `DedupAccumulator`), not on incremental daily/session aggregation. Grok uses it to **reclassify** overlapping `EstimatedProxy` snapshot rows to `CostKind::Real` with `recorded_cost_usd = Some(0.0)` when the same session already has API-equivalent priced rows — it does not drop those rows. Snapshot-only sessions stay `EstimatedProxy`. Tool-call hooks are Claude-only today. Cursor overrides `find_files_for_filter` so API requests can follow the date range.

## `Capabilities`

All eight flags that exist today:

```rust
pub struct Capabilities {
    pub has_projects: bool,
    pub has_billing_blocks: bool,
    pub has_reasoning_tokens: bool,
    pub has_cache_creation: bool,
    pub has_cache_read: bool,       // trustworthy prompt-cache reads
    pub needs_dedup: bool,
    pub has_tool_calls: bool,
    pub has_endpoints: bool,        // native vs proxy (Claude)
}
```

`Capabilities::combine` ORs most flags across `--source all`. `has_cache_read` is AND (a mixed-source cache hit rate is hidden unless every selected source reports trustworthy cache reads).

## `RawEntry`

Every parser maps native logs into this shape.

```rust
pub struct RawEntry {
    pub timestamp: String,           // UTC
    pub timestamp_ms: i64,
    pub date_str: String,            // local YYYY-MM-DD
    pub message_id: Option<String>,  // dedup key
    pub session_key: String,         // stable internal session identity
    pub session_id: String,
    pub project_path: String,
    pub model: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_creation: i64,
    pub cache_creation_1h: i64,      // subset of cache_creation, not additive
    pub cache_read: i64,
    pub reasoning_tokens: i64,
    pub stop_reason: Option<String>,
    pub cost_kind: CostKind,         // Real | EstimatedProxy | Mixed
}
```

Five **non-overlapping** token buckets:

```
total_tokens = input + output + reasoning + cache_creation + cache_read
```

`cache_creation_1h` is the portion of `cache_creation` written with a 1-hour TTL (higher list price). It is **not** a sixth additive bucket. Entries also carry `endpoint`, `call_count`, and optional recorded/API-equivalent cost fields used by aggregation — see `src/core/types.rs`.

## Data flow

```
CLI (main.rs / lib.rs)  or  SDK (summarize_cost*)
        │
        ▼
inventory.rs  →  registry lookup (name / alias / "all")
        │
        ▼
DataLoader
  find_files[_for_filter] → parse_file (rayon) → date filter
  then either:
    needs_dedup: DedupAccumulator → finalize_entries → aggregate
    else: aggregate incrementally (no finalize_entries)
        │
        ▼
RawEntry  (five buckets + CostKind + session_key)
        │
        ▼
aggregate  daily / session / project / blocks / endpoints
        │
        ▼
PricingDb  families: Anthropic | OpenAI | Xai | Google | DeepSeek | Qwen | Glm | Moonshot
        │
        ▼
table / JSON / CSV / statusline    or    CostSummary (SDK skips the output layer)
```

- **`--source all`:** default token columns are `CostKind::Real` only (`apply_real_token_totals_for_all_source`). Estimated-proxy tokens (for example Grok context snapshots) stay out of those totals; estimated proxy cost can still be reported separately. Do not apply that transform to the Grok-only path.
- **Grok-only:** daily/weekly/monthly/today use `load_grok_daily_with_cost` and `GrokCostReport`. Do not bypass that path.
- SDK `summarize_cost` / `summarize_cost_with_cli_config` reuse inventory, loader, aggregation, and pricing.

## Add a source

This is the checklist that compiles. Nested clap is optional.

1. Parser module under `src/source/<name>/` (for example `src/source/newcli/mod.rs`).
2. `mod newcli;` in `src/source/mod.rs`.
3. **One row** in `define_sources!` in `src/source/inventory.rs`.
4. Aliases on `Source::aliases()`.
5. `--source newcli` is enough. Nested `ccstats newcli` in `cli/commands.rs` is optional sugar (only Codex, Grok, and Kimi have nested commands today).

Do **not** edit a `registry.rs` `vec!` of constructors. Do **not** add a parallel `UsageSource` variant in `sdk.rs`. Both are generated from the inventory row.

```rust
// src/source/newcli/mod.rs
use std::path::{Path, PathBuf};

use crate::source::{Capabilities, ParseOutput, Source};
use crate::utils::Timezone;

pub(crate) struct NewcliSource;

impl NewcliSource {
    pub(crate) const fn new() -> Self {
        Self
    }
}

impl Source for NewcliSource {
    fn name(&self) -> &'static str {
        "newcli"
    }

    fn display_name(&self) -> &'static str {
        "NewCLI"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["nc"]
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            has_projects: false,
            has_billing_blocks: false,
            has_reasoning_tokens: false,
            has_cache_creation: false,
            has_cache_read: false,
            needs_dedup: false,
            has_tool_calls: false,
            has_endpoints: false,
        }
    }

    fn find_files(&self) -> Vec<PathBuf> {
        Vec::new()
    }

    fn parse_file(&self, _path: &Path, _timezone: Timezone, _debug: bool) -> ParseOutput {
        ParseOutput::default()
    }
}
```

```rust
// src/source/mod.rs — next to the other `mod` lines
mod newcli;
```

```rust
// src/source/inventory.rs — inside define_sources! { ... }
/// NewCLI local usage.
Newcli => "newcli", super::newcli::NewcliSource::new(),
```

Map native records into `RawEntry` (five buckets, `cache_creation_1h` ⊆ `cache_creation`, `cost_kind`, `session_key`). Explicit root env vars that are set but not a directory must not fall back to the default home.

## CLI config

CLI reads optional TOML, then merges command-line flags. Flags win.

Search order (first file that exists is authoritative):

1. `~/.config/ccstats/config.toml`
2. Platform config dir, for example macOS `~/Library/Application Support/ccstats/config.toml`
3. `~/.ccstats.toml`

If that file cannot be read, or TOML/types are invalid, CLI errors. Quiet/statusline paths do not fall back to defaults. Only when **no** file exists is `Config::default()` used.

Config keys do not set source roots:

| Key | Type | Values |
|-----|------|--------|
| `offline`, `compact`, `no_cost`, `no_color`, `breakdown`, `debug`, `strict_pricing` | boolean | `true` / `false` |
| `order` | string | `asc` / `desc` |
| `color` | string | `auto` / `always` / `never` |
| `cost` | string | `show` / `hide` |
| `timezone`, `locale`, `currency`, `source` | string | same strings as the CLI flags |

```toml
source = "codex"
timezone = "Asia/Shanghai"
currency = "USD"
offline = true
strict_pricing = true
compact = true
order = "desc"
color = "auto"
cost = "show"
```

## Env overrides

Source roots are env vars, not config keys. If an override **is set** but is not a usable directory, discovery must return no files — it must **not** fall back to the default home. Grok already does this (`GROK_HOME`).

| Source | Env var | Value | Default when unset |
|--------|---------|-------|--------------------|
| Claude Code | `CLAUDE_CONFIG_DIR` | Config root containing `projects/` | `~/.claude` |
| OpenAI Codex | `CODEX_HOME` | Root containing `sessions/` and `archived_sessions/` | `~/.codex` |
| Cursor | `CURSOR_API_KEY` / `CURSOR_SESSION_TOKEN` | Admin API key or dashboard session cookie | Optional `CURSOR_USAGE_FILE` replay |
| Grok | `GROK_HOME` | Root containing `logs/unified.jsonl` and `sessions/` | `~/.grok` |
| Kimi Code | `KIMI_CODE_HOME` | Root containing `sessions/` | `~/.kimi-code` |

Other registered sources have their own env vars (see README / `ccstats doctor`).

## Claude Code parse sketch

Input: `$CLAUDE_CONFIG_DIR/projects/**/*.jsonl` (default `~/.claude/projects`).

```
{
  "timestamp": "2026-02-05T10:30:00Z",
  "message": {
    "id": "msg_xxx",
    "model": "claude-3-opus-20240229",
    "stop_reason": "end_turn",
    "usage": {
      "input_tokens": 1000,
      "output_tokens": 500,
      "cache_creation_input_tokens": 80,
      "cache_read_input_tokens": 200,
      "cache_creation": { "ephemeral_1h_input_tokens": 20 }
    }
  }
}
```

1. Glob JSONL under `projects/`.
2. Parse files in parallel (`DataLoader` + rayon).
3. `session_id` = file stem; `session_key` = file path; `project_path` = directory under `projects/`.
4. Strip `anthropic.` / `claude-` prefixes and `-YYYYMMDD` suffixes from the model name.
5. Five buckets from `usage`; `cache_creation_1h` from `ephemeral_1h_input_tokens`, clamped to `cache_creation`.
6. Dedup by `message_id` (source-wide prefix); keep the completed (`stop_reason`) entry.
7. Aggregate daily / session / project / clock-aligned blocks.

## Codex CLI parse sketch

Input: `$CODEX_HOME/sessions/**/*.jsonl` and `$CODEX_HOME/archived_sessions/**/*.jsonl` (default `~/.codex`).

```
{
  "timestamp": "2026-02-05T10:30:00Z",
  "type": "event_msg",
  "payload": {
    "type": "token_count",
    "info": {
      "total_token_usage": {
        "input_tokens": 5000,
        "cached_input_tokens": 1000,
        "output_tokens": 2000,
        "reasoning_output_tokens": 500,
        "total_tokens": 7000
      },
      "last_token_usage": { ... },
      "model": "gpt-5.2"
    }
  }
}
```

1. Glob active and archived session JSONL.
2. `turn_context` (and similar) events update the current model.
3. `event_msg` + `token_count` events carry **cumulative** totals.
4. Delta: use `last_token_usage` when present; otherwise `total - previous_total`.
5. Skip when the full cumulative vector is unchanged, or the delta is empty.
6. Split buckets: `input = input_tokens - cached_input_tokens`; `output = output_tokens - reasoning_output_tokens`; `cache_read = cached_input_tokens`; `reasoning = reasoning_output_tokens`. Codex `input_tokens` includes cache; OpenAI `output_tokens` includes reasoning.
7. `needs_dedup` is true; message ids are synthesized from totals/deltas.

## Pricing cache (24h TTL)

Preferred file: platform cache dir / `ccstats/pricing.json` (for example `~/Library/Caches/ccstats/pricing.json` on macOS). Legacy read path: `~/.cache/ccstats/pricing.json`.

1. If a cache file is younger than 24 hours, use it.
2. After TTL, fetch LiteLLM, parse allowlisted families (including Google/Gemini), and rewrite the cache.
3. Fetch failure falls back to the existing cache (even if stale).
4. No cache: built-in per-family fallback prices. Unknown models stay N/A under `--strict-pricing` (never a silent Sonnet guess).

## Performance

- Parallel file parse (rayon)
- Filter after parse (local date + timezone, optional timestamp bounds)
- Lazy pricing load and 24h cache
- Streaming JSONL reads

## CLI period grain vs SDK current period

These share English words but are different concepts. Decided in issue [#136](https://github.com/majiayu000/ccstats/issues/136): keep current behavior; no `--current` flag.

- **CLI `weekly` / `monthly`** are aggregation grains. They group already-filtered history by ISO week (Monday start) or calendar month. Only `today` / `statusline` apply a today date filter. Bound dates with `--since` / `--until`.
- **SDK `UsageRange::ThisWeek` / `ThisMonth`** are current-period date ranges in the selected timezone: Monday–today and month-start–today.

## Parked (do not invent a fix)

- **`blocks`** are clock-aligned 5-hour windows (`hour() / 5 * 5` → local 00:00 / 05:00 / 10:00 / 15:00 / 20:00), not Anthropic’s rolling billing window. See issue [#135](https://github.com/majiayu000/ccstats/issues/135).
