import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  aggregateUsageOverviews,
  hasExactCost,
  type CostSummary,
  type DesktopBridge,
  type SourceDescriptor,
  type UsageOverview,
} from "./bridge";
import { errorMessage, formatCost, formatTokens } from "./format";

export function useLiveUsage(
  bridge: DesktopBridge,
  sources: SourceDescriptor[],
  selectedSource: string,
  active: boolean,
) {
  const [overview, setOverview] = useState<UsageOverview | null>(null);
  const [baseline, setBaseline] = useState<CostSummary | null>(null);
  const [startedAt, setStartedAt] = useState<string | null>(null);
  const [updatedAt, setUpdatedAt] = useState<string | null>(null);
  const [monitoring, setMonitoring] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const requestId = useRef(0);
  const refreshingRef = useRef(false);

  const refresh = useCallback(async (resetBaseline = false) => {
    if (!selectedSource || (!resetBaseline && refreshingRef.current)) return;
    const request = ++requestId.current;
    refreshingRef.current = true;
    setRefreshing(true);
    setError(null);
    if (resetBaseline) {
      setOverview(null);
      setBaseline(null);
      setStartedAt(null);
      setUpdatedAt(null);
    }
    try {
      const findings = selectedSource === "all" ? await bridge.sourceDiagnostics() : [];
      const ready = new Set(findings.filter((row) => row.status !== "missing").map((row) => row.name));
      const names = sources.filter((source) => source.name !== "all" && ready.has(source.name)).map((source) => source.name);
      if (selectedSource === "all" && names.length === 0) throw new Error("No detected or configured sources are ready to aggregate.");
      const next = selectedSource === "all" ? aggregateUsageOverviews(await bridge.usageOverviews(names)) : await bridge.usageOverview(selectedSource);
      const today = next.summaries.find((summary) => summary.range === "today");
      if (!today) throw new Error("Today is missing from the usage response.");
      if (request !== requestId.current) return;
      const now = new Date().toISOString();
      setOverview(next);
      setUpdatedAt(now);
      if (resetBaseline) {
        setBaseline(today);
        setStartedAt(now);
      }
    } catch (nextError) {
      if (request === requestId.current) setError(errorMessage(nextError));
    } finally {
      if (request === requestId.current) {
        refreshingRef.current = false;
        setRefreshing(false);
      }
    }
  }, [bridge, selectedSource, sources]);

  useEffect(() => {
    if (!active || !selectedSource) return;
    setMonitoring(true);
    void refresh(true);
    return () => {
      requestId.current += 1;
      refreshingRef.current = false;
      setRefreshing(false);
    };
  }, [active, refresh, selectedSource]);

  useEffect(() => {
    if (!active || !monitoring) return;
    const timer = window.setInterval(() => void refresh(false), 15_000);
    return () => window.clearInterval(timer);
  }, [active, monitoring, refresh]);

  return {
    summary: useMemo(() => overview?.summaries.find((summary) => summary.range === "today") ?? null, [overview]),
    baseline,
    startedAt,
    updatedAt,
    monitoring,
    refreshing,
    error,
    refresh,
    toggleMonitoring: () => setMonitoring((current) => !current),
  };
}

export type LiveUsage = ReturnType<typeof useLiveUsage>;

export function LiveView({ live }: { live: LiveUsage }) {
  if (!live.summary) {
    return live.error
      ? <section className="state-pane error-state" role="alert"><span aria-hidden="true">!</span><div><h2>Live monitoring could not start.</h2><p>{live.error}</p><button type="button" onClick={() => void live.refresh(true)}>Try again</button></div></section>
      : <section className="loading-state" aria-live="polite"><div><i /><i /><i /></div><strong>Establishing live baseline…</strong><span>Scanning today’s local ledger</span></section>;
  }

  const tokenDelta = live.summary.tokens.total_tokens - (live.baseline?.tokens.total_tokens ?? live.summary.tokens.total_tokens);
  const exactCost = hasExactCost(live.summary);
  const costDelta = live.baseline && exactCost && hasExactCost(live.baseline)
    ? live.summary.cost! - live.baseline.cost!
    : null;
  const costPrefix = live.summary.api_equivalent_cost_coverage?.cost_is_lower_bound ? "≥ " : exactCost ? "" : "≈ ";

  return (
    <section className="live-workbench" aria-labelledby="live-title">
      <header className="work-pane pane-heading live-heading"><div><h2 id="live-title">Live usage</h2><p>Today’s local ledger, rescanned every 15 seconds while monitoring.</p></div><div className={`live-state ${live.monitoring ? "active" : "paused"}`}><i aria-hidden="true" /><strong>{live.monitoring ? "Monitoring" : "Paused"}</strong></div></header>
      {live.error ? <p className="inline-warning" role="alert">Refresh failed; showing the last trusted snapshot. {live.error}</p> : null}
      {live.summary.parse_error_entries > 0 ? <p className="inline-warning" role="status">{formatTokens(live.summary.parse_error_entries)} malformed records were rejected in the combined scan; their dates cannot be assigned to Today.</p> : null}
      <div className="live-reading-grid"><article className="work-pane live-primary"><span>Tokens today</span><strong data-testid="live-total-tokens">{formatTokens(live.summary.tokens.total_tokens)}</strong><small>{live.summary.valid_entries} parsed records</small></article><article className="work-pane live-metric"><span>Since monitoring started</span><strong data-testid="live-token-delta">{`${tokenDelta >= 0 ? "+" : "−"}${formatTokens(Math.abs(tokenDelta))}`}</strong><small>Token change</small></article><article className="work-pane live-metric"><span>Cost today</span><strong>{costPrefix}{formatCost(live.summary.cost, live.summary.currency)}</strong><small data-testid="live-cost-delta">{costDelta === null ? `Cost delta unavailable · ${live.summary.pricing_source}` : `${costDelta >= 0 ? "+" : "−"}${formatCost(Math.abs(costDelta), live.summary.currency)}`}</small></article></div>
      <aside className="work-pane live-control"><dl><div><dt>Started</dt><dd>{live.startedAt ? new Date(live.startedAt).toLocaleTimeString() : "—"}</dd></div><div><dt>Last refresh</dt><dd>{live.updatedAt ? new Date(live.updatedAt).toLocaleTimeString() : "—"}</dd></div><div><dt>Scan duration</dt><dd>{live.summary.elapsed_ms.toFixed(1)} ms</dd></div></dl><button type="button" aria-label={live.monitoring ? "Pause monitoring" : "Resume monitoring"} onClick={live.toggleMonitoring}>{live.monitoring ? "Pause" : "Resume"}</button></aside>
    </section>
  );
}
