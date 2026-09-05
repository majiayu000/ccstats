import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  aggregateUsageOverviews,
  readySourcesForAggregation,
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
import { Icon } from "./Icon";
import { SessionTitle } from "./SessionTitle";

type AnalyticsView = "overview" | "trust" | "live" | "top" | "limits" | "machines" | "activity" | "projects" | "history" | "diagnostics";
const VIEW_GROUPS: ReadonlyArray<{ label: string; views: ReadonlyArray<{ value: AnalyticsView; label: string }> }> = [
  { label: "Workspace", views: [
    { value: "overview", label: "Overview" },
    { value: "live", label: "Live" },
    { value: "top", label: "Top consumers" },
  ] },
  { label: "Explore", views: [
    { value: "activity", label: "Turns & tools" },
    { value: "projects", label: "Projects" },
    { value: "history", label: "History" },
  ] },
  { label: "Manage", views: [
    { value: "trust", label: "Cost evidence" },
    { value: "limits", label: "Limits" },
    { value: "diagnostics", label: "Diagnostics" },
  ] },
  { label: "Devices", views: [
    { value: "machines", label: "Machines" },
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
  const buckets = [
    { label: "Input", value: tokens.input_tokens, tone: "input", color: "#ca6546" },
    { label: "Output", value: tokens.output_tokens, tone: "output", color: "#e9ae89" },
    { label: "Reasoning", value: tokens.reasoning_tokens, tone: "reasoning", color: "#6d7f91" },
    { label: "Cache write", value: tokens.cache_creation_tokens, tone: "cache-write", color: "#c9bc9e" },
    { label: "Cache read", value: tokens.cache_read_tokens, tone: "cache-read", color: "#718477" },
    ...(tokens.reported_total_adjustment !== 0 ? [{ label: "Provider adjustment", value: tokens.reported_total_adjustment, tone: "adjustment", color: "#a494af" }] : []),
  ];
  const positiveTotal = buckets.reduce((sum, bucket) => sum + Math.max(bucket.value, 0), 0);
  let offset = 0;
  return (
    <div className="composition-layout">
      <div className="composition-ring">
        <svg viewBox="0 0 200 200" role="img" aria-label="Token composition; exact values in the legend">
          <circle cx="100" cy="100" r="82" fill="none" stroke="var(--line)" strokeWidth="20" />
          {buckets.filter((bucket) => bucket.value > 0).map((bucket) => {
            const share = bucket.value / positiveTotal * 100;
            const start = offset;
            offset += share;
            return <circle key={bucket.tone} cx="100" cy="100" r="82" fill="none" stroke={bucket.color} strokeWidth="20" pathLength="100" strokeDasharray={`${share} ${100 - share}`} strokeDashoffset={-start} transform="rotate(-90 100 100)" />;
          })}
        </svg>
        <div className="ring-label"><span>Token mix</span><strong>{new Intl.NumberFormat("en-US", { notation: "compact", maximumFractionDigits: 2 }).format(tokens.total_tokens)}</strong><small>Total tokens</small></div>
      </div>
      <dl className="token-key">
        {buckets.map((bucket) => <div key={bucket.tone}><dt><i className={`key-${bucket.tone}`} />{bucket.label}</dt><dd>{formatTokens(bucket.value)}</dd></div>)}
      </dl>
      {tokens.reported_total_adjustment < 0 ? <p className="token-adjustment-note">The provider-reported total is {formatTokens(Math.abs(tokens.reported_total_adjustment))} tokens below the named components. The total remains authoritative; the ring shows positive components.</p> : null}
    </div>
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

function OverviewView({ overview, summary, sourceDescriptor, range, onNavigate }: {
  overview: UsageOverview;
  summary: CostSummary;
  sourceDescriptor: SourceDescriptor | null;
  range: UsageRange;
  onNavigate: (view: AnalyticsView) => void;
}) {
  const models = [...summary.models].sort((a, b) => b.tokens.total_tokens - a.tokens.total_tokens);
  const modelTokens = models.reduce((sum, model) => sum + model.tokens.total_tokens, 0);
  const leadingShare = modelTokens > 0 && models[0] ? models[0].tokens.total_tokens / modelTokens * 100 : 0;
  const reportMetadata = <div className="report-meta"><div><span className="source-beacon" /><strong>{overview.display_name}</strong><span> / {range.replaceAll("_", " ")}</span></div><span>Updated {new Date(overview.generated_at).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}</span></div>;
  if (summary.valid_entries === 0 && summary.tokens.total_tokens === 0) {
    return <>{reportMetadata}<EmptyState source={overview.display_name} range={range.replaceAll("_", " ")} parseErrors={summary.parse_error_entries} /></>;
  }
  return (
    <>
      {reportMetadata}
      <section className="metric-grid" aria-label="Usage snapshot">
        <article className="metric-card metric-primary"><div className="metric-label"><span>Total tokens</span><Icon name="activity" /></div><strong data-testid="total-tokens">{formatTokens(summary.tokens.total_tokens)}</strong><small><span className="metric-dot" />Across {summary.models.length} observed {summary.models.length === 1 ? "model" : "models"}</small></article>
        <article className="metric-card"><div className="metric-label"><span>{costIsLowerBound(summary) ? "Cost lower bound" : hasExactCost(summary) ? "Total cost" : "Cost to review"}</span><Icon name="trust" /></div><strong data-testid="total-cost" className={hasExactCost(summary) ? "" : "unknown-cost"}>{displayedCost(summary)}</strong><small data-testid={costIsLowerBound(summary) ? "cost-coverage" : undefined}>{costIsLowerBound(summary) && summary.api_equivalent_cost_coverage ? `${formatTokens(summary.api_equivalent_cost_coverage.priced_tokens)} / ${formatTokens(summary.api_equivalent_cost_coverage.total_tokens)} tokens priced (${summary.api_equivalent_cost_coverage.percent.toFixed(1)}%)` : summary.cost === null ? "Pricing is not available" : `${summary.pricing_source.replaceAll("_", " ")} pricing · ${summary.currency}`}</small></article>
        <article className="metric-card"><div className="metric-label"><span>Cache hit rate</span><Icon name="refresh" /></div><strong>{summary.tokens.cache_hit_rate === null ? "—" : summary.tokens.cache_hit_rate.toFixed(1)}{summary.tokens.cache_hit_rate !== null ? <em>%</em> : null}</strong><small>{formatTokens(summary.tokens.cache_read_tokens)} tokens reused</small></article>
        <article className="metric-card"><div className="metric-label"><span>Parsed records</span><Icon name="history" /></div><strong>{formatTokens(summary.valid_entries)}</strong><small>{overview.elapsed_ms.toFixed(1)} ms to scan this ledger</small></article>
      </section>

      <section className="overview-workbench">
        <article className="work-pane token-map-pane"><header className="pane-heading"><div><span className="eyebrow">THE BREAKDOWN</span><h2>Where your tokens go</h2></div><span className="subtle-tag">Tokens</span></header><TokenMap tokens={summary.tokens} /></article>
        <article className="work-pane model-distribution"><header className="pane-heading"><div><span className="eyebrow">MODEL MIX</span><h2>Your most-used models</h2></div><Icon name="top" /></header>
          {models.length > 0 ? <><p className="model-insight"><strong>{leadingShare.toFixed(0)}<span>%</span></strong><span>of model-attributed tokens<br />from <b>{models[0].model}</b></span></p><ol className="model-bars">{models.slice(0, 4).map((model, index) => <li key={model.model}><div><span><i className={`model-dot model-color-${index}`} />{model.model}</span><strong>{modelTokens > 0 ? (model.tokens.total_tokens / modelTokens * 100).toFixed(1) : "0.0"}%</strong></div><div className="model-track"><i className={`model-color-${index}`} style={{ width: `${modelTokens > 0 ? model.tokens.total_tokens / modelTokens * 100 : 0}%` }} /></div></li>)}</ol></> : <p className="evidence-note">No model attribution was recorded for this period.</p>}
          <button className="text-action" onClick={() => onNavigate("top")}>Explore consumers <Icon name="arrow" /></button>
        </article>
      </section>
      <div className="quality-row"><QualityStatus summary={summary} /><button className="text-action" onClick={() => onNavigate("trust")}>Inspect pricing <Icon name="arrow" /></button></div>
      <SourceTable overview={overview} range={range} />
      <ModelTable summary={summary} />
      <aside className="explore-banner"><div className="banner-icon"><Icon name="projects" /></div><div><strong>Follow the numbers back to the work.</strong><p>{sourceDescriptor?.has_projects ? "Explore the projects and sessions behind this usage." : "Inspect the recorded model turns behind this usage."}</p></div><button onClick={() => onNavigate(sourceDescriptor?.has_projects ? "projects" : "activity")}>{sourceDescriptor?.has_projects ? "Open explorer" : "View activity"}<Icon name="arrow" /></button></aside>
    </>
  );
}

function ProjectView({ data }: { data: ProjectDrilldownSummary }) {
  const [selectedPath, setSelectedPath] = useState<string | null>(null);
  const selected = data.projects.find((project) => project.project_path === selectedPath) ?? null;

  if (data.projects.length === 0 && data.quality.parse_error_entries > 0) return <AnalysisQualityNotice quality={data.quality} />;
  if (data.projects.length === 0) return <EmptyState source={data.display_name} range={data.range.replaceAll("_", " ")} parseErrors={0} />;
  return (
    <><AnalysisQualityNotice quality={data.quality} />
    {data.session_titles_error && <aside role="alert" className="quality-notice quality-notice-error">
      <strong>Source titles could not be loaded.</strong>
      <span>{data.session_titles_error} Usage totals are still available. Refresh to retry.</span>
    </aside>}
    <section className="project-workbench" aria-labelledby="projects-title">
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
                <article key={`${data.source_name}:${session.session_id}`}>
                  <SessionTitle source={data.source_name} projectName={selected.project_name} session={session} sourceTitle={data.session_titles[session.session_id]} />
                  <dl><div><dt>Tokens</dt><dd>{formatTokens(session.metrics.tokens.total_tokens)}</dd></div><div><dt>Cost</dt><dd className={hasExactCost(session.metrics) ? "known-cost" : "unknown-cost"}>{session.metrics.api_equivalent_cost_coverage?.cost_is_lower_bound ? "≥ " : session.metrics.cost !== null && !hasExactCost(session.metrics) ? "≈ " : ""}{formatCost(session.metrics.cost, data.currency)}<small>{session.metrics.pricing_source}</small></dd></div></dl>
                </article>
              ))}
            </div>
          </>
        ) : (
          <div className="inspector-empty"><span aria-hidden="true">↳</span><h2>Select a project</h2><p>Its session titles, IDs, last activity, tokens, and cost will appear here.</p></div>
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
        <div className="chart-plot" aria-label="Token history chart" style={{ minWidth: `${data.points.length * 46}px` }}>
          {data.points.map((point) => (
            <div className="chart-slot" key={point.date} title={`${point.date}: ${formatTokens(point.tokens.total_tokens)} tokens`}>
              <span className="chart-value">{formatTokens(point.tokens.total_tokens)}</span>
              <i style={{ height: `${(point.tokens.total_tokens / maxTokens) * 100}%` }} />
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
              <div className={`usage-meter ${quota.quota.projected_pct_at_reset > 100 ? "meter-watch" : ""}`} role="progressbar" aria-label="Codex weekly quota used" aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.min(100, Math.max(0, quota.quota.used_pct))}><i style={{ width: `${Math.min(100, Math.max(0, quota.quota.used_pct))}%` }} /></div>
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
          {validLimit && spentIsExact && spent !== null ? <div className={`usage-meter ${spent > limit ? "meter-watch" : ""}`} role="progressbar" aria-label="Monthly budget used" aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.min(100, Math.max(0, spent / limit * 100))}><i style={{ width: `${Math.min(100, Math.max(0, spent / limit * 100))}%` }} /></div> : null}
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
    const names = readySourcesForAggregation(sources, findings);
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
    ? "A little clarity on everything your AI is using."
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
    <div className="ledger-shell">
      <aside className="sidebar">
        <div className="brand-lockup"><span className="brand-mark" aria-hidden="true"><i /><i /><i /></span><strong>ccstats<span>.</span></strong><small>DESKTOP</small></div><div className="workspace-label"><span className="workspace-avatar">P</span><div><strong>Personal workspace</strong><small>Local usage analytics</small></div></div>
        <nav className="side-nav" aria-label="Analytics view">
          {VIEW_GROUPS.map((group) => <section className="nav-group" aria-label={group.label} key={group.label}><h2>{group.label}</h2>{group.views.map((item) => <button type="button" key={item.value} className={view === item.value ? "active" : ""} aria-label={item.label} aria-pressed={view === item.value} disabled={loading || sources.length === 0} onClick={() => void loadView(item.value, selectedSource, selectedRange)}><Icon name={item.value} /><strong>{item.label}</strong>{item.value === "live" ? <i className="nav-live-dot" /> : null}</button>)}</section>)}
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
        <div className="local-status"><Icon name="shield" /><div><strong>Your data stays yours.</strong><small>No transcript upload</small></div></div>
      </aside>

      <div className="workspace">
        <div className="workspace-topbar"><div><Icon name={view} /><span>Workspace</span><span className="breadcrumb-divider">/</span><strong>{VIEW_GROUPS.flatMap((group) => group.views).find((item) => item.value === view)?.label}</strong></div><span className="local-badge"><i />Local workspace</span></div>
        <header className="workspace-toolbar">
          <div className="page-heading"><span className="eyebrow">{view === "machines" ? "YOUR CONNECTED DEVICES" : "YOUR AI, ACCOUNTED FOR"}</span><h1>{pageTitle}</h1><p>{pageDescription}</p></div>
          <div className="toolbar-actions">
            {view !== "diagnostics" && view !== "limits" && view !== "live" && view !== "machines" ? <div className="period-control" aria-label="Usage period">
              {RANGE_OPTIONS.map((range) => <button type="button" key={range.value} aria-label={range.label} aria-pressed={selectedRange === range.value} className={selectedRange === range.value ? "active" : ""} onClick={() => changeRange(range.value)}>{range.label}</button>)}
            </div> : null}
            {view !== "machines" && view !== "activity" ? <button type="button" className="refresh-button" aria-label="Refresh ledger" onClick={() => view === "live" ? void live.refresh(false) : void loadView(view, selectedSource, selectedRange)} disabled={loading || live.refreshing || selectedSource.length === 0}><Icon name="refresh" /> Refresh</button> : null}
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
            <div data-testid="overview-content"><OverviewView overview={overview} summary={summary} sourceDescriptor={sourceDescriptor} range={selectedRange} onNavigate={(nextView) => void loadView(nextView, selectedSource, selectedRange)} /></div>
          ) : null}
        </main>
        <footer className="workspace-footer"><span><span className="footer-mark">cc.</span> A clearer picture of your AI usage.</span><span>{selectedSourceLabel} · {selectedRangeLabel}</span></footer>
      </div>
    </div>
  );
}
