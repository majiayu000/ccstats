import { hasExactCost, type CostSummary, type UsageOverview, type UsageRange } from "./bridge";
import { formatCost, formatTokens } from "./format";

const PRICING_EVIDENCE: Record<string, { label: string; detail: string }> = {
  recorded: { label: "Source-recorded", detail: "The source supplied the monetary amount. It is preserved as recorded, not described as an invoice." },
  live: { label: "Live price catalog", detail: "Token usage was priced with the current downloaded model catalog." },
  cache: { label: "Cached price catalog", detail: "Token usage was priced with a recent local copy of the model catalog." },
  cache_stale: { label: "Stale price catalog", detail: "Token usage was priced with an older local catalog. Review before treating it as current." },
  fallback: { label: "Fallback price", detail: "The exact model was absent, so ccstats used its model-family fallback price." },
  unknown: { label: "Unknown", detail: "No trustworthy monetary amount or matching price was available." },
  mixed: { label: "Mixed evidence", detail: "More than one pricing source contributed to this total." },
};

function pricingEvidence(source: string) {
  return PRICING_EVIDENCE[source] ?? {
    label: source.replaceAll("_", " "),
    detail: "This pricing source was returned by the local ccstats ledger.",
  };
}

function usageBasis(kind: string) {
  if (kind === "real") return "Observed usage";
  if (kind === "estimated_proxy") return "Proxy estimate";
  if (kind === "mixed") return "Observed + proxy estimate";
  return kind.replaceAll("_", " ");
}

function coverageLabel(summary: CostSummary) {
  const coverage = summary.api_equivalent_cost_coverage;
  if (!coverage) return "Not applicable";
  return `${coverage.percent.toFixed(1)}%`;
}

function trustStatus(summary: CostSummary) {
  if (summary.cost === null) return { label: "Unpriced", tone: "unknown-cost" };
  if (summary.api_equivalent_cost_coverage?.cost_is_lower_bound) return { label: "Lower bound", tone: "unknown-cost" };
  if (!hasExactCost(summary)) return { label: "Review", tone: "unknown-cost" };
  return { label: "Cost available", tone: "known-cost" };
}

function evidenceCost(summary: CostSummary) {
  return `${summary.api_equivalent_cost_coverage?.cost_is_lower_bound ? "≥ " : ""}${formatCost(summary.cost, summary.currency)}`;
}

export function CostTrustView({ overview, summary, range }: {
  overview: UsageOverview;
  summary: CostSummary;
  range: UsageRange;
}) {
  const evidence = pricingEvidence(summary.pricing_source);
  const status = trustStatus(summary);
  const sourceRows = overview.source_overviews
    ? overview.source_overviews.flatMap((source) => {
      const sourceSummary = source.summaries.find((candidate) => candidate.range === range);
      return sourceSummary ? [{ name: source.display_name, summary: sourceSummary }] : [];
    })
    : [{ name: overview.display_name, summary }];

  return (
    <section className="trust-workbench" aria-labelledby="trust-title">
      <header className="work-pane trust-heading">
        <div><h2 id="trust-title">Cost evidence</h2><p>Separate usage basis, pricing origin, and coverage before trusting a total.</p></div>
        <span className={status.tone}>{status.label}</span>
      </header>

      <dl className="trust-summary">
        <div><dt>Displayed cost</dt><dd data-testid="trust-displayed-cost">{evidenceCost(summary)}</dd><small>{summary.currency}</small></div>
        <div><dt>Pricing evidence</dt><dd data-testid="trust-pricing-source">{evidence.label}</dd><small>{summary.pricing_source}</small></div>
        <div><dt>Usage basis</dt><dd>{usageBasis(summary.cost_kind)}</dd><small>{summary.cost_kind}</small></div>
        <div><dt>API-equivalent coverage</dt><dd data-testid="trust-coverage">{coverageLabel(summary)}</dd><small>{summary.api_equivalent_cost_coverage?.cost_is_lower_bound ? "Displayed cost is a lower bound" : "Only shown when the source defines this boundary"}</small></div>
      </dl>

      <section className="work-pane trust-explanation" aria-label="Pricing evidence explanation">
        <span aria-hidden="true">i</span>
        <div><strong>{evidence.label}</strong><p>{evidence.detail}</p></div>
      </section>

      <section className="work-pane table-pane trust-table" aria-labelledby="source-trust-title">
        <header className="pane-heading"><div><h2 id="source-trust-title">Source evidence</h2><p>Every source contributing to this window.</p></div><span>{sourceRows.length} sources</span></header>
        <div className="table-scroll">
          <table>
            <thead><tr><th>Source</th><th>Usage basis</th><th>Pricing evidence</th><th>Coverage</th><th>Tokens</th><th>Cost</th><th>Proxy estimate</th></tr></thead>
            <tbody>
              {sourceRows.map((row) => (
                <tr key={row.name}>
                  <td className="identity-cell">{row.name}</td>
                  <td>{usageBasis(row.summary.cost_kind)}</td>
                  <td>{pricingEvidence(row.summary.pricing_source).label}</td>
                  <td>{coverageLabel(row.summary)}</td>
                  <td>{formatTokens(row.summary.tokens.total_tokens)}</td>
                  <td className={row.summary.cost === null || row.summary.api_equivalent_cost_coverage?.cost_is_lower_bound ? "unknown-cost" : "known-cost"}>{evidenceCost(row.summary)}</td>
                  <td>{row.summary.estimated_cost === null ? "—" : formatCost(row.summary.estimated_cost, row.summary.currency)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </section>

      <section className="work-pane table-pane trust-table" aria-labelledby="model-trust-title">
        <header className="pane-heading"><div><h2 id="model-trust-title">Model evidence</h2><p>Pricing origin and usage basis for each model.</p></div><span>{summary.models.length} models</span></header>
        {summary.models.length === 0 ? <div className="evidence-note"><strong>No model cost evidence in this window.</strong><p>Choose a wider period or another source.</p></div> : (
          <div className="table-scroll">
            <table>
              <thead><tr><th>Model</th><th>Usage basis</th><th>Pricing evidence</th><th>Tokens</th><th>Cost</th><th>Proxy estimate</th></tr></thead>
              <tbody>
                {summary.models.map((model) => (
                  <tr key={model.model}>
                    <td className="identity-cell">{model.model}</td>
                    <td>{usageBasis(model.cost_kind)}</td>
                    <td>{pricingEvidence(model.pricing_source).label}</td>
                    <td>{formatTokens(model.tokens.total_tokens)}</td>
                    <td className={hasExactCost(summary) && hasExactCost(model) ? "known-cost" : "unknown-cost"}>{summary.api_equivalent_cost_coverage?.cost_is_lower_bound ? "≥ " : model.cost !== null && (!hasExactCost(summary) || !hasExactCost(model)) ? "≈ " : ""}{formatCost(model.cost, summary.currency)}</td>
                    <td>{model.estimated_cost === null ? "—" : formatCost(model.estimated_cost, summary.currency)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </section>
    </section>
  );
}
