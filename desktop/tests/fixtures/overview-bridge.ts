import type { Page } from "@playwright/test";
import type { ProjectDrilldownSummary, UsageOverview } from "../../src/bridge";

type MockOptions = {
  delay?: number;
  qualityWarning?: boolean;
  fail?: boolean;
  omitWeek?: boolean;
  allMalformed?: boolean;
  failCatalogOnce?: boolean;
  quotaFail?: boolean;
  liveGrowth?: boolean;
  liveFailAfterGrowth?: boolean;
  noReadySources?: boolean;
  totalAdjustment?: boolean;
  titleMode?: "missing" | "error";
};

export async function injectBridge(page: Page, options: MockOptions = {}) {
  await page.addInitScript(({ delay, qualityWarning, fail, omitWeek, allMalformed, failCatalogOnce, quotaFail, liveGrowth, liveFailAfterGrowth, noReadySources, totalAdjustment, titleMode }) => {
    const calls: string[] = [];
    const exportCalls: string[] = [];
    let catalogCalls = 0;
    type MockMachine = { machine_id: string; machine_name: string; captured_at_ms: number; source_count: number; currency: string | null; is_local: boolean; today_current: boolean; week_current: boolean; month_current: boolean; totals: { today_tokens: number; week_tokens: number; month_tokens: number; today_cost: number | null; week_cost: number | null; month_cost: number | null } };
    type MockRollup = { local_machine_id: string; local_machine_name: string | null; currency: string | null; today_current_machines: number; week_current_machines: number; month_current_machines: number; machines: MockMachine[]; totals: MockMachine["totals"] };
    type MockStoredMachine = {
      machine_id: string;
      machine_name: string;
      captured_at_ms: number;
      windows: { range: string; since: string; until: string; since_utc_ms: number; until_exclusive_utc_ms: number }[];
      sources: UsageOverview[];
    };
    let storedMachine: MockStoredMachine | undefined;
    let machineState: MockRollup = { local_machine_id: "local-1", local_machine_name: null, currency: null, today_current_machines: 0, week_current_machines: 0, month_current_machines: 0, machines: [], totals: { today_tokens: 0, week_tokens: 0, month_tokens: 0, today_cost: 0, week_cost: 0, month_cost: 0 } };
    const summaries = [
      { range: "today", total: 1_240, records: 4 },
      { range: "this_week", total: 8_420, records: 18 },
      { range: "this_month", total: 24_900, records: 51 },
    ] as const;

    window.__CCSTATS_TEST_BRIDGE__ = {
      listSources: async () => {
        catalogCalls += 1;
        window.__CCSTATS_TEST_CATALOG_CALLS__ = catalogCalls;
        if (failCatalogOnce && catalogCalls === 1) throw new Error("catalog is unavailable");
        return [
          {
            source: "claude",
            name: "claude",
            display_name: "Claude Code",
            aliases: ["cc"],
            has_projects: true,
            has_reasoning_tokens: false,
            has_cache_creation: true,
            has_cache_read: true,
          },
          {
            source: "codex",
            name: "codex",
            display_name: "OpenAI Codex",
            aliases: ["cx"],
            has_projects: false,
            has_reasoning_tokens: true,
            has_cache_creation: false,
            has_cache_read: true,
          },
        ];
      },
      sourceDiagnostics: async () => [
        {
          source: "claude",
          name: "claude",
          display_name: "Claude Code",
          status: noReadySources ? "missing" : "detected",
          files: noReadySources ? 0 : 12,
          detail: noReadySources ? "No local usage files found" : "Found 12 local usage files",
          setup: "Run Claude Code once",
        },
        {
          source: "codex",
          name: "codex",
          display_name: "OpenAI Codex",
          status: "missing",
          files: 0,
          detail: "No local usage files found",
          setup: "Run Codex once",
        },
      ],
      codexQuotaOverview: async () => {
        if (quotaFail) throw new Error("No current Codex weekly quota snapshot");
        return {
          quota: {
            observed_at: "2026-09-01T02:00:00Z",
            resets_at: "2026-09-07T02:00:00Z",
            estimated_depletion_at: "2026-09-05T02:00:00Z",
            window_minutes: 10_080,
            used_pct: 25,
            remaining_pct: 75,
            projected_pct_at_reset: 140,
            status: "likely_exhausted",
          },
          value_estimate: {
            observed_at: "2026-09-01T02:00:00Z",
            window_started_at: "2026-08-31T02:00:00Z",
            resets_at: "2026-09-07T02:00:00Z",
            used_pct: 25,
            observed_cost_usd: 10,
            estimated_weekly_value_usd: 40,
            observed_tokens: 1_100_000,
            estimated_weekly_tokens: 4_400_000,
            valid_entries: 4,
            dedup_skipped_entries: 1,
          },
          value_estimate_error: null,
        };
      },
      usageOverview: async (source: string) => {
        calls.push(source);
        if (fail) throw new Error("ledger is unavailable");
        if (liveFailAfterGrowth && calls.length >= 4) throw new Error("live scan is unavailable");
        if (delay) await new Promise((resolve) => setTimeout(resolve, delay));
        const liveStep = liveGrowth ? Math.max(calls.length - 2, 0) : 0;
        return {
          source,
          source_name: source,
          display_name: source === "codex" ? "OpenAI Codex" : "Claude Code",
          currency: "USD",
          generated_at: "2026-08-31T08:00:00Z",
          elapsed_ms: 12.4,
          summaries: summaries
            .filter(({ range }) => !omitWeek || range !== "this_week")
            .map(({ range, total, records }, index) => {
            const currentTotal = range === "today" ? total + liveStep * 250 : total;
            return {
            source,
            source_name: source,
            display_name: source === "codex" ? "OpenAI Codex" : "Claude Code",
            range,
            since: "2026-08-01",
            until: "2026-08-31",
            currency: "USD",
            cost: index === 0 ? liveGrowth ? 2 + liveStep * 0.5 : null : index === 1 ? 7.42 : 18.9,
            cost_usd: index === 0 ? liveGrowth ? 2 + liveStep * 0.5 : null : index === 1 ? 7.42 : 18.9,
            estimated_cost: null,
            estimated_cost_usd: null,
            cost_kind: "real",
            pricing_source: index === 0 ? liveGrowth ? "recorded" : "unknown" : index === 1 ? "cache_stale" : "recorded",
            api_equivalent_cost_coverage: index === 1 ? {
              total_tokens: 8_420,
              priced_tokens: 6_315,
              percent: 75,
              complete: false,
              cost_is_lower_bound: true,
            } : null,
            tokens: {
              input_tokens: allMalformed ? 0 : currentTotal - (totalAdjustment ? 400 : 300),
              output_tokens: allMalformed ? 0 : 180,
              reasoning_tokens: allMalformed ? 0 : 20,
              cache_creation_tokens: 0,
              cache_creation_1h_tokens: 0,
              cache_read_tokens: allMalformed ? 0 : 100,
              reported_total_adjustment: allMalformed ? 0 : totalAdjustment ? 100 : 0,
              cache_hit_rate: allMalformed ? null : 12.5,
              total_tokens: allMalformed ? 0 : currentTotal,
            },
            models: [
              {
                model: source === "codex" ? "gpt-5" : "claude-sonnet-4-6",
                cost: index === 0 ? null : 7.42,
                cost_usd: index === 0 ? null : 7.42,
                estimated_cost: null,
                estimated_cost_usd: null,
                cost_kind: "real",
                pricing_source: index === 0 ? "unknown" : index === 1 ? "cache_stale" : "recorded",
                tokens: {
                  input_tokens: currentTotal - 300,
                  output_tokens: 180,
                  reasoning_tokens: 20,
                  cache_creation_tokens: 0,
                  cache_creation_1h_tokens: 0,
                  cache_read_tokens: 100,
                  reported_total_adjustment: 0,
                  cache_hit_rate: 12.5,
                  total_tokens: currentTotal,
                },
              },
            ],
            valid_entries: allMalformed ? 0 : records,
            skipped_entries: qualityWarning ? 2 : 0,
            parse_error_entries: allMalformed ? 3 : qualityWarning ? 1 : 0,
            elapsed_ms: 12.4,
            };
            }),
        };
      },
      usageOverviews: async (sources: string[]) => Promise.all(
        sources.map((source) => window.__CCSTATS_TEST_BRIDGE__!.usageOverview(source)),
      ),
      projectDrilldown: async (source, range): Promise<ProjectDrilldownSummary> => ({
        source,
        source_name: source,
        display_name: "Claude Code",
        range,
        currency: "USD",
        quality: { valid_entries: 4, dedup_skipped_entries: 0, parse_error_entries: 0 },
        session_titles: titleMode ? {} : { "session-abc123": { text: "完善用量聚合", origin: "source_summary" } },
        session_titles_error: titleMode === "error" ? "Malformed session title index at line 2" : null,
        projects: [{
          project_path: "/work/ccstats",
          project_name: "ccstats",
          session_count: 1,
          metrics: {
            currency: "USD",
            cost: 2.4,
            cost_usd: 2.4,
            estimated_cost: null,
            estimated_cost_usd: null,
            cost_kind: "real",
            pricing_source: "recorded",
            api_equivalent_cost_coverage: null,
            tokens: { input_tokens: 900, output_tokens: 180, reasoning_tokens: 20, cache_creation_tokens: 0, cache_creation_1h_tokens: 0, cache_read_tokens: 100, reported_total_adjustment: 0, cache_hit_rate: 10, total_tokens: 1_200 },
            models: [],
          },
          sessions: [{
            session_id: "session-abc123",
            project_path: "/work/ccstats",
            first_timestamp: "2026-08-31T06:00:00Z",
            last_timestamp: "2026-08-31T08:00:00Z",
            metrics: {
              currency: "USD",
              cost: 2.4,
              cost_usd: 2.4,
              estimated_cost: null,
              estimated_cost_usd: null,
              cost_kind: "real",
              pricing_source: "recorded",
              api_equivalent_cost_coverage: null,
              tokens: { input_tokens: 900, output_tokens: 180, reasoning_tokens: 20, cache_creation_tokens: 0, cache_creation_1h_tokens: 0, cache_read_tokens: 100, reported_total_adjustment: 0, cache_hit_rate: 10, total_tokens: 1_200 },
              models: [],
            },
          }],
        }],
      }),
      usageHistory: async (source, range) => ({
        source,
        source_name: source,
        display_name: "Claude Code",
        range,
        as_of_date: "2026-09-02",
        currency: "USD",
        quality: { valid_entries: 4, dedup_skipped_entries: 0, parse_error_entries: 0 },
        points: [
          { date: "2026-08-27", currency: "USD", tokens: { input_tokens: 80, output_tokens: 20, reasoning_tokens: 0, cache_creation_tokens: 0, cache_creation_1h_tokens: 0, cache_read_tokens: 0, reported_total_adjustment: 0, cache_hit_rate: 0, total_tokens: 100 }, records: 1, cost: 0.2, cost_usd: 0.2, cost_status: "known", cost_kind: "real", pricing_source: "recorded", api_equivalent_cost_coverage: null },
          { date: "2026-08-28", currency: "USD", tokens: { input_tokens: 90, output_tokens: 30, reasoning_tokens: 0, cache_creation_tokens: 0, cache_creation_1h_tokens: 0, cache_read_tokens: 0, reported_total_adjustment: 0, cache_hit_rate: 0, total_tokens: 120 }, records: 1, cost: 0.25, cost_usd: 0.25, cost_status: "known", cost_kind: "real", pricing_source: "recorded", api_equivalent_cost_coverage: null },
          { date: "2026-08-29", currency: "USD", tokens: { input_tokens: 85, output_tokens: 25, reasoning_tokens: 0, cache_creation_tokens: 0, cache_creation_1h_tokens: 0, cache_read_tokens: 0, reported_total_adjustment: 0, cache_hit_rate: 0, total_tokens: 110 }, records: 1, cost: 0.22, cost_usd: 0.22, cost_status: "known", cost_kind: "real", pricing_source: "recorded", api_equivalent_cost_coverage: null },
          { date: "2026-08-30", currency: "USD", tokens: { input_tokens: 300, output_tokens: 100, reasoning_tokens: 0, cache_creation_tokens: 0, cache_creation_1h_tokens: 0, cache_read_tokens: 0, reported_total_adjustment: 0, cache_hit_rate: 0, total_tokens: 400 }, records: 1, cost: null, cost_usd: null, cost_status: "unknown", cost_kind: "real", pricing_source: "unknown", api_equivalent_cost_coverage: null },
          { date: "2026-08-31", currency: "USD", tokens: { input_tokens: 600, output_tokens: 180, reasoning_tokens: 20, cache_creation_tokens: 0, cache_creation_1h_tokens: 0, cache_read_tokens: 100, reported_total_adjustment: 0, cache_hit_rate: 12.5, total_tokens: 900 }, records: 3, cost: 2.4, cost_usd: 2.4, cost_status: "known", cost_kind: "real", pricing_source: "recorded", api_equivalent_cost_coverage: null },
        ],
      }),
      turnToolBreakdown: async (source, range) => ({
        source,
        source_name: source,
        display_name: source === "claude" ? "Claude Code" : "OpenAI Codex",
        range,
        total_turns: 2,
        turns: [
          {
            timestamp: "2026-09-02T08:00:00Z",
            session_id: "session-abc123",
            message_id: "message-2",
            project_path: "/work/ccstats",
            model: "claude-sonnet-4-5",
            model_call_count: 1,
            tokens: { input_tokens: 400, output_tokens: 120, reasoning_tokens: 0, cache_creation_tokens: 0, cache_creation_1h_tokens: 0, cache_read_tokens: 80, reported_total_adjustment: 0, cache_hit_rate: 16.7, total_tokens: 600 },
          },
          {
            timestamp: "2026-09-02T07:00:00Z",
            session_id: "session-abc123",
            message_id: "message-1",
            project_path: "/work/ccstats",
            model: "claude-sonnet-4-5",
            model_call_count: 1,
            tokens: { input_tokens: 420, output_tokens: 160, reasoning_tokens: 20, cache_creation_tokens: 0, cache_creation_1h_tokens: 0, cache_read_tokens: 0, reported_total_adjustment: 0, cache_hit_rate: 0, total_tokens: 600 },
          },
        ],
        tool_calls_supported: source === "claude",
        tool_calls_total: source === "claude" ? 5 : 0,
        tools: source === "claude" ? [{ name: "Read", calls: 3 }, { name: "Bash", calls: 2 }] : [],
        quality: { valid_entries: 2, dedup_skipped_entries: 1, parse_error_entries: 0 },
      }),
      exportHistory: async (source, range, format) => {
        exportCalls.push(`${source}:${range}:${format}`);
        return format === "csv" ? "date,total_tokens\r\n2026-08-31,900\r\n" : "{\"points\":[]}";
      },
      machineRollup: async () => machineState,
      saveMachineSnapshot: async (machineName, machineSources) => {
        const totalsFor = (range: string) => ({
          tokens: machineSources.reduce((sum, source) => sum + (source.summaries.find((summary) => summary.range === range)?.tokens.total_tokens ?? 0), 0),
          cost: machineSources.reduce<number | null>((sum, source) => { const cost = source.summaries.find((summary) => summary.range === range)?.cost; return sum === null || cost === null || cost === undefined ? null : sum + cost; }, 0),
        });
        const today = totalsFor("today"); const week = totalsFor("this_week"); const month = totalsFor("this_month");
        const totals = { today_tokens: today.tokens, week_tokens: week.tokens, month_tokens: month.tokens, today_cost: today.cost, week_cost: week.cost, month_cost: month.cost };
        machineState = { local_machine_id: "local-1", local_machine_name: machineName, currency: "USD", today_current_machines: 1, week_current_machines: 1, month_current_machines: 1, totals, machines: [{ machine_id: "local-1", machine_name: machineName, captured_at_ms: 1_788_316_800_000, source_count: machineSources.length, currency: "USD", is_local: true, today_current: true, week_current: true, month_current: true, totals }] };
        storedMachine = {
          machine_id: "local-1",
          machine_name: machineName,
          captured_at_ms: 1_788_316_800_000,
          windows: summaries.map(({ range }) => ({
            range,
            since: "2026-08-01",
            until: "2026-08-31",
            since_utc_ms: Date.parse("2026-08-01T00:00:00Z"),
            until_exclusive_utc_ms: Date.parse("2026-09-01T00:00:00Z"),
          })),
          sources: machineSources,
        };
        return machineState;
      },
      exportMachineBundle: async () => JSON.stringify({ schema_version: 2, machines: storedMachine ? [storedMachine] : [] }),
      importMachineBundle: async (content) => {
        const { schema_version, machines } = JSON.parse(content) as { schema_version: number; machines: MockStoredMachine[] };
        if (schema_version !== 2) throw new Error(`unsupported machine snapshot bundle version ${schema_version}`);
        const [remote] = machines;
        const remoteTotals = { today_tokens: 1_240, week_tokens: 8_420, month_tokens: 24_900, today_cost: null, week_cost: 7.42, month_cost: 18.9 };
        const local = machineState.machines[0];
        const totals = { today_tokens: machineState.totals.today_tokens + remoteTotals.today_tokens, week_tokens: machineState.totals.week_tokens + remoteTotals.week_tokens, month_tokens: machineState.totals.month_tokens + remoteTotals.month_tokens, today_cost: null, week_cost: (machineState.totals.week_cost ?? 0) + 7.42, month_cost: (machineState.totals.month_cost ?? 0) + 18.9 };
        machineState = { ...machineState, today_current_machines: 2, week_current_machines: 2, month_current_machines: 2, totals, machines: [local, { machine_id: remote.machine_id, machine_name: remote.machine_name, captured_at_ms: remote.captured_at_ms, source_count: remote.sources.length, currency: "USD", is_local: false, today_current: true, week_current: true, month_current: true, totals: remoteTotals }] };
        return machineState;
      },
    };
    window.__CCSTATS_TEST_CALLS__ = calls;
    window.__CCSTATS_TEST_EXPORT_CALLS__ = exportCalls;
  }, options);
}
