import { invoke } from "@tauri-apps/api/core";

export type UsageRange = "today" | "this_week" | "this_month";

export interface SourceDescriptor {
  source: string;
  name: string;
  display_name: string;
  aliases: string[];
  has_projects: boolean;
  has_reasoning_tokens: boolean;
  has_cache_creation: boolean;
  has_cache_read: boolean;
}

export type SourceDiagnosticStatus = "detected" | "configured" | "missing" | "error";

export interface SourceDiagnosticDescriptor {
  source: string;
  name: string;
  display_name: string;
  status: SourceDiagnosticStatus;
  files: number;
  detail: string;
  setup: string;
}

export function readySourcesForAggregation(
  sources: SourceDescriptor[],
  findings: SourceDiagnosticDescriptor[],
): string[] {
  const errors = findings.filter((row) => row.status === "error");
  if (errors.length > 0) {
    throw new Error(errors.map((row) => `${row.display_name}: ${row.detail}`).join("\n"));
  }
  const ready = new Set(findings.filter((row) => row.status === "detected" || row.status === "configured").map((row) => row.name));
  return sources.filter((source) => source.name !== "all" && ready.has(source.name)).map((source) => source.name);
}

export interface CodexWeeklyQuota {
  observed_at: string;
  resets_at: string;
  estimated_depletion_at: string | null;
  window_minutes: number;
  used_pct: number;
  remaining_pct: number;
  projected_pct_at_reset: number;
  status: "on_track" | "watch" | "likely_exhausted" | "exhausted";
}

export interface CodexWeeklyValueEstimate {
  observed_at: string;
  window_started_at: string;
  resets_at: string;
  used_pct: number;
  observed_cost_usd: number;
  estimated_weekly_value_usd: number;
  observed_tokens: number;
  estimated_weekly_tokens: number;
  valid_entries: number;
  dedup_skipped_entries: number;
}

export interface CodexQuotaOverview {
  quota: CodexWeeklyQuota;
  value_estimate: CodexWeeklyValueEstimate | null;
  value_estimate_error: string | null;
}

export interface TokenBreakdown {
  input_tokens: number;
  output_tokens: number;
  reasoning_tokens: number;
  cache_creation_tokens: number;
  cache_creation_1h_tokens: number;
  cache_read_tokens: number;
  reported_total_adjustment: number;
  cache_hit_rate: number | null;
  total_tokens: number;
}

export interface ModelCostSummary {
  model: string;
  cost: number | null;
  cost_usd: number | null;
  estimated_cost: number | null;
  estimated_cost_usd: number | null;
  cost_kind: string;
  pricing_source: string;
  tokens: TokenBreakdown;
}

export interface ApiEquivalentCostCoverage {
  total_tokens: number;
  priced_tokens: number;
  percent: number;
  complete: boolean;
  cost_is_lower_bound: boolean;
}

export interface CostSummary {
  range: UsageRange;
  since: string | null;
  until: string | null;
  currency: string;
  cost: number | null;
  cost_usd: number | null;
  estimated_cost: number | null;
  estimated_cost_usd: number | null;
  cost_kind: string;
  pricing_source: string;
  api_equivalent_cost_coverage: ApiEquivalentCostCoverage | null;
  tokens: TokenBreakdown;
  models: ModelCostSummary[];
  valid_entries: number;
  skipped_entries: number;
  parse_error_entries: number;
  elapsed_ms: number;
}

export interface UsageOverview {
  source: string;
  source_name: string;
  display_name: string;
  currency: string;
  generated_at: string;
  summaries: CostSummary[];
  elapsed_ms: number;
  source_overviews?: UsageOverview[];
}

export interface AnalyticsQuality {
  valid_entries: number;
  dedup_skipped_entries: number;
  parse_error_entries: number;
}

export interface UsageMetrics {
  currency: string;
  cost: number | null;
  cost_usd: number | null;
  estimated_cost: number | null;
  estimated_cost_usd: number | null;
  cost_kind: string;
  pricing_source: string;
  api_equivalent_cost_coverage: ApiEquivalentCostCoverage | null;
  tokens: TokenBreakdown;
  models: ModelCostSummary[];
}

export interface SessionTitle {
  text: string;
  origin: "source_title" | "source_summary";
}

export interface SessionDrilldown {
  session_id: string;
  project_path: string;
  first_timestamp: string;
  last_timestamp: string;
  metrics: UsageMetrics;
}

export interface ProjectDrilldown {
  project_path: string;
  project_name: string;
  session_count: number;
  metrics: UsageMetrics;
  sessions: SessionDrilldown[];
}

export interface ProjectDrilldownSummary {
  source: string;
  source_name: string;
  display_name: string;
  range: UsageRange;
  currency: string;
  quality: AnalyticsQuality;
  projects: ProjectDrilldown[];
  session_titles: Record<string, SessionTitle>;
  session_titles_error: string | null;
}

export interface DailyUsagePoint {
  date: string;
  currency: string;
  tokens: TokenBreakdown;
  records: number;
  cost: number | null;
  cost_usd: number | null;
  cost_status: "known" | "partial" | "unknown";
  cost_kind: string;
  pricing_source: string;
  api_equivalent_cost_coverage: ApiEquivalentCostCoverage | null;
}

export interface UsageHistory {
  source: string;
  source_name: string;
  display_name: string;
  range: UsageRange;
  as_of_date: string;
  currency: string;
  points: DailyUsagePoint[];
  quality: AnalyticsQuality;
}

export type CostEvidence = Pick<CostSummary, "cost" | "cost_kind" | "pricing_source"> & { api_equivalent_cost_coverage?: ApiEquivalentCostCoverage | null };

export function hasExactCost(evidence: CostEvidence) {
  return evidence.cost !== null
    && evidence.cost_kind === "real"
    && ["recorded", "live", "cache"].includes(evidence.pricing_source)
    && evidence.api_equivalent_cost_coverage?.cost_is_lower_bound !== true;
}

export interface ToolUsage {
  name: string;
  calls: number;
}

export interface ModelTurnUsage {
  timestamp: string;
  session_id: string;
  message_id: string | null;
  project_path: string;
  model: string;
  model_call_count: number;
  tokens: TokenBreakdown;
}

export interface TurnToolBreakdown {
  source: string;
  source_name: string;
  display_name: string;
  range: UsageRange;
  total_turns: number;
  turns: ModelTurnUsage[];
  tool_calls_supported: boolean;
  tool_calls_total: number;
  tools: ToolUsage[];
  quality: AnalyticsQuality;
}

export interface ConsumerInput {
  name: string;
  tokens: number;
  cost: number | null;
}

export interface RankedConsumer extends ConsumerInput {
  rank: number;
  share: number;
  share_basis: "cost" | "tokens";
}

export interface UsageAnomaly {
  status: "spike" | "normal" | "insufficient";
  date: string | null;
  tokens: number | null;
  baseline_tokens: number | null;
  change_pct: number | null;
  sample_days: number;
}

export interface MachineUsageTotals {
  today_tokens: number;
  week_tokens: number;
  month_tokens: number;
  today_cost: number | null;
  week_cost: number | null;
  month_cost: number | null;
}

export interface MachineUsage {
  machine_id: string;
  machine_name: string;
  captured_at_ms: number;
  source_count: number;
  currency: string | null;
  is_local: boolean;
  today_current: boolean;
  week_current: boolean;
  month_current: boolean;
  totals: MachineUsageTotals;
}

export interface MachineRollup {
  local_machine_id: string;
  local_machine_name: string | null;
  currency: string | null;
  today_current_machines: number;
  week_current_machines: number;
  month_current_machines: number;
  machines: MachineUsage[];
  totals: MachineUsageTotals;
}

export function rankConsumers(rows: ConsumerInput[], limit = 5): RankedConsumer[] {
  const shareBasis = rows.length > 0 && rows.every((row) => row.cost !== null && row.cost >= 0) ? "cost" : "tokens";
  const total = rows.reduce((sum, row) => sum + (shareBasis === "cost" ? row.cost ?? 0 : row.tokens), 0);
  return [...rows]
    .sort((left, right) => {
      if (shareBasis === "cost") {
        return (right.cost ?? 0) - (left.cost ?? 0)
          || right.tokens - left.tokens
          || left.name.localeCompare(right.name);
      }
      return right.tokens - left.tokens || left.name.localeCompare(right.name);
    })
    .slice(0, Math.max(limit, 0))
    .map((row, index) => ({
      ...row,
      rank: index + 1,
      share: total > 0 ? (shareBasis === "cost" ? row.cost ?? 0 : row.tokens) / total * 100 : 0,
      share_basis: shareBasis,
    }));
}

export function detectUsageAnomaly(history: UsageHistory | null): UsageAnomaly {
  if (!history) return { status: "insufficient", date: null, tokens: null, baseline_tokens: null, change_pct: null, sample_days: 0 };
  const complete = [...history.points]
    .filter((point) => point.date < history.as_of_date && point.tokens.total_tokens > 0)
    .sort((left, right) => left.date.localeCompare(right.date));
  const latest = complete.at(-1);
  const baselineDays = complete.slice(0, -1).slice(-7);
  if (!latest || baselineDays.length < 3) {
    return { status: "insufficient", date: latest?.date ?? null, tokens: latest?.tokens.total_tokens ?? null, baseline_tokens: null, change_pct: null, sample_days: baselineDays.length };
  }
  const baseline = baselineDays.reduce((sum, point) => sum + point.tokens.total_tokens, 0) / baselineDays.length;
  if (baseline <= 0) return { status: "insufficient", date: latest.date, tokens: latest.tokens.total_tokens, baseline_tokens: null, change_pct: null, sample_days: baselineDays.length };
  const change = (latest.tokens.total_tokens - baseline) / baseline * 100;
  return { status: change >= 100 ? "spike" : "normal", date: latest.date, tokens: latest.tokens.total_tokens, baseline_tokens: baseline, change_pct: change, sample_days: baselineDays.length };
}

function addTokens(target: TokenBreakdown, source: TokenBreakdown) {
  target.input_tokens += source.input_tokens;
  target.output_tokens += source.output_tokens;
  target.reasoning_tokens += source.reasoning_tokens;
  target.cache_creation_tokens += source.cache_creation_tokens;
  target.cache_creation_1h_tokens += source.cache_creation_1h_tokens;
  target.cache_read_tokens += source.cache_read_tokens;
  target.reported_total_adjustment = (target.reported_total_adjustment ?? 0)
    + (source.reported_total_adjustment ?? 0);
  target.total_tokens += source.total_tokens;
}

function sumCost(
  summaries: ReadonlyArray<{ cost: number | null; tokens: TokenBreakdown }>,
) {
  let total = 0;
  for (const summary of summaries) {
    if (summary.cost === null) {
      if (summary.tokens.total_tokens > 0) return null;
      continue;
    }
    total += summary.cost;
  }
  return total;
}

function sumOptionalAmounts(values: ReadonlyArray<number | null>) {
  const known = values.filter((value): value is number => value !== null);
  return known.length === 0 ? null : known.reduce((sum, value) => sum + value, 0);
}

function aggregateCoverage(summaries: CostSummary[]): ApiEquivalentCostCoverage | null {
  const rows = summaries.flatMap((summary) => summary.api_equivalent_cost_coverage ? [summary.api_equivalent_cost_coverage] : []);
  if (rows.length === 0) return null;
  const totalTokens = rows.reduce((sum, row) => sum + row.total_tokens, 0);
  const pricedTokens = rows.reduce((sum, row) => sum + row.priced_tokens, 0);
  return {
    total_tokens: totalTokens,
    priced_tokens: pricedTokens,
    percent: totalTokens > 0 ? Math.min(Math.max(pricedTokens, 0), totalTokens) / totalTokens * 100 : 0,
    complete: rows.every((row) => row.complete),
    cost_is_lower_bound: rows.some((row) => row.cost_is_lower_bound),
  };
}

function aggregateCacheHitRate(rows: ReadonlyArray<TokenBreakdown>): number | null {
  const supported = rows.filter((row) => row.cache_hit_rate !== null);
  const inputSide = supported.reduce(
    (sum, row) => sum + row.input_tokens + row.cache_creation_tokens + row.cache_read_tokens,
    0,
  );
  if (inputSide === 0) return null;
  return supported.reduce((sum, row) => sum + row.cache_read_tokens, 0) / inputSide * 100;
}

function aggregateModels(summaries: CostSummary[]): ModelCostSummary[] {
  const models = new Map<string, ModelCostSummary[]>();
  for (const summary of summaries) {
    for (const model of summary.models) {
      const rows = models.get(model.model) ?? [];
      rows.push(model);
      models.set(model.model, rows);
    }
  }

  return [...models.entries()]
    .map(([model, rows]) => {
      const tokens: TokenBreakdown = {
        input_tokens: 0,
        output_tokens: 0,
        reasoning_tokens: 0,
        cache_creation_tokens: 0,
        cache_creation_1h_tokens: 0,
        cache_read_tokens: 0,
        reported_total_adjustment: 0,
        cache_hit_rate: null,
        total_tokens: 0,
      };
      for (const row of rows) addTokens(tokens, row.tokens);
      tokens.cache_hit_rate = aggregateCacheHitRate(rows.map((row) => row.tokens));
      const kinds = new Set(rows.map((row) => row.cost_kind));
      return {
        model,
        cost: sumCost(rows),
        cost_usd: sumCost(rows.map((row) => ({ cost: row.cost_usd, tokens: row.tokens }))),
        estimated_cost: sumOptionalAmounts(rows.map((row) => row.estimated_cost)),
        estimated_cost_usd: sumOptionalAmounts(rows.map((row) => row.estimated_cost_usd)),
        cost_kind: kinds.size === 1 ? rows[0].cost_kind : "mixed",
        pricing_source: new Set(rows.map((row) => row.pricing_source)).size === 1 ? rows[0].pricing_source : "mixed",
        tokens,
      };
    })
    .sort((left, right) => right.tokens.total_tokens - left.tokens.total_tokens || left.model.localeCompare(right.model));
}

export function aggregateUsageOverviews(overviews: UsageOverview[]): UsageOverview {
  if (overviews.length === 0) throw new Error("All Sources requires at least one registered source");
  const currencies = new Set(overviews.map((overview) => overview.currency));
  if (currencies.size !== 1) throw new Error("All Sources received inconsistent currencies");
  const ranges: UsageRange[] = ["today", "this_week", "this_month"];

  const summaries = ranges.map((range) => {
    const sourceSummaries = overviews.map((overview) => {
      const summary = overview.summaries.find((candidate) => candidate.range === range);
      if (!summary) throw new Error(`${overview.display_name} is missing ${range}`);
      return summary;
    });
    const tokens: TokenBreakdown = {
      input_tokens: 0,
      output_tokens: 0,
      reasoning_tokens: 0,
      cache_creation_tokens: 0,
      cache_creation_1h_tokens: 0,
      cache_read_tokens: 0,
      reported_total_adjustment: 0,
      cache_hit_rate: null,
      total_tokens: 0,
    };
    for (const summary of sourceSummaries) addTokens(tokens, summary.tokens);
    tokens.cache_hit_rate = aggregateCacheHitRate(sourceSummaries.map((summary) => summary.tokens));
    const evidenceSummaries = sourceSummaries.filter((summary) =>
      summary.valid_entries > 0
      || summary.models.length > 0
      || summary.api_equivalent_cost_coverage !== null,
    );
    return {
      range,
      since: sourceSummaries[0].since,
      until: sourceSummaries[0].until,
      currency: overviews[0].currency,
      cost: sumCost(sourceSummaries),
      cost_usd: sumCost(sourceSummaries.map((summary) => ({ cost: summary.cost_usd, tokens: summary.tokens }))),
      estimated_cost: sumOptionalAmounts(sourceSummaries.map((summary) => summary.estimated_cost)),
      estimated_cost_usd: sumOptionalAmounts(sourceSummaries.map((summary) => summary.estimated_cost_usd)),
      cost_kind: evidenceSummaries.length === 0
        ? "unknown"
        : new Set(evidenceSummaries.map((summary) => summary.cost_kind)).size === 1
          ? evidenceSummaries[0].cost_kind
          : "mixed",
      pricing_source: evidenceSummaries.length === 0
        ? "unknown"
        : new Set(evidenceSummaries.map((summary) => summary.pricing_source)).size === 1
          ? evidenceSummaries[0].pricing_source
          : "mixed",
      api_equivalent_cost_coverage: aggregateCoverage(sourceSummaries),
      tokens,
      models: aggregateModels(sourceSummaries),
      valid_entries: sourceSummaries.reduce((sum, summary) => sum + summary.valid_entries, 0),
      skipped_entries: sourceSummaries.reduce((sum, summary) => sum + summary.skipped_entries, 0),
      parse_error_entries: sourceSummaries.reduce((sum, summary) => sum + summary.parse_error_entries, 0),
      elapsed_ms: sourceSummaries.reduce((sum, summary) => sum + summary.elapsed_ms, 0),
    } satisfies CostSummary;
  });

  return {
    source: "all",
    source_name: "all",
    display_name: "All Sources",
    currency: overviews[0].currency,
    generated_at: overviews.map((overview) => overview.generated_at).sort().at(-1) ?? overviews[0].generated_at,
    summaries,
    elapsed_ms: overviews.reduce((sum, overview) => sum + overview.elapsed_ms, 0),
    source_overviews: overviews,
  };
}

export interface DesktopBridge {
  listSources: () => Promise<SourceDescriptor[]>;
  sourceDiagnostics: () => Promise<SourceDiagnosticDescriptor[]>;
  codexQuotaOverview: () => Promise<CodexQuotaOverview>;
  usageOverview: (source: string) => Promise<UsageOverview>;
  usageOverviews: (sources: string[]) => Promise<UsageOverview[]>;
  projectDrilldown: (source: string, range: UsageRange) => Promise<ProjectDrilldownSummary>;
  usageHistory: (source: string, range: UsageRange) => Promise<UsageHistory>;
  turnToolBreakdown: (source: string, range: UsageRange) => Promise<TurnToolBreakdown>;
  exportHistory: (source: string, range: UsageRange, format: "csv" | "json") => Promise<string>;
  machineRollup: () => Promise<MachineRollup>;
  saveMachineSnapshot: (machineName: string, sources: UsageOverview[]) => Promise<MachineRollup>;
  exportMachineBundle: () => Promise<string>;
  importMachineBundle: (content: string) => Promise<MachineRollup>;
}

declare global {
  interface Window {
    __CCSTATS_TEST_BRIDGE__?: DesktopBridge;
    __CCSTATS_TEST_CALLS__?: string[];
    __CCSTATS_TEST_CATALOG_CALLS__?: number;
    __CCSTATS_TEST_EXPORT_CALLS__?: string[];
  }
}

const tauriBridge: DesktopBridge = {
  listSources: () => invoke<SourceDescriptor[]>("list_sources"),
  sourceDiagnostics: () => invoke<SourceDiagnosticDescriptor[]>("source_diagnostics"),
  codexQuotaOverview: () => invoke<CodexQuotaOverview>("codex_quota_overview"),
  usageOverview: (source) => invoke<UsageOverview>("usage_overview", { source }),
  usageOverviews: (sources) => invoke<UsageOverview[]>("usage_overviews", { sources }),
  projectDrilldown: (source, range) => invoke<ProjectDrilldownSummary>("project_drilldown", { source, range }),
  usageHistory: (source, range) => invoke<UsageHistory>("usage_history", { source, range }),
  turnToolBreakdown: (source, range) => invoke<TurnToolBreakdown>("turn_tool_breakdown", { source, range }),
  exportHistory: (source, range, format) => invoke<string>("export_history", { source, range, format }),
  machineRollup: () => invoke<MachineRollup>("machine_rollup"),
  saveMachineSnapshot: (machineName, sources) => invoke<MachineRollup>("save_machine_snapshot", { machineName, sources }),
  exportMachineBundle: () => invoke<string>("export_machine_bundle"),
  importMachineBundle: (content) => invoke<MachineRollup>("import_machine_bundle", { content }),
};

export function desktopBridge(): DesktopBridge {
  return window.__CCSTATS_TEST_BRIDGE__ ?? tauriBridge;
}
