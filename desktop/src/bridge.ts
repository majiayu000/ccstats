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

export interface TokenBreakdown {
  input_tokens: number;
  output_tokens: number;
  reasoning_tokens: number;
  cache_creation_tokens: number;
  cache_read_tokens: number;
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
  cost_kind: string;
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
}

export interface DesktopBridge {
  listSources: () => Promise<SourceDescriptor[]>;
  usageOverview: (source: string) => Promise<UsageOverview>;
}

declare global {
  interface Window {
    __CCSTATS_TEST_BRIDGE__?: DesktopBridge;
    __CCSTATS_TEST_CALLS__?: string[];
    __CCSTATS_TEST_CATALOG_CALLS__?: number;
  }
}

const tauriBridge: DesktopBridge = {
  listSources: () => invoke<SourceDescriptor[]>("list_sources"),
  usageOverview: (source) => invoke<UsageOverview>("usage_overview", { source }),
};

export function desktopBridge(): DesktopBridge {
  return window.__CCSTATS_TEST_BRIDGE__ ?? tauriBridge;
}
