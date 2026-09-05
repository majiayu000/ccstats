import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  aggregateUsageOverviews,
  detectUsageAnomaly,
  desktopBridge,
  hasExactCost,
  type AnalyticsQuality,
  rankConsumers,
  type CodexQuotaOverview,
  type CostSummary,
  type ProjectDrilldownSummary,
  type RankedConsumer,
  type SourceDescriptor,
  type SourceDiagnosticDescriptor,
  type TokenBreakdown,
  type UsageHistory,
  type UsageOverview,
  type UsageRange,
} from "./bridge";
import { errorMessage, formatCost, formatTokens } from "./format";
import { ActivityView } from "./ActivityView";
import { CostTrustView } from "./CostTrustView";
import { LiveView, useLiveUsage } from "./LiveView";
import { MachinesView } from "./MachinesView";

type AnalyticsView = "overview" | "trust" | "live" | "top" | "limits" | "machines" | "activity" | "projects" | "history" | "diagnostics";
const VIEW_GROUPS: ReadonlyArray<{ label: string; views: ReadonlyArray<{ value: AnalyticsView; label: string; detail: string }> }> = [
  { label: "Observe", views: [
    { value: "overview", label: "Overview", detail: "Source totals" },
    { value: "live", label: "Live", detail: "15-second watch" },
    { value: "top", label: "Top consumers", detail: "Rankings and spikes" },
  ] },
  { label: "Explain", views: [
    { value: "activity", label: "Turns & tools", detail: "Evidence trace" },
    { value: "projects", label: "Projects", detail: "Session trace" },
    { value: "history", label: "History", detail: "Daily movement" },
  ] },
  { label: "Trust", views: [
    { value: "trust", label: "Cost evidence", detail: "Pricing provenance" },
    { value: "limits", label: "Limits", detail: "Quota and budget" },
    { value: "diagnostics", label: "Diagnostics", detail: "Source readiness" },
  ] },
  { label: "Devices", views: [
    { value: "machines", label: "Machines", detail: "Cross-device rollup" },
  ] },
];

const RANGE_OPTIONS: ReadonlyArray<{ value: UsageRange; label: string }> = [
  { value: "today", label: "Today" }, { value: "this_week", label: "This week" }, { value: "this_month", label: "This month" },
];

function QualityStatus({ summary }: { summary: CostSummary }) {
  const hasWarnings = summary.skipped_entries > 0 || summary.parse_error_entries > 0;

  return (
    <aside className={`quality-status ${hasWarnings ? "quality-warning" : ""}`} role="status">
      <span className="quality-symbol" aria-hidden="true">{hasWarnings ? "!" : "✓"}</span>
      <div>
        <strong>{hasWarnings ? "Review needed" : "Ledger healthy"}</strong>
        <p>
          {hasWarnings
            ? `${summary.skipped_entries} deduplicated · ${summary.parse_error_entries} malformed in combined scan`
            : `${summary.valid_entries} records reconciled with no parse warnings`}
        </p>
      </div>
      <dl>
        <div><dt>Valid</dt><dd>{formatTokens(summary.valid_entries)}</dd></div>
        <div><dt>Deduped</dt><dd>{formatTokens(summary.skipped_entries)}</dd></div>
        <div><dt>Scan malformed</dt><dd>{formatTokens(summary.parse_error_entries)}</dd></div>
      </dl>
    </aside>
  );
}

function AnalysisQualityNotice({ quality, combinedScan = false }: { quality: AnalyticsQuality; combinedScan?: boolean }) {
  if (quality.parse_error_entries === 0 && quality.dedup_skipped_entries === 0) return null;
  const noValidRecords = !combinedScan && quality.valid_entries === 0 && quality.parse_error_entries > 0;
  return (
    <aside className={`quality-notice ${noValidRecords ? "quality-notice-error" : ""}`} role={noValidRecords ? "alert" : "status"}>
      <strong>{noValidRecords ? "No valid records could be parsed." : "Data quality needs review."}</strong>
      <span>
        {formatTokens(quality.parse_error_entries)} malformed · {formatTokens(quality.dedup_skipped_entries)} deduplicated
        {combinedScan ? " in the combined scan; malformed dates are not attributable to this selected period." : " in this period."}
      </span>
    </aside>
  );
}

function costIsLowerBound(summary: CostSummary) {
  return summary.cost !== null && summary.api_equivalent_cost_coverage?.cost_is_lower_bound === true;
}

function costSummaryQuality(summary: CostSummary): AnalyticsQuality { return { valid_entries: summary.valid_entries, dedup_skipped_entries: summary.skipped_entries, parse_error_entries: summary.parse_error_entries }; }

function displayedCost(summary: CostSummary) {
  return `${costIsLowerBound(summary) ? "≥ " : summary.cost !== null && !hasExactCost(summary) ? "≈ " : ""}${formatCost(summary.cost, summary.currency)}`;
}

function TokenMap({ tokens }: { tokens: TokenBreakdown }) {
  const adjustment = tokens.reported_total_adjustment;
  const namedBuckets = [
    { label: "Input", value: tokens.input_tokens, tone: "input" },
    { label: "Output", value: tokens.output_tokens, tone: "output" },
    { label: "Reasoning", value: tokens.reasoning_tokens, tone: "reasoning" },
    { label: "Cache write", value: tokens.cache_creation_tokens, tone: "cache-write" },
    { label: "Cache read", value: tokens.cache_read_tokens, tone: "cache-read" },
  ];
  const componentTotal = namedBuckets.reduce((sum, bucket) => sum + Math.max(bucket.value, 0), 0);
  const total = Math.max(tokens.total_tokens, componentTotal, 1);
  const buckets = adjustment > 0
    ? [...namedBuckets, { label: "Provider adjustment", value: adjustment, tone: "adjustment" }]
    : namedBuckets;

  return (
    <>
      <div className="token-map" aria-label="Token composition">
        {buckets.map((bucket) => {
          const percentage = (bucket.value / total) * 100;
          return bucket.value > 0 ? (
            <div
              className={`token-map-segment map-${bucket.tone}`}
              key={bucket.label}
              style={{ flexBasis: `${percentage}%` }}
            >
              {percentage >= 10 ? (
                <><span>{bucket.label}</span><strong>{formatTokens(bucket.value)}</strong></>
              ) : null}
            </div>
          ) : null;
        })}
      </div>
      <dl className="token-key">
        {[...namedBuckets, ...(adjustment !== 0 ? [{ label: "Provider adjustment", value: adjustment, tone: "adjustment" }] : [])].map((bucket) => (
          <div key={bucket.label}>
            <dt><i className={`key-${bucket.tone}`} />{bucket.label}</dt>
            <dd>{formatTokens(bucket.value)}</dd>
            <small>{((bucket.value / total) * 100).toFixed(1)}%</small>
          </div>
        ))}
      </dl>
      {adjustment < 0 ? <p className="token-adjustment-note">The provider-reported total is {formatTokens(Math.abs(adjustment))} tokens below the named components. The total remains authoritative.</p> : null}
    </>
  );
}

function ModelTable({ summary }: { summary: CostSummary }) {
  return (
    <section className="work-pane table-pane" aria-labelledby="models-title">
      <header className="pane-heading">
        <div><h2 id="models-title">Models</h2><p>Provider model attribution for this period.</p></div>
        <span>{summary.models.length} observed</span>
      </header>
      <div className="table-scroll">
        <table>
          <thead><tr><th>Model</th><th>Input</th><th>Output</th><th>Cache</th><th>Total</th><th>Cost</th></tr></thead>
          <tbody>
            {summary.models.map((model) => (
              <tr key={model.model}>
                <td className="identity-cell">{model.model}</td>
                <td>{formatTokens(model.tokens.input_tokens)}</td>
                <td>{formatTokens(model.tokens.output_tokens)}</td>
                <td>{formatTokens(model.tokens.cache_read_tokens)}</td>
                <td>{formatTokens(model.tokens.total_tokens)}</td>
                <td className={hasExactCost(summary) && hasExactCost(model) ? "known-cost" : "unknown-cost"}>{costIsLowerBound(summary) ? "≥ " : model.cost !== null && (!hasExactCost(summary) || !hasExactCost(model)) ? "≈ " : ""}{formatCost(model.cost, summary.currency)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </section>
  );
}

function SourceTable({ overview, range }: { overview: UsageOverview; range: UsageRange }) {
  const rows = overview.source_overviews?.map((source) => ({
    source,
    summary: source.summaries.find((candidate) => candidate.range === range),
  })).filter((row): row is { source: UsageOverview; summary: CostSummary } => row.summary !== undefined) ?? [];

  if (rows.length === 0) return null;
  return (
    <section className="work-pane table-pane" aria-labelledby="sources-title">
      <header className="pane-heading">
        <div><h2 id="sources-title">Sources</h2><p>Every ready AI ledger included in the total.</p></div>
        <span>{rows.length} scanned</span>
      </header>
      <div className="table-scroll">
        <table>
          <thead><tr><th>Source</th><th>Records</th><th>Tokens</th><th>Cost</th></tr></thead>
          <tbody>
            {rows.map(({ source, summary }) => (
              <tr key={source.source_name}>
                <td className="identity-cell">{source.display_name}</td>
                <td>{formatTokens(summary.valid_entries)}</td>
                <td>{formatTokens(summary.tokens.total_tokens)}</td>
                <td className={hasExactCost(summary) ? "known-cost" : "unknown-cost"}>{displayedCost(summary)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </section>
  );
}

function EmptyState({ source, range, parseErrors }: { source: string; range: string; parseErrors: number }) {
  return (
    <section className="state-pane empty-state">
      <span aria-hidden="true">○</span>
      <div>
        <h2>{parseErrors > 0 ? `No valid ${source} records in this window.` : `The ${source} ledger is quiet in this window.`}</h2>
        {parseErrors > 0
          ? <p data-testid="unattributed-parse-errors">{formatTokens(parseErrors)} malformed records were rejected during the combined scan. Their dates are unavailable, so they cannot be assigned to {range}.</p>
          : <p>Run the source once or choose a wider period. ccstats never invents missing usage.</p>}
      </div>
    </section>
  );
}

function OverviewView({
  overview,
  summary,
  sourceDescriptor,
  range,
}: {
  overview: UsageOverview;
  summary: CostSummary;
  sourceDescriptor: SourceDescriptor | null;
  range: UsageRange;
}) {
  return (
    <>
      <div className="report-meta">
        <div><span className="source-beacon" /><strong>{overview.display_name}</strong><span>{summary.range.replaceAll("_", " ")}</span></div>
        <span>Scanned in {overview.elapsed_ms.toFixed(1)} ms · {new Date(overview.generated_at).toLocaleString()}</span>
      </div>

      {summary.valid_entries === 0 && summary.tokens.total_tokens === 0 ? (
        <EmptyState source={overview.display_name} range={range.replaceAll("_", " ")} parseErrors={summary.parse_error_entries} />
      ) : (
        <>
          <section className="overview-workbench">
            <article className="work-pane token-map-pane">
              <header className="pane-heading">
                <div><h2>Token map</h2><p>Reported components reconciled on one shared scale.</p></div>
                <span>{summary.currency}</span>
              </header>
              <div className="primary-reading">
                <span>Total tokens</span>
                <strong data-testid="total-tokens">{formatTokens(summary.tokens.total_tokens)}</strong>
              </div>
              <TokenMap tokens={summary.tokens} />
              <QualityStatus summary={summary} />
            </article>

            <aside className="work-pane snapshot-pane" aria-label="Usage snapshot">
              <header className="pane-heading"><div><h2>Snapshot</h2><p>Trust and accounting signals.</p></div></header>
              <dl className="snapshot-list">
                <div>
                  <dt>{costIsLowerBound(summary) ? "Cost lower bound" : hasExactCost(summary) ? "Total cost" : "Cost to review"}</dt>
                  <dd data-testid="total-cost" className={hasExactCost(summary) ? "known-cost" : "unknown-cost"}>{displayedCost(summary)}</dd>
                  <small data-testid={costIsLowerBound(summary) ? "cost-coverage" : undefined}>{costIsLowerBound(summary) && summary.api_equivalent_cost_coverage ? `${formatTokens(summary.api_equivalent_cost_coverage.priced_tokens)} / ${formatTokens(summary.api_equivalent_cost_coverage.total_tokens)} tokens priced (${summary.api_equivalent_cost_coverage.percent.toFixed(1)}%)` : summary.cost === null ? "No trustworthy price" : hasExactCost(summary) ? summary.cost_kind : summary.pricing_source}</small>
                </div>
                <div><dt>Records</dt><dd>{formatTokens(summary.valid_entries)}</dd><small>Parsed source events</small></div>
                <div><dt>Cache hit</dt><dd>{summary.tokens.cache_hit_rate === null ? "—" : `${summary.tokens.cache_hit_rate.toFixed(1)}%`}</dd><small>Input-side reuse</small></div>
              </dl>
              <div className="capability-block">
                <span>Available evidence</span>
                <div>
                  {sourceDescriptor?.has_projects ? <em>Projects</em> : null}
                  {sourceDescriptor?.has_reasoning_tokens ? <em>Reasoning</em> : null}
                  {sourceDescriptor?.has_cache_read ? <em>Cache read</em> : null}
                  {sourceDescriptor?.has_cache_creation ? <em>Cache write</em> : null}
                </div>
              </div>
            </aside>
          </section>
          <SourceTable overview={overview} range={range} />
          <ModelTable summary={summary} />
        </>
      )}
    </>
  );
}

function ProjectView({ data }: { data: ProjectDrilldownSummary }) {
  const [selectedPath, setSelectedPath] = useState<string | null>(null);
  const selected = data.projects.find((project) => project.project_path === selectedPath) ?? null;

  if (data.projects.length === 0 && data.quality.parse_error_entries > 0) return <AnalysisQualityNotice quality={data.quality} />;
  if (data.projects.length === 0) return <EmptyState source={data.display_name} range={data.range.replaceAll("_", " ")} parseErrors={0} />;
  return (
    <><AnalysisQualityNotice quality={data.quality} /><section className="project-workbench" aria-labelledby="projects-title">
      <div className="work-pane project-master">
        <header className="pane-heading">
          <div><h2 id="projects-title">Projects &amp; sessions</h2><p>Select a project to inspect its sessions.</p></div>
          <span>{data.projects.length} projects</span>
        </header>
        <div className="table-scroll">
          <table>
            <thead><tr><th>Project</th><th>Sessions</th><th>Tokens</th><th>Cost</th></tr></thead>
            <tbody>
              {data.projects.map((project) => (
                <tr className={selectedPath === project.project_path ? "selected-row" : ""} key={project.project_path}>
                  <td className="project-cell">
                    <button type="button" aria-pressed={selectedPath === project.project_path} onClick={() => setSelectedPath(project.project_path)}>{project.project_name}</button>
                    <small>{project.project_path}</small>
                  </td>
                  <td>{formatTokens(project.session_count)}</td>
                  <td>{formatTokens(project.metrics.tokens.total_tokens)}</td>
                  <td className={hasExactCost(project.metrics) ? "known-cost" : "unknown-cost"}>{project.metrics.api_equivalent_cost_coverage?.cost_is_lower_bound ? "≥ " : project.metrics.cost !== null && !hasExactCost(project.metrics) ? "≈ " : ""}{formatCost(project.metrics.cost, data.currency)}<small>{project.metrics.pricing_source}</small></td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </div>

      <aside className="work-pane session-inspector">
        {selected ? (
          <>
            <header className="pane-heading">
              <div><h2>{selected.project_name}</h2><p className="mono-detail">{selected.project_path}</p></div>
              <span>{selected.session_count} sessions</span>
            </header>
            <div className="session-list">
              {selected.sessions.map((session) => (
                <article key={session.session_id}>
                  <div><strong>{session.session_id}</strong><span>{new Date(session.last_timestamp).toLocaleString()}</span></div>
                  <dl><div><dt>Tokens</dt><dd>{formatTokens(session.metrics.tokens.total_tokens)}</dd></div><div><dt>Cost</dt><dd className={hasExactCost(session.metrics) ? "known-cost" : "unknown-cost"}>{session.metrics.api_equivalent_cost_coverage?.cost_is_lower_bound ? "≥ " : session.metrics.cost !== null && !hasExactCost(session.metrics) ? "≈ " : ""}{formatCost(session.metrics.cost, data.currency)}<small>{session.metrics.pricing_source}</small></dd></div></dl>
                </article>
              ))}
            </div>
          </>
        ) : (
          <div className="inspector-empty"><span aria-hidden="true">↳</span><h2>Select a project</h2><p>Its session IDs, last activity, tokens, and cost will appear here.</p></div>
        )}
      </aside>
    </section></>
  );
}

function HistoryView({ data, exporting, onExport }: { data: UsageHistory; exporting: boolean; onExport: (format: "csv" | "json") => void }) {
  const maxTokens = Math.max(...data.points.map((point) => point.tokens.total_tokens), 1);

  if (data.points.length === 0 && data.quality.parse_error_entries > 0) return <AnalysisQualityNotice quality={data.quality} />;
  if (data.points.length === 0) return <EmptyState source={data.display_name} range={data.range.replaceAll("_", " ")} parseErrors={0} />;
  return (
    <><AnalysisQualityNotice quality={data.quality} /><section className="work-pane history-workbench" aria-labelledby="history-title">
      <header className="pane-heading history-heading">
        <div><h2 id="history-title">History</h2><p>Daily token totals with exact records below.</p></div>
        <div className="export-actions"><button type="button" disabled={exporting} onClick={() => onExport("csv")}>Export CSV</button><button type="button" disabled={exporting} onClick={() => onExport("json")}>Export JSON</button></div>
      </header>
      <figure className="history-chart">
        <div className="chart-plot" aria-label="Token history chart">
          {data.points.map((point) => (
            <div className="chart-slot" key={point.date}>
              <span className="chart-value">{formatTokens(point.tokens.total_tokens)}</span>
              <i style={{ height: `${Math.max((point.tokens.total_tokens / maxTokens) * 100, 3)}%` }} />
              <small>{point.date.slice(5)}</small>
            </div>
          ))}
        </div>
        <figcaption>All bars share a zero baseline. Dates without records are not fabricated.</figcaption>
      </figure>
      <div className="table-scroll">
        <table>
          <thead><tr><th>Date</th><th>Records</th><th>Input</th><th>Output</th><th>Total</th><th>Cost</th></tr></thead>
          <tbody>
            {data.points.map((point) => (
              <tr key={point.date}>
                <td className="identity-cell">{point.date}</td>
                <td>{formatTokens(point.records)}</td>
                <td>{formatTokens(point.tokens.input_tokens)}</td>
                <td>{formatTokens(point.tokens.output_tokens)}</td>
                <td>{formatTokens(point.tokens.total_tokens)}</td>
                <td className={point.cost_status === "known" ? "known-cost" : "unknown-cost"}>{point.api_equivalent_cost_coverage?.cost_is_lower_bound ? "≥ " : point.cost_status === "partial" ? "≈ " : ""}{formatCost(point.cost, point.currency)}<small>{point.pricing_source}</small></td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </section></>
  );
}

function ConsumerTable({ title, rows, currency }: { title: string; rows: RankedConsumer[]; currency: string }) {
  if (rows.length === 0) return null;
  return (
    <section className="work-pane table-pane consumer-pane">
      <header className="pane-heading"><div><h3>{title}</h3><p>Share uses {rows[0].share_basis} across this complete ranking.</p></div></header>
      <div className="table-scroll"><table>
        <thead><tr><th>#</th><th>Consumer</th><th>Tokens</th><th>Share</th><th>Cost</th></tr></thead>
        <tbody>{rows.map((row) => <tr key={row.name}><td>{row.rank}</td><td className="identity-cell">{row.name}</td><td>{formatTokens(row.tokens)}</td><td>{row.share.toFixed(1)}%</td><td className={row.cost === null ? "unknown-cost" : "known-cost"}>{formatCost(row.cost, currency)}</td></tr>)}</tbody>
      </table></div>
    </section>
  );
}

function TopInsightsView({ overview, summary, history, projects, warnings }: { overview: UsageOverview; summary: CostSummary; history: UsageHistory | null; projects: ProjectDrilldownSummary | null; warnings: string[] }) {
  const models = rankConsumers(summary.models.map((model) => ({ name: model.model, tokens: model.tokens.total_tokens, cost: hasExactCost(summary) && hasExactCost(model) ? model.cost : null })));
  const sources = rankConsumers(overview.source_overviews?.flatMap((source) => {
    const row = source.summaries.find((candidate) => candidate.range === summary.range);
    return row ? [{ name: source.display_name, tokens: row.tokens.total_tokens, cost: hasExactCost(row) ? row.cost : null }] : [];
  }) ?? []);
  const projectRows = rankConsumers(projects?.projects.map((project) => ({ name: project.project_name, tokens: project.metrics.tokens.total_tokens, cost: hasExactCost(project.metrics) ? project.metrics.cost : null })) ?? []);
  const anomaly = detectUsageAnomaly(history);
  return (
    <section className="top-workbench" aria-labelledby="top-title">
      <AnalysisQualityNotice quality={costSummaryQuality(summary)} combinedScan />
      <header className="work-pane pane-heading top-heading"><div><h2 id="top-title">Top consumers</h2><p>Ranked by cost when every row is priced; otherwise by tokens.</p></div></header>
      <aside className={`work-pane anomaly-pane anomaly-${anomaly.status}`} role="status">
        <div><strong>{anomaly.status === "spike" ? "Usage spike detected" : anomaly.status === "normal" ? "No usage spike" : "Not enough history"}</strong><p>{anomaly.date ?? "A concrete source needs at least four complete usage days."}</p></div>
        <dl><div><dt>Latest</dt><dd>{anomaly.tokens === null ? "—" : formatTokens(anomaly.tokens)}</dd></div><div><dt>7-day baseline</dt><dd>{anomaly.baseline_tokens === null ? "—" : formatTokens(Math.round(anomaly.baseline_tokens))}</dd></div><div><dt>Change</dt><dd data-testid="anomaly-change">{anomaly.change_pct === null ? "—" : `${anomaly.change_pct.toFixed(1)}%`}</dd></div></dl>
      </aside>
      {warnings.map((warning) => <p className="inline-warning" key={warning}>{warning}</p>)}
      <div className="consumer-grid"><ConsumerTable title="Models" rows={models} currency={summary.currency} /><ConsumerTable title="Sources" rows={sources} currency={summary.currency} /><ConsumerTable title="Projects" rows={projectRows} currency={summary.currency} /></div>
    </section>
  );
}

function DiagnosticsView({ rows }: { rows: SourceDiagnosticDescriptor[] }) {
  const detected = rows.filter((row) => row.status === "detected").length;
  const configured = rows.filter((row) => row.status === "configured").length;
  const missing = rows.filter((row) => row.status === "missing").length;

  return (
    <section className="work-pane table-pane diagnostics-pane" aria-labelledby="diagnostics-title">
      <header className="pane-heading">
        <div><h2 id="diagnostics-title">Source diagnostics</h2><p>Read-only discovery; remote providers are not contacted.</p></div>
        <span>{rows.length} registered</span>
      </header>
      <div className="diagnostic-summary" aria-label="Diagnostic totals">
        <strong>{`${detected} detected`}</strong>
        <strong>{`${configured} configured`}</strong>
        <strong>{`${missing} missing`}</strong>
      </div>
      <div className="table-scroll">
        <table>
          <thead><tr><th>Source</th><th>Status</th><th>Files</th><th>Detail and next step</th></tr></thead>
          <tbody>
            {rows.map((row) => (
              <tr key={row.name}>
                <td className="identity-cell">{row.display_name}</td>
                <td><span className={`diagnostic-status status-${row.status}`}>{row.status}</span></td>
                <td>{formatTokens(row.files)}</td>
                <td className="diagnostic-detail"><span>{row.detail}</span>{row.status === "missing" ? <small>{row.setup}</small> : null}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </section>
  );
}

function formatPercent(value: number) {
  return `${value.toFixed(1)}%`;
}

function formatTimestamp(value: string | null) {
  if (value === null) return "Not projected";
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? value : date.toLocaleString();
}

function QuotaBudgetView({ quota, quotaError, monthly, budget, onBudgetChange }: {
  quota: CodexQuotaOverview | null;
  quotaError: string | null;
  monthly: CostSummary | null;
  budget: string;
  onBudgetChange: (value: string) => void;
}) {
  const limit = Number(budget);
  const validLimit = Number.isFinite(limit) && limit > 0;
  const spent = monthly?.cost ?? null;
  const spentIsLowerBound = monthly?.api_equivalent_cost_coverage?.cost_is_lower_bound === true;
  const spentIsExact = monthly !== null && hasExactCost(monthly);
  const spentPrefix = spentIsLowerBound ? "≥ " : spent !== null && !spentIsExact ? "≈ " : "";
  const [asOfYear, asOfMonth, daysElapsed] = monthly?.until?.split("-").map(Number) ?? [];
  const daysInMonth = asOfYear && asOfMonth ? new Date(Date.UTC(asOfYear, asOfMonth, 0)).getUTCDate() : 0;
  const projected = spent === null || !daysElapsed || !daysInMonth ? null : spent * daysInMonth / daysElapsed;
  const remaining = spent === null || !validLimit || !spentIsExact ? null : limit - spent;
  const budgetStatus = spentIsLowerBound ? "Lower bound" : spent !== null && !spentIsExact ? "Review" : spent === null || projected === null || !validLimit
    ? "Unavailable"
    : projected > limit
      ? "Over budget"
      : projected >= limit * 0.9 ? "Watch" : "On track";

  return (
    <section className="limits-workbench" aria-labelledby="limits-title">
      {monthly ? <AnalysisQualityNotice quality={costSummaryQuality(monthly)} combinedScan /> : null}
      <header className="work-pane pane-heading limits-heading">
        <div><h2 id="limits-title">Quota and budget</h2><p>Provider quota stays separate from local cost forecasting.</p></div>
      </header>
      <div className="limits-grid">
        <section className="work-pane limit-pane" aria-labelledby="quota-title">
          <header className="pane-heading"><div><h3 id="quota-title">Codex weekly quota</h3><p>Latest provider-reported weekly window.</p></div></header>
          {quota ? (
            <>
              <dl className="limit-metrics">
                <div><dt>Used</dt><dd data-testid="quota-used">{formatPercent(quota.quota.used_pct)}</dd></div>
                <div><dt>Remaining</dt><dd data-testid="quota-remaining">{formatPercent(quota.quota.remaining_pct)}</dd></div>
                <div><dt>Projected at reset</dt><dd>{formatPercent(quota.quota.projected_pct_at_reset)}</dd></div>
                <div><dt>Status</dt><dd>{quota.quota.status.replaceAll("_", " ")}</dd></div>
              </dl>
              <dl className="limit-details">
                <div><dt>Resets</dt><dd>{formatTimestamp(quota.quota.resets_at)}</dd></div>
                <div><dt>Projected depletion</dt><dd>{formatTimestamp(quota.quota.estimated_depletion_at)}</dd></div>
                <div><dt>API-equivalent weekly value</dt><dd data-testid="quota-value">{quota.value_estimate ? formatCost(quota.value_estimate.estimated_weekly_value_usd, "USD") : "Unavailable"}</dd></div>
              </dl>
              {quota.value_estimate_error ? <p className="inline-warning">{quota.value_estimate_error}</p> : null}
            </>
          ) : <p className="inline-warning">{quotaError ?? "Quota snapshot unavailable"}</p>}
        </section>

        <section className="work-pane limit-pane" aria-labelledby="budget-title">
          <header className="pane-heading"><div><h3 id="budget-title">Monthly budget</h3><p>Forecast from confirmed month-to-date cost.</p></div></header>
          <label className="budget-input">Monthly budget<input aria-label="Monthly budget" type="number" min="0.01" step="1" value={budget} onChange={(event) => onBudgetChange(event.target.value)} /></label>
          {!validLimit ? <p className="inline-warning">Enter a positive monthly budget.</p> : null}
          <dl className="limit-metrics">
            <div><dt>Spent</dt><dd data-testid="budget-spent">{spentPrefix}{formatCost(spent, monthly?.currency ?? "USD")}</dd></div>
            <div><dt>Remaining</dt><dd data-testid="budget-remaining">{formatCost(remaining, monthly?.currency ?? "USD")}</dd></div>
            <div><dt>Month-end forecast</dt><dd>{spentPrefix}{formatCost(projected, monthly?.currency ?? "USD")}</dd></div>
            <div><dt>Status</dt><dd>{budgetStatus}</dd></div>
          </dl>
          <p className="limit-note">{daysElapsed} of {daysInMonth} days elapsed. Unknown costs remain unknown.</p>
        </section>
      </div>
    </section>
  );
}

export default function App() {
  const bridge = useMemo(() => desktopBridge(), []);
  const requestId = useRef(0);
  const [sources, setSources] = useState<SourceDescriptor[]>([]);
  const [selectedSource, setSelectedSource] = useState("");
  const [selectedRange, setSelectedRange] = useState<UsageRange>("today");
  const [view, setView] = useState<AnalyticsView>("overview");
  const [overview, setOverview] = useState<UsageOverview | null>(null);
  const [drilldown, setDrilldown] = useState<ProjectDrilldownSummary | null>(null);
  const [history, setHistory] = useState<UsageHistory | null>(null);
  const [diagnostics, setDiagnostics] = useState<SourceDiagnosticDescriptor[] | null>(null);
  const [quotaOverview, setQuotaOverview] = useState<CodexQuotaOverview | null>(null);
  const [quotaError, setQuotaError] = useState<string | null>(null);
  const [monthlyBudget, setMonthlyBudget] = useState("100");
  const [insightHistory, setInsightHistory] = useState<UsageHistory | null>(null);
  const [insightProjects, setInsightProjects] = useState<ProjectDrilldownSummary | null>(null);
  const [insightWarnings, setInsightWarnings] = useState<string[]>([]);
  const [exporting, setExporting] = useState(false);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const readySourceNames = useMemo(() => {
    const ready = new Set(diagnostics?.filter((row) => (row.status === "detected" || row.status === "configured")).map((row) => row.name) ?? []);
    return sources.filter((source) => source.name !== "all" && ready.has(source.name)).map((source) => source.name);
  }, [diagnostics, sources]);
  const readOverview = useCallback(async (source: string) => {
    if (source !== "all") return bridge.usageOverview(source);
    const findings = await bridge.sourceDiagnostics(); setDiagnostics(findings);
    const ready = new Set(findings.filter((row) => (row.status === "detected" || row.status === "configured")).map((row) => row.name));
    const names = sources.filter((item) => item.name !== "all" && ready.has(item.name)).map((item) => item.name);
    if (names.length === 0) throw new Error("No detected or configured sources are ready to aggregate.");
    return aggregateUsageOverviews(await bridge.usageOverviews(names));
  }, [bridge, sources]);

  const loadView = useCallback(async (nextView: AnalyticsView, source: string, range: UsageRange) => {
    const request = ++requestId.current;
    setView(nextView);
    if (nextView === "live" || nextView === "machines" || nextView === "activity") { setLoading(false); setError(null); return; }
    setLoading(true);
    setError(null);
    try {
      if (nextView === "overview" || nextView === "trust") {
        const nextOverview = await readOverview(source);
        if (request !== requestId.current) return;
        setOverview(nextOverview);
      } else if (nextView === "top") {
        const supportsProjects = sources.find((item) => item.name === source)?.has_projects ?? false;
        const [nextOverview, nextHistory, nextProjects] = await Promise.allSettled([
          readOverview(source),
          source === "all" ? Promise.resolve(null) : bridge.usageHistory(source, "this_month"),
          source !== "all" && supportsProjects ? bridge.projectDrilldown(source, range) : Promise.resolve(null),
        ]);
        if (request !== requestId.current) return;
        if (nextOverview.status === "rejected") throw nextOverview.reason;
        setOverview(nextOverview.value);
        setInsightHistory(nextHistory.status === "fulfilled" ? nextHistory.value : null);
        setInsightProjects(nextProjects.status === "fulfilled" ? nextProjects.value : null);
        setInsightWarnings([nextHistory, nextProjects].flatMap((result) => result.status === "rejected" ? [errorMessage(result.reason)] : []));
      } else if (nextView === "projects") {
        const next = source === "all" ? null : await bridge.projectDrilldown(source, range);
        if (request !== requestId.current) return;
        setHistory(null); setDrilldown(next);
      } else if (nextView === "history") {
        const next = source === "all" ? null : await bridge.usageHistory(source, range);
        if (request !== requestId.current) return;
        setDrilldown(null); setHistory(next);
      } else if (nextView === "diagnostics") {
        const next = await bridge.sourceDiagnostics();
        if (request !== requestId.current) return;
        setDrilldown(null); setHistory(null); setDiagnostics(next);
      } else {
        const [nextOverview, nextQuota] = await Promise.allSettled([
          readOverview(source),
          bridge.codexQuotaOverview(),
        ]);
        if (request !== requestId.current) return;
        if (nextOverview.status === "rejected") throw nextOverview.reason;
        setOverview(nextOverview.value);
        if (nextQuota.status === "fulfilled") {
          setQuotaOverview(nextQuota.value);
          setQuotaError(null);
        } else {
          setQuotaOverview(null);
          setQuotaError(errorMessage(nextQuota.reason));
        }
      }
    } catch (nextError) {
      if (request !== requestId.current) return;
      if (nextView === "top") { setOverview(null); setInsightHistory(null); setInsightProjects(null); }
      else if (nextView === "projects") setDrilldown(null);
      else if (nextView === "history") setHistory(null);
      else if (nextView === "diagnostics") setDiagnostics(null);
      else setOverview(null);
      setError(errorMessage(nextError));
    } finally {
      if (request === requestId.current) setLoading(false);
    }
  }, [bridge, readOverview, sources]);

  const initialize = useCallback(async () => {
    const request = ++requestId.current;
    setLoading(true);
    setError(null);
    setOverview(null);
    try {
      const [catalog, findings] = await Promise.all([bridge.listSources(), bridge.sourceDiagnostics()]);
      if (catalog.length === 0) throw new Error("No usage sources are registered");
      const allSources: SourceDescriptor = {
        source: "all",
        name: "all",
        display_name: "All Sources",
        aliases: [],
        has_projects: false,
        has_reasoning_tokens: catalog.some((source) => source.has_reasoning_tokens),
        has_cache_creation: catalog.some((source) => source.has_cache_creation),
        has_cache_read: catalog.some((source) => source.has_cache_read),
      };
      if (request !== requestId.current) return;
      const ready = new Set(findings.filter((row) => (row.status === "detected" || row.status === "configured")).map((row) => row.name));
      const initial = catalog.find((source) => ready.has(source.name));
      setSources([allSources, ...catalog]);
      setDiagnostics(findings);
      setSelectedSource((initial ?? catalog.find((source) => source.name === "claude") ?? catalog[0]).name);
      if (!initial) { setView("diagnostics"); setOverview(null); return; }
      const nextOverview = await bridge.usageOverview(initial.name);
      if (request === requestId.current) setOverview(nextOverview);
    } catch (nextError) {
      if (request === requestId.current) setError(errorMessage(nextError));
    } finally {
      if (request === requestId.current) setLoading(false);
    }
  }, [bridge]);

  useEffect(() => { void initialize(); }, [initialize]);
  const live = useLiveUsage(bridge, sources, selectedSource, view === "live");

  const summary = overview?.summaries.find((item) => item.range === selectedRange) ?? null;
  const monthlySummary = overview?.summaries.find((item) => item.range === "this_month") ?? null;
  const sourceDescriptor = sources.find((source) => source.name === selectedSource) ?? null;
  const selectedRangeLabel = RANGE_OPTIONS.find((range) => range.value === selectedRange)?.label ?? "";
  const selectedSourceLabel = sources.find((source) => source.name === selectedSource)?.display_name ?? "Usage source";
  const missingRangeError = (view === "overview" || view === "trust" || view === "top") && overview && !summary
    ? `${selectedRangeLabel} is missing from the usage response.`
    : null;

  function changeSource(source: string) {
    setSelectedSource(source);
    void loadView(view, source, selectedRange);
  }

  function changeRange(range: UsageRange) {
    setSelectedRange(range);
    if (view === "top" || view === "projects" || view === "history") void loadView(view, selectedSource, range);
  }

  function retry() {
    if (sources.length === 0) void initialize();
    else void loadView(view, selectedSource, selectedRange);
  }

  async function exportHistory(format: "csv" | "json") {
    setExporting(true);
    setError(null);
    try {
      const content = await bridge.exportHistory(selectedSource, selectedRange, format);
      const url = URL.createObjectURL(new Blob([content], { type: format === "csv" ? "text/csv" : "application/json" }));
      const link = document.createElement("a");
      link.href = url;
      link.download = `ccstats-${selectedSource}-${selectedRange}.${format}`;
      link.click();
      URL.revokeObjectURL(url);
    } catch (nextError) {
      setError(errorMessage(nextError));
    } finally {
      setExporting(false);
    }
  }

  const pageTitle = view === "overview" ? "Usage overview" : view === "trust" ? "Cost trust" : view === "live" ? "Live usage" : view === "top" ? "Top consumers" : view === "limits" ? "Quota and budget" : view === "machines" ? "Machine rollup" : view === "activity" ? "Turns and tools" : view === "projects" ? "Project explorer" : view === "history" ? "Usage history" : "Source diagnostics";
  const pageDescription = view === "overview"
    ? "Reconcile tokens, cost, and provider evidence."
    : view === "trust"
      ? "Trace every displayed amount to its usage basis, pricing source, and coverage boundary."
    : view === "live"
      ? "Watch today’s local ledger and measure growth from a trusted baseline."
    : view === "top"
      ? "Find dominant consumers and compare the latest complete day with its baseline."
      : view === "limits"
      ? "Track the provider quota window and forecast monthly spend."
      : view === "machines"
      ? "Persist aggregate snapshots and combine usage from your other devices."
      : view === "activity"
      ? "Inspect deduplicated model responses and independently counted tool calls."
      : view === "projects"
      ? "Trace a project total into the sessions that produced it."
      : view === "history"
        ? "Inspect daily movement and export the underlying ledger."
        : "Check every registered source and see the next setup action.";

  return (
    <div className="polar-shell">
      <aside className="sidebar">
        <div className="brand-lockup"><span aria-hidden="true">c</span><div><strong>ccstats</strong><small>Usage intelligence</small></div></div>
        <nav className="side-nav" aria-label="Analytics view">
          {VIEW_GROUPS.map((group) => <section className="nav-group" aria-label={group.label} key={group.label}><h2>{group.label}</h2>{group.views.map((item) => <button type="button" key={item.value} className={view === item.value ? "active" : ""} aria-label={item.label} aria-pressed={view === item.value} disabled={loading || sources.length === 0} onClick={() => void loadView(item.value, selectedSource, selectedRange)}><div><strong>{item.label}</strong><small>{item.detail}</small></div></button>)}</section>)}
        </nav>
        <div className="sidebar-source">
          <label htmlFor="source-select">Usage source</label>
          <div>
            <select id="source-select" value={selectedSource} onChange={(event) => changeSource(event.target.value)} disabled={loading || sources.length === 0}>
              {sources.map((source) => <option key={source.name} value={source.name}>{source.display_name}</option>)}
            </select>
            <span aria-hidden="true">⌄</span>
          </div>
          <small>{Math.max(sources.length - 1, 0)} registered · {readySourceNames.length} ready</small>
        </div>
        <div className="local-status"><i aria-hidden="true" /><div><strong>Local only</strong><small>No transcript upload</small></div></div>
      </aside>

      <div className="workspace">
        <header className="workspace-toolbar">
          <div className="page-heading"><span>{view === "machines" ? "Local snapshot store" : selectedSourceLabel}</span><h1>{pageTitle}</h1><p>{pageDescription}</p></div>
          <div className="toolbar-actions">
            {view !== "diagnostics" && view !== "limits" && view !== "live" && view !== "machines" ? <div className="period-control" aria-label="Usage period">
              {RANGE_OPTIONS.map((range) => <button type="button" key={range.value} aria-label={range.label} aria-pressed={selectedRange === range.value} className={selectedRange === range.value ? "active" : ""} onClick={() => changeRange(range.value)}>{range.label}</button>)}
            </div> : null}
            {view !== "machines" && view !== "activity" ? <button type="button" className="refresh-button" aria-label="Refresh ledger" onClick={() => view === "live" ? void live.refresh(false) : void loadView(view, selectedSource, selectedRange)} disabled={loading || live.refreshing || selectedSource.length === 0}><span aria-hidden="true">↻</span> Refresh</button> : null}
          </div>
        </header>

        <main className="workspace-content">
          {loading ? (
            <section className="loading-state" aria-live="polite"><div><i /><i /><i /></div><strong>{view === "overview" ? "Auditing registered sources…" : view === "limits" ? "Reading quota and budget evidence…" : view === "projects" ? "Resolving projects and sessions…" : view === "history" ? "Building daily history…" : "Checking source readiness…"}</strong><span>Discover · parse · deduplicate · price</span></section>
          ) : error || missingRangeError ? (
            <section className="state-pane error-state" role="alert"><span aria-hidden="true">!</span><div><h2>Could not read this source.</h2><p>{error ?? missingRangeError}</p><button type="button" onClick={retry}>Try again</button></div></section>
          ) : view === "live" ? (
            <LiveView live={live} />
          ) : view === "machines" ? (
            <MachinesView bridge={bridge} sources={sources} />
          ) : view === "activity" ? (
            <ActivityView bridge={bridge} source={selectedSource} range={selectedRange} />
          ) : view === "trust" ? (
            summary && overview ? <><AnalysisQualityNotice quality={costSummaryQuality(summary)} combinedScan /><CostTrustView overview={overview} summary={summary} range={selectedRange} /></> : null
          ) : view === "top" ? (
            summary && overview ? <TopInsightsView overview={overview} summary={summary} history={insightHistory} projects={insightProjects} warnings={insightWarnings} /> : null
          ) : view === "limits" ? (
            <QuotaBudgetView quota={quotaOverview} quotaError={quotaError} monthly={monthlySummary} budget={monthlyBudget} onBudgetChange={setMonthlyBudget} />
          ) : view === "diagnostics" ? (
            diagnostics ? <DiagnosticsView rows={diagnostics} /> : null
          ) : view === "projects" ? (
            selectedSource === "all"
              ? <section className="state-pane empty-state"><span aria-hidden="true">↳</span><div><h2>Choose one source</h2><p>Choose a concrete source to inspect projects and sessions.</p></div></section>
              : drilldown ? <ProjectView data={drilldown} /> : null
          ) : view === "history" ? (
            selectedSource === "all"
              ? <section className="state-pane empty-state"><span aria-hidden="true">↳</span><div><h2>Choose one source</h2><p>Choose a concrete source to inspect and export daily history.</p></div></section>
              : history ? <HistoryView data={history} exporting={exporting} onExport={(format) => void exportHistory(format)} /> : null
          ) : summary && overview ? (
            <div data-testid="overview-content"><OverviewView overview={overview} summary={summary} sourceDescriptor={sourceDescriptor} range={selectedRange} /></div>
          ) : null}
        </main>
        <footer className="workspace-footer"><span>Authoritative fields over inferred totals</span><span>{selectedRangeLabel}</span></footer>
      </div>
    </div>
  );
}
