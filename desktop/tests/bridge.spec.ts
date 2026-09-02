import { expect, test } from "@playwright/test";
import {
  aggregateUsageOverviews,
  type CostSummary,
  type TokenBreakdown,
  type UsageOverview,
  type UsageRange,
} from "../src/bridge";

function tokens(input: number, cacheRead: number, cacheHitRate: number | null): TokenBreakdown {
  return {
    input_tokens: input,
    output_tokens: 0,
    reasoning_tokens: 0,
    cache_creation_tokens: 0,
    cache_creation_1h_tokens: 0,
    cache_read_tokens: cacheRead,
    reported_total_adjustment: 0,
    cache_hit_rate: cacheHitRate,
    total_tokens: input + cacheRead,
  };
}

function overview(name: string, tokenCounts: TokenBreakdown): UsageOverview {
  const summary = (range: UsageRange): CostSummary => ({
    range,
    since: "2026-09-02",
    until: "2026-09-02",
    currency: "USD",
    cost: 0,
    cost_usd: 0,
    estimated_cost: null,
    estimated_cost_usd: null,
    cost_kind: "real",
    pricing_source: "recorded",
    api_equivalent_cost_coverage: null,
    tokens: { ...tokenCounts },
    models: [{
      model: "shared-model",
      cost: 0,
      cost_usd: 0,
      estimated_cost: null,
      estimated_cost_usd: null,
      cost_kind: "real",
      pricing_source: "recorded",
      tokens: { ...tokenCounts },
    }],
    valid_entries: 1,
    skipped_entries: 0,
    parse_error_entries: 0,
    elapsed_ms: 1,
  });

  return {
    source: name,
    source_name: name,
    display_name: name,
    currency: "USD",
    generated_at: "2026-09-02T00:00:00Z",
    summaries: [summary("today"), summary("this_week"), summary("this_month")],
    elapsed_ms: 3,
  };
}

test("cache aggregation excludes sources without cache-hit evidence", () => {
  const aggregate = aggregateUsageOverviews([
    overview("cache-aware", tokens(50, 50, 50)),
    overview("cache-opaque", tokens(900, 0, null)),
  ]);

  for (const summary of aggregate.summaries) {
    expect(summary.tokens.total_tokens).toBe(1_000);
    expect(summary.tokens.cache_hit_rate).toBe(50);
    expect(summary.models[0].tokens.total_tokens).toBe(1_000);
    expect(summary.models[0].tokens.cache_hit_rate).toBe(50);
  }
});
