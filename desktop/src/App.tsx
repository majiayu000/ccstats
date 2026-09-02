import { useCallback, useEffect, useMemo, useState } from "react";
import {
  desktopBridge,
  type CostSummary,
  type SourceDescriptor,
  type TokenBreakdown,
  type UsageOverview,
  type UsageRange,
} from "./bridge";

const RANGE_OPTIONS: ReadonlyArray<{ value: UsageRange; label: string; eyebrow: string }> = [
  { value: "today", label: "Today", eyebrow: "Live window" },
  { value: "this_week", label: "This week", eyebrow: "Since Monday" },
  { value: "this_month", label: "This month", eyebrow: "Month to date" },
];

const integer = new Intl.NumberFormat("en-US");

function formatTokens(value: number) {
  return integer.format(value);
}

function formatCost(value: number | null, currency: string) {
  if (value === null) return "Unknown";
  return new Intl.NumberFormat("en-US", {
    style: "currency",
    currency,
    minimumFractionDigits: 2,
    maximumFractionDigits: 2,
  }).format(value);
}

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

function TokenComposition({ tokens }: { tokens: TokenBreakdown }) {
  const buckets = [
    { label: "Input", value: tokens.input_tokens, tone: "acid" },
    { label: "Output", value: tokens.output_tokens, tone: "paper" },
    { label: "Reasoning", value: tokens.reasoning_tokens, tone: "amber" },
    { label: "Cache write", value: tokens.cache_creation_tokens, tone: "blue" },
    { label: "Cache read", value: tokens.cache_read_tokens, tone: "mint" },
  ];
  const max = Math.max(...buckets.map((bucket) => bucket.value), 1);

  return (
    <section className="ledger-panel composition-panel" aria-labelledby="composition-title">
      <div className="section-heading">
        <div>
          <p className="section-kicker">Token taxonomy</p>
          <h2 id="composition-title">Composition</h2>
        </div>
        {tokens.cache_hit_rate !== null ? (
          <span className="cache-rate">{tokens.cache_hit_rate.toFixed(1)}% cache hit</span>
        ) : (
          <span className="muted-badge">Cache rate unavailable</span>
        )}
      </div>
      <div className="bucket-list">
        {buckets.map((bucket) => (
          <div className="bucket-row" key={bucket.label}>
            <div className="bucket-label">
              <span>{bucket.label}</span>
              <strong>{formatTokens(bucket.value)}</strong>
            </div>
            <div className="bucket-track" aria-hidden="true">
              <span
                className={`bucket-fill bucket-${bucket.tone}`}
                style={{ width: `${Math.max((bucket.value / max) * 100, bucket.value > 0 ? 1 : 0)}%` }}
              />
            </div>
          </div>
        ))}
      </div>
    </section>
  );
}

function QualityPanel({ summary }: { summary: CostSummary }) {
  const hasWarnings = summary.skipped_entries > 0 || summary.parse_error_entries > 0;

  return (
    <section
      className={`ledger-panel quality-panel ${hasWarnings ? "quality-warning" : "quality-clean"}`}
      aria-labelledby="quality-title"
      role="status"
    >
      <div className="quality-mark" aria-hidden="true">{hasWarnings ? "!" : "✓"}</div>
      <div>
        <p className="section-kicker">Data quality</p>
        <h2 id="quality-title">{hasWarnings ? "Review needed" : "Ledger healthy"}</h2>
        <p>
          {hasWarnings
            ? `${summary.skipped_entries} deduplicated · ${summary.parse_error_entries} malformed in combined scan`
            : `${summary.valid_entries} records reconciled with no parse warnings`}
        </p>
      </div>
      <dl className="quality-facts">
        <div>
          <dt>Valid</dt>
          <dd>{formatTokens(summary.valid_entries)}</dd>
        </div>
        <div>
          <dt>Deduped</dt>
          <dd>{formatTokens(summary.skipped_entries)}</dd>
        </div>
        <div>
          <dt>Scan malformed</dt>
          <dd>{formatTokens(summary.parse_error_entries)}</dd>
        </div>
      </dl>
    </section>
  );
}

function ModelTable({ summary }: { summary: CostSummary }) {
  return (
    <section className="ledger-panel model-panel" aria-labelledby="models-title">
      <div className="section-heading">
        <div>
          <p className="section-kicker">Attribution</p>
          <h2 id="models-title">Models</h2>
        </div>
        <span className="row-count">{summary.models.length} observed</span>
      </div>
      <div className="table-scroll">
        <table>
          <thead>
            <tr>
              <th>Model</th>
              <th>Input</th>
              <th>Output</th>
              <th>Cache</th>
              <th>Total</th>
              <th>Cost</th>
            </tr>
          </thead>
          <tbody>
            {summary.models.map((model) => (
              <tr key={model.model}>
                <td className="model-name">{model.model}</td>
                <td>{formatTokens(model.tokens.input_tokens)}</td>
                <td>{formatTokens(model.tokens.output_tokens)}</td>
                <td>{formatTokens(model.tokens.cache_read_tokens)}</td>
                <td>{formatTokens(model.tokens.total_tokens)}</td>
                <td className={model.cost === null ? "unknown-cost" : "known-cost"}>
                  {formatCost(model.cost, summary.currency)}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </section>
  );
}

function EmptyState({
  source,
  rangeLabel,
  parseErrors,
}: {
  source: string;
  rangeLabel: string;
  parseErrors: number;
}) {
  const hasUnattributedErrors = parseErrors > 0;

  return (
    <section className="empty-state">
      <span className="empty-index">00</span>
      <div>
        <p className="section-kicker">
          {hasUnattributedErrors ? "Unattributed parse warnings" : "No local records"}
        </p>
        <h2>
          {hasUnattributedErrors
            ? `No valid ${source} records in this window.`
            : `The ${source} ledger is quiet in this window.`}
        </h2>
        {hasUnattributedErrors ? (
          <p data-testid="unattributed-parse-errors">
            {formatTokens(parseErrors)} malformed records were rejected during the combined range
            scan. The summary does not expose their dates, so they cannot be assigned to {rangeLabel}.
          </p>
        ) : (
          <p>Run the source once or choose a wider time window. ccstats never invents missing usage.</p>
        )}
      </div>
    </section>
  );
}

export default function App() {
  const bridge = useMemo(() => desktopBridge(), []);
  const [sources, setSources] = useState<SourceDescriptor[]>([]);
  const [selectedSource, setSelectedSource] = useState("");
  const [selectedRange, setSelectedRange] = useState<UsageRange>("today");
  const [overview, setOverview] = useState<UsageOverview | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const loadOverview = useCallback(
    async (source: string) => {
      setLoading(true);
      setError(null);
      try {
        setOverview(await bridge.usageOverview(source));
      } catch (nextError) {
        setOverview(null);
        setError(errorMessage(nextError));
      } finally {
        setLoading(false);
      }
    },
    [bridge],
  );

  const initialize = useCallback(async () => {
    setLoading(true);
    setError(null);
    setOverview(null);
    try {
      const catalog = await bridge.listSources();
      if (catalog.length === 0) throw new Error("No usage sources are registered");
      const initial = catalog.find((source) => source.name === "claude") ?? catalog[0];
      setSources(catalog);
      setSelectedSource(initial.name);
      setOverview(await bridge.usageOverview(initial.name));
    } catch (nextError) {
      setError(errorMessage(nextError));
    } finally {
      setLoading(false);
    }
  }, [bridge]);

  useEffect(() => {
    void initialize();
  }, [initialize]);

  const summary = overview?.summaries.find((item) => item.range === selectedRange) ?? null;
  const sourceDescriptor = sources.find((source) => source.name === selectedSource) ?? null;
  const selectedRangeLabel = RANGE_OPTIONS.find((range) => range.value === selectedRange)?.label;
  const missingRangeError =
    overview && !summary ? `${selectedRangeLabel} is missing from the usage response.` : null;
  const costCoverage = summary?.api_equivalent_cost_coverage ?? null;
  const costIsLowerBound =
    summary !== null && summary.cost !== null && costCoverage?.cost_is_lower_bound === true;

  function changeSource(source: string) {
    setSelectedSource(source);
    void loadOverview(source);
  }

  function retry() {
    if (sources.length === 0) {
      void initialize();
    } else {
      void loadOverview(selectedSource);
    }
  }

  return (
    <div className="app-shell">
      <header className="topbar">
        <div className="brand-lockup">
          <span className="brand-glyph" aria-hidden="true">c.</span>
          <div>
            <strong>ccstats</strong>
            <span>local audit console</span>
          </div>
        </div>
        <div className="privacy-note"><i /> Usage files stay local · no transcript upload</div>
      </header>

      <main>
        <section className="hero">
          <div className="hero-copy">
            <p className="hero-index">LEDGER / OVERVIEW / 01</p>
            <h1>Usage overview</h1>
            <p>Inspect authoritative token records, model attribution, and accounting quality from one usage source.</p>
          </div>
          <div className="source-control">
            <label htmlFor="source-select">Usage source</label>
            <div className="select-frame">
              <select
                id="source-select"
                value={selectedSource}
                onChange={(event) => changeSource(event.target.value)}
                disabled={loading || sources.length === 0}
              >
                {sources.map((source) => (
                  <option key={source.name} value={source.name}>{source.display_name}</option>
                ))}
              </select>
              <span aria-hidden="true">↓</span>
            </div>
            <p>{sources.length} registered usage sources</p>
          </div>
        </section>

        <section className="command-strip" aria-label="Overview controls">
          <div className="range-switcher">
            {RANGE_OPTIONS.map((range) => (
              <button
                type="button"
                key={range.value}
                className={selectedRange === range.value ? "active" : ""}
                onClick={() => setSelectedRange(range.value)}
                aria-pressed={selectedRange === range.value}
              >
                <small>{range.eyebrow}</small>
                {range.label}
              </button>
            ))}
          </div>
          <button
            type="button"
            className="refresh-button"
            aria-label="Refresh ledger"
            onClick={() => void loadOverview(selectedSource)}
            disabled={loading || selectedSource.length === 0}
          >
            <span aria-hidden="true">↻</span> Refresh ledger
          </button>
        </section>

        {loading ? (
          <section className="loading-state" aria-live="polite">
            <div className="scanner" aria-hidden="true"><span /></div>
            <p>Auditing registered sources…</p>
            <small>Discovery → parse → deduplicate → price</small>
          </section>
        ) : error || missingRangeError ? (
          <section className="error-state" role="alert">
            <span className="error-code">ERR / LEDGER</span>
            <h2>Could not read this source.</h2>
            <p>{error ?? missingRangeError}</p>
            <button type="button" onClick={retry}>Try again</button>
          </section>
        ) : summary ? (
          <div data-testid="overview-content">
            <section className="report-meta">
              <div>
                <span className="live-dot" />
                <strong>{overview?.display_name}</strong>
                <span>{selectedRangeLabel}</span>
              </div>
              <p>
                Generated {overview ? new Date(overview.generated_at).toLocaleString() : "—"}
                <span>·</span>
                {overview?.elapsed_ms.toFixed(1)} ms scan
              </p>
            </section>

            {summary.valid_entries === 0 && summary.tokens.total_tokens === 0 ? (
              <EmptyState
                source={overview?.display_name ?? selectedSource}
                rangeLabel={selectedRangeLabel ?? selectedRange}
                parseErrors={summary.parse_error_entries}
              />
            ) : (
              <>
                <section className="metric-rack" aria-label="Usage totals">
                  <article className="metric-primary">
                    <p>Total tokens</p>
                    <strong data-testid="total-tokens">{formatTokens(summary.tokens.total_tokens)}</strong>
                    <span>Provider-reported components</span>
                  </article>
                  <article>
                    <p>{costIsLowerBound ? "Cost lower bound" : "Total cost"}</p>
                    <strong data-testid="total-cost" className={summary.cost === null ? "unknown-cost" : ""}>
                      {costIsLowerBound ? "≥ " : ""}{formatCost(summary.cost, summary.currency)}
                    </strong>
                    {costIsLowerBound && costCoverage ? (
                      <span data-testid="cost-coverage">
                        {formatTokens(costCoverage.priced_tokens)} / {formatTokens(costCoverage.total_tokens)} tokens priced ({costCoverage.percent.toFixed(1)}%)
                      </span>
                    ) : (
                      <span>{summary.cost === null ? "No trustworthy price" : summary.cost_kind}</span>
                    )}
                  </article>
                  <article>
                    <p>Records</p>
                    <strong>{formatTokens(summary.valid_entries)}</strong>
                    <span>Parsed source events</span>
                  </article>
                  <article>
                    <p>Capabilities</p>
                    <div className="capability-list">
                      {sourceDescriptor?.has_projects ? <span>Projects</span> : null}
                      {sourceDescriptor?.has_reasoning_tokens ? <span>Reasoning</span> : null}
                      {sourceDescriptor?.has_cache_read ? <span>Cache read</span> : null}
                      {sourceDescriptor?.has_cache_creation ? <span>Cache write</span> : null}
                    </div>
                    <span>Only source-backed fields</span>
                  </article>
                </section>

                <div className="analysis-grid">
                  <TokenComposition tokens={summary.tokens} />
                  <QualityPanel summary={summary} />
                </div>
                <ModelTable summary={summary} />
              </>
            )}
          </div>
        ) : null}
      </main>
      <footer>
        <span>ccstats desktop · audit surface</span>
        <span>Authoritative fields over inferred totals</span>
      </footer>
    </div>
  );
}
