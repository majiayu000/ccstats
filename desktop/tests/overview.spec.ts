import { expect, test, type Page } from "@playwright/test";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { rankConsumers } from "../src/bridge";

const WEB_ELEMENT_ID = "element-6066-11e4-a52e-4f735466cecf";

type WebDriverElement = { [WEB_ELEMENT_ID]: string };

test("token rankings do not prioritize a smaller priced row", () => {
  const rows = rankConsumers([
    { name: "small priced", tokens: 10, cost: 1 },
    { name: "large unpriced", tokens: 1_000_000, cost: null },
  ]);

  expect(rows.map((row) => row.name)).toEqual(["large unpriced", "small priced"]);
  expect(rows[0].share_basis).toBe("tokens");
});

async function webdriverRequest<T>(port: number, path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(`http://127.0.0.1:${port}${path}`, {
    ...init,
    headers: { "content-type": "application/json", ...init?.headers },
  });
  const payload = (await response.json()) as { value: T | { error?: string; message?: string } };
  if (!response.ok || (typeof payload.value === "object" && payload.value !== null && "error" in payload.value)) {
    throw new Error(`WebDriver ${init?.method ?? "GET"} ${path} failed: ${JSON.stringify(payload.value)}`);
  }
  return payload.value as T;
}

async function startWebdriverSession(port: number): Promise<string> {
  const deadline = Date.now() + 120_000;
  let lastError: unknown;
  while (Date.now() < deadline) {
    try {
      const value = await webdriverRequest<{ sessionId: string }>(port, "/session", {
        method: "POST",
        body: JSON.stringify({ capabilities: { alwaysMatch: { browserName: "tauri" } } }),
      });
      return value.sessionId;
    } catch (error) {
      lastError = error;
      await new Promise((resolve) => setTimeout(resolve, 250));
    }
  }
  throw new Error(`embedded WebDriver did not become ready: ${String(lastError)}`);
}

async function findElement(port: number, sessionId: string, selector: string): Promise<string> {
  const element = await webdriverRequest<WebDriverElement>(port, `/session/${sessionId}/element`, {
    method: "POST",
    body: JSON.stringify({ using: "css selector", value: selector }),
  });
  return element[WEB_ELEMENT_ID];
}

async function waitForElement(
  port: number,
  sessionId: string,
  selector: string,
  timeout = 120_000,
): Promise<string> {
  const deadline = Date.now() + timeout;
  let lastError: unknown;
  while (Date.now() < deadline) {
    try {
      return await findElement(port, sessionId, selector);
    } catch (error) {
      lastError = error;
      await new Promise((resolve) => setTimeout(resolve, 250));
    }
  }
  throw new Error(`element ${selector} did not appear: ${String(lastError)}`);
}

async function reserveAvailablePort(): Promise<number> {
  const server = createServer();
  await new Promise<void>((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  const address = server.address();
  if (!address || typeof address === "string") {
    server.close();
    throw new Error("failed to reserve a TCP port for the embedded WebDriver");
  }
  await new Promise<void>((resolve, reject) => {
    server.close((error) => (error ? reject(error) : resolve()));
  });
  return address.port;
}

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
};

async function injectBridge(page: Page, options: MockOptions = {}) {
  await page.addInitScript(({ delay, qualityWarning, fail, omitWeek, allMalformed, failCatalogOnce, quotaFail, liveGrowth, liveFailAfterGrowth, noReadySources, totalAdjustment }) => {
    const calls: string[] = [];
    const exportCalls: string[] = [];
    let catalogCalls = 0;
    type MockMachine = { machine_id: string; machine_name: string; captured_at_ms: number; source_count: number; currency: string | null; is_local: boolean; today_current: boolean; week_current: boolean; month_current: boolean; totals: { today_tokens: number; week_tokens: number; month_tokens: number; today_cost: number | null; week_cost: number | null; month_cost: number | null } };
    type MockRollup = { local_machine_id: string; local_machine_name: string | null; currency: string | null; today_current_machines: number; week_current_machines: number; month_current_machines: number; machines: MockMachine[]; totals: MockMachine["totals"] };
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
      projectDrilldown: async (source, range) => ({
        source,
        source_name: source,
        display_name: "Claude Code",
        range,
        currency: "USD",
        quality: { valid_entries: 4, dedup_skipped_entries: 0, parse_error_entries: 0 },
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
        return machineState;
      },
      exportMachineBundle: async () => JSON.stringify({ schema_version: 1, machines: machineState.machines }),
      importMachineBundle: async () => {
        const remoteTotals = { today_tokens: 1_240, week_tokens: 8_420, month_tokens: 24_900, today_cost: null, week_cost: 7.42, month_cost: 18.9 };
        const local = machineState.machines[0];
        const totals = { today_tokens: machineState.totals.today_tokens + remoteTotals.today_tokens, week_tokens: machineState.totals.week_tokens + remoteTotals.week_tokens, month_tokens: machineState.totals.month_tokens + remoteTotals.month_tokens, today_cost: null, week_cost: (machineState.totals.week_cost ?? 0) + 7.42, month_cost: (machineState.totals.month_cost ?? 0) + 18.9 };
        machineState = { ...machineState, today_current_machines: 2, week_current_machines: 2, month_current_machines: 2, totals, machines: [local, { machine_id: "remote-1", machine_name: "Remote laptop", captured_at_ms: 1_788_230_400_000, source_count: 1, currency: "USD", is_local: false, today_current: true, week_current: true, month_current: true, totals: remoteTotals }] };
        return machineState;
      },
    };
    window.__CCSTATS_TEST_CALLS__ = calls;
    window.__CCSTATS_TEST_EXPORT_CALLS__ = exportCalls;
  }, options);
}

test("loads the audit overview and preserves unknown cost", async ({ page }) => {
  await injectBridge(page, { delay: 2_000 });
  await page.goto("/", { waitUntil: "domcontentloaded" });

  await expect(page.getByText("Auditing registered sources…")).toBeVisible();
  await expect(page.getByRole("heading", { name: "Usage overview" })).toBeVisible();
  await expect(page.getByTestId("total-tokens")).toHaveText("1,240");
  await expect(page.getByTestId("total-cost")).toHaveText("Unknown");
  await expect(page.getByRole("cell", { name: "Unknown" })).toBeVisible();
});

test("reconciles a provider-authoritative total adjustment", async ({ page }) => {
  await injectBridge(page, { totalAdjustment: true });
  await page.goto("/");

  await expect(page.getByText("Provider adjustment")).toBeVisible();
  await expect(page.getByTestId("total-tokens")).toHaveText("1,240");
});

test("explains pricing provenance and partial API-equivalent coverage", async ({ page }) => {
  await injectBridge(page);
  await page.addInitScript(() => {
    const bridge = window.__CCSTATS_TEST_BRIDGE__!;
    const original = bridge.usageOverview;
    bridge.usageOverview = async (source) => {
      const result = await original(source);
      return { ...result, summaries: result.summaries.map((summary) => summary.range === "this_month" ? { ...summary, pricing_source: "fallback" } : summary) };
    };
  });
  await page.goto("/");

  await page.getByRole("button", { name: "Cost evidence" }).click();
  await expect(page.getByRole("heading", { name: "Cost evidence" })).toBeVisible();
  await expect(page.getByTestId("trust-displayed-cost")).toHaveText("Unknown");
  await expect(page.getByTestId("trust-pricing-source")).toHaveText("Unknown");

  await page.getByRole("button", { name: "This week" }).click();
  await expect(page.getByTestId("trust-pricing-source")).toHaveText("Stale price catalog");
  await expect(page.getByTestId("trust-coverage")).toHaveText("75.0%");
  await expect(page.getByText("Displayed cost is a lower bound")).toBeVisible();
  await expect(page.getByRole("heading", { name: "Source evidence" })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Model evidence" })).toBeVisible();
  await expect(page.getByRole("row", { name: /claude-sonnet-4-6/ })).toContainText("≥ $7.42");

  await page.getByRole("button", { name: "This month" }).click();
  await page.getByRole("button", { name: "Overview" }).click();
  await expect(page.getByTestId("total-cost")).toHaveText("≈ $18.90");
  await page.getByRole("button", { name: "Top" }).click();
  await expect(page.getByText("Share uses tokens across this complete ranking.").first()).toBeVisible();

  await page.setViewportSize({ width: 390, height: 844 });
  await expect.poll(() => page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
});

test("separates model turns from independently counted tool calls", async ({ page }) => {
  await injectBridge(page);
  await page.goto("/");

  await page.getByRole("button", { name: "Turns & tools" }).click();

  await expect(page.getByRole("heading", { name: "Turn and tool evidence" })).toBeVisible();
  await expect(page.getByTestId("activity-turn-count")).toHaveText("2");
  await expect(page.getByTestId("activity-tool-count")).toHaveText("5");
  await expect(page.getByText("Read", { exact: true })).toBeVisible();
  await expect(page.getByText("Tool tokens remain unknown")).toBeVisible();

  await page.getByLabel("Usage source").selectOption("codex");
  await expect(page.getByText("This source does not expose tool-call records.")).toBeVisible();
  await expect(page.getByTestId("activity-turn-count")).toHaveText("2");
});

test("shows source diagnostics and actionable setup guidance", async ({ page }) => {
  await injectBridge(page);
  await page.goto("/");

  await page.getByRole("button", { name: "Diagnostics" }).click();

  await expect(page.getByRole("heading", { name: "Source diagnostics", level: 2 })).toBeVisible();
  await expect(page.getByText("1 detected")).toBeVisible();
  await expect(page.getByText("1 missing")).toBeVisible();
  await expect(page.getByRole("cell", { name: "Claude Code" })).toBeVisible();
  await expect(page.getByText("Run Codex once")).toBeVisible();
});

test("shows Codex quota and calculates a monthly budget", async ({ page }) => {
  await injectBridge(page);
  await page.goto("/");

  await page.getByRole("button", { name: "Limits" }).click();

  await expect(page.getByRole("heading", { name: "Quota and budget", level: 2 })).toBeVisible();
  await expect(page.getByTestId("quota-used")).toHaveText("25.0%");
  await expect(page.getByTestId("quota-remaining")).toHaveText("75.0%");
  await expect(page.getByTestId("quota-value")).toHaveText("$40.00");
  await expect(page.getByTestId("budget-spent")).toHaveText("$18.90");

  await page.getByRole("spinbutton", { name: "Monthly budget" }).fill("1000");
  await expect(page.getByTestId("budget-remaining")).toHaveText("$981.10");
});

test("keeps monthly budget available when quota is unavailable", async ({ page }) => {
  await injectBridge(page, { quotaFail: true });
  await page.goto("/");

  await page.getByRole("button", { name: "Limits" }).click();

  await expect(page.getByText("No current Codex weekly quota snapshot")).toBeVisible();
  await expect(page.getByTestId("budget-spent")).toHaveText("$18.90");
});

test("does not present a lower-bound monthly cost as exact budget spend", async ({ page }) => {
  await injectBridge(page);
  await page.addInitScript(() => {
    const bridge = window.__CCSTATS_TEST_BRIDGE__!;
    const original = bridge.usageOverview;
    bridge.usageOverview = async (source) => {
      const result = await original(source);
      return { ...result, summaries: result.summaries.map((summary) => summary.range === "this_month" ? { ...summary, api_equivalent_cost_coverage: { total_tokens: summary.tokens.total_tokens, priced_tokens: summary.tokens.total_tokens - 100, percent: 99.6, complete: false, cost_is_lower_bound: true } } : summary) };
    };
  });
  await page.goto("/");
  await page.getByRole("button", { name: "Limits" }).click();

  await expect(page.getByTestId("budget-spent")).toHaveText("≥ $18.90");
  await expect(page.getByTestId("budget-remaining")).toHaveText("Unknown");
  await expect(page.getByRole("definition").filter({ hasText: "Lower bound" })).toBeVisible();
});

test("ranks consumers and flags a daily usage spike", async ({ page }) => {
  await injectBridge(page);
  await page.goto("/");

  await page.getByRole("button", { name: "Top" }).click();

  await expect(page.getByRole("heading", { name: "Top consumers", level: 2 })).toBeVisible();
  await expect(page.getByRole("cell", { name: "claude-sonnet-4-6" })).toBeVisible();
  await expect(page.getByRole("cell", { name: "ccstats" })).toBeVisible();
  await expect(page.getByText("Usage spike detected")).toBeVisible();
  await expect(page.getByTestId("anomaly-change")).toContainText("393.2%");

  await page.setViewportSize({ width: 390, height: 844 });
  await expect.poll(() => page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
});

test("monitors live growth, preserves trusted data on failure, and pauses polling", async ({ page }) => {
  await page.clock.install();
  await injectBridge(page, { liveGrowth: true, liveFailAfterGrowth: true });
  await page.goto("/");

  await page.getByRole("button", { name: "Live" }).click();
  await expect(page.getByRole("heading", { name: "Live usage", level: 2 })).toBeVisible();
  await expect(page.getByTestId("live-total-tokens")).toHaveText("1,240");
  await expect(page.getByText("Monitoring", { exact: true })).toBeVisible();

  await page.clock.fastForward(15_000);
  await expect(page.getByTestId("live-total-tokens")).toHaveText("1,490");
  await expect(page.getByTestId("live-token-delta")).toHaveText("+250");
  await expect(page.getByTestId("live-cost-delta")).toHaveText("+$0.50");

  await page.clock.fastForward(15_000);
  await expect(page.getByRole("alert")).toContainText("Refresh failed; showing the last trusted snapshot");
  await expect(page.getByTestId("live-total-tokens")).toHaveText("1,490");

  await page.getByRole("button", { name: "Pause monitoring" }).click();
  await page.clock.fastForward(30_000);
  await expect.poll(() => page.evaluate(() => window.__CCSTATS_TEST_CALLS__?.length)).toBe(4);
  await page.setViewportSize({ width: 390, height: 844 });
  await expect.poll(() => page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
});

test("persists this machine and imports a remote machine rollup", async ({ page }) => {
  await injectBridge(page);
  await page.goto("/");
  await page.getByRole("button", { name: "Machines" }).click();

  await expect(page.getByRole("heading", { name: "Machine rollup", level: 2 })).toBeVisible();
  await expect(page.getByText("No machine snapshots yet.")).toBeVisible();
  await page.getByLabel("Local machine name").fill("Studio Mac");
  await page.getByRole("button", { name: "Capture this machine" }).click();
  await expect(page.getByRole("row", { name: /Studio Mac/ })).toBeVisible();
  await expect(page.getByTestId("machines-month")).toHaveText("24,900");

  const download = page.waitForEvent("download");
  await page.getByRole("button", { name: "Export snapshots" }).click();
  expect((await download).suggestedFilename()).toBe("ccstats-machine-snapshots.json");
  await page.getByLabel("Import machine snapshots").setInputFiles({ name: "remote.json", mimeType: "application/json", buffer: Buffer.from('{"schema_version":1}') });
  await expect(page.getByRole("row", { name: /Remote laptop/ })).toBeVisible();
  await expect(page.getByTestId("machines-month")).toHaveText("49,800");

  await page.setViewportSize({ width: 390, height: 844 });
  await expect.poll(() => page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
});

test("keeps core evidence in the first viewport and reflows without page overflow", async ({ page }) => {
  await injectBridge(page);
  await page.goto("/");

  const total = await page.getByTestId("total-tokens").boundingBox();
  expect(total?.y).toBeLessThan(650);

  await page.setViewportSize({ width: 390, height: 844 });
  await expect(page.getByLabel("Usage source")).toBeVisible();
  await expect.poll(() => page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
});

test("switches windows and sources, then refreshes", async ({ page }) => {
  await injectBridge(page);
  await page.goto("/");

  await page.getByRole("button", { name: "This week" }).click();
  await expect(page.getByTestId("total-tokens")).toHaveText("8,420");
  await expect(page.getByTestId("total-cost")).toContainText("7.42");

  await page.getByLabel("Usage source").selectOption("codex");
  await expect(page.getByRole("cell", { name: "gpt-5" })).toBeVisible();
  await page.getByRole("button", { name: "Refresh ledger" }).click();
  await expect
    .poll(() => page.evaluate(() => window.__CCSTATS_TEST_CALLS__))
    .toEqual(["claude", "codex", "codex"]);
});

test("refreshes discovery before aggregating all sources", async ({ page }) => {
  await injectBridge(page);
  await page.addInitScript(() => {
    const bridge = window.__CCSTATS_TEST_BRIDGE__!;
    const original = bridge.sourceDiagnostics;
    let scans = 0;
    bridge.sourceDiagnostics = async () => {
      const findings = await original();
      scans += 1;
      return scans === 1 ? findings : findings.map((row) => row.name === "codex" ? { ...row, status: "detected", files: 1 } : row);
    };
  });
  await page.goto("/");

  await page.getByLabel("Usage source").selectOption("all");
  await expect.poll(() => page.evaluate(() => window.__CCSTATS_TEST_CALLS__)).toContain("codex");
  await expect(page.getByTestId("total-tokens")).toHaveText("2,480");
});

test("aggregates ready sources without scanning missing ledgers", async ({ page }) => {
  await injectBridge(page);
  await page.goto("/");

  await page.getByLabel("Usage source").selectOption("all");
  await page.getByRole("button", { name: "This week" }).click();

  await expect(page.getByTestId("total-tokens")).toHaveText("8,420");
  await expect(page.getByTestId("total-cost")).toContainText("7.42");
  await expect(page.getByRole("heading", { name: "Sources" })).toBeVisible();
  await expect(page.getByRole("row", { name: /Claude Code/ })).toBeVisible();
  await expect(page.getByRole("row", { name: /OpenAI Codex/ })).toHaveCount(0);
});

test("drills from a project into its real sessions", async ({ page }) => {
  await injectBridge(page);
  await page.goto("/");

  await page.getByRole("button", { name: "Projects" }).click();
  await expect(page.getByRole("heading", { name: "Projects & sessions" })).toBeVisible();
  await expect(page.getByRole("row", { name: /ccstats/ })).toContainText("1,200");
  await page.getByRole("button", { name: /ccstats/ }).click();
  await expect(page.getByText("session-abc123")).toBeVisible();
});

test("shows real history dates and exports CSV and JSON", async ({ page }) => {
  await injectBridge(page);
  await page.goto("/");

  await page.getByRole("button", { name: "History" }).click();
  await expect(page.getByRole("heading", { name: "History", exact: true })).toBeVisible();
  await expect(page.getByRole("row", { name: /2026-08-30/ })).toContainText("Unknown");
  await page.getByRole("button", { name: "Export CSV" }).click();
  await page.getByRole("button", { name: "Export JSON" }).click();
  await expect.poll(() => page.evaluate(() => window.__CCSTATS_TEST_EXPORT_CALLS__)).toEqual([
    "claude:today:csv",
    "claude:today:json",
  ]);
});

test("keeps the latest history request when an older range finishes later", async ({ page }) => {
  await injectBridge(page);
  await page.addInitScript(() => {
    const bridge = window.__CCSTATS_TEST_BRIDGE__!;
    const original = bridge.usageHistory;
    bridge.usageHistory = async (source, range) => {
      const result = await original(source, range);
      await new Promise((resolve) => setTimeout(resolve, range === "today" ? 120 : 10));
      return { ...result, range, points: result.points.map((point, index) => ({ ...point, date: range === "today" ? `2026-09-${String(index + 1).padStart(2, "0")}` : `2026-08-${String(index + 27).padStart(2, "0")}` })) };
    };
  });
  await page.goto("/");

  await page.getByRole("button", { name: "History" }).click();
  await page.getByRole("button", { name: "This week" }).click();
  await expect(page.getByRole("cell", { name: "2026-08-31", exact: true }).first()).toBeVisible();
  await page.waitForTimeout(150);
  await expect(page.getByRole("cell", { name: "2026-09-01", exact: true })).toHaveCount(0);
});

test("requires a concrete source for project drilldown", async ({ page }) => {
  await injectBridge(page);
  await page.goto("/");

  await page.getByLabel("Usage source").selectOption("all");
  await page.getByRole("button", { name: "Projects" }).click();
  await expect(page.getByText("Choose a concrete source to inspect projects and sessions.")).toBeVisible();
});

test("surfaces data quality warnings", async ({ page }) => {
  await injectBridge(page, { qualityWarning: true });
  await page.goto("/");

  await expect(page.getByRole("status")).toContainText("Review needed");
  await expect(page.getByRole("status")).toContainText("2 deduplicated");
  await expect(page.getByRole("status")).toContainText("1 malformed");
});

test("shows a recoverable error without fallback data", async ({ page }) => {
  await injectBridge(page, { fail: true });
  await page.goto("/");

  await expect(page.getByRole("alert")).toContainText("ledger is unavailable");
  await expect(page.getByTestId("overview-content")).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Try again" })).toBeVisible();
});

test("reports a missing requested range instead of leaving the page blank", async ({ page }) => {
  await injectBridge(page, { omitWeek: true });
  await page.goto("/");

  await page.getByRole("button", { name: "This week" }).click();
  await expect(page.getByRole("alert")).toContainText("This week is missing from the usage response");
});

test("does not attribute combined-scan parse errors to the selected period", async ({ page }) => {
  await injectBridge(page, { allMalformed: true });
  await page.goto("/");

  await expect(page.getByRole("alert")).toHaveCount(0);
  await expect(page.getByRole("heading", { name: "No valid Claude Code records in this window." })).toBeVisible();
  await expect(page.getByTestId("unattributed-parse-errors")).toContainText("3 malformed records were rejected during the combined scan");
  await expect(page.getByTestId("unattributed-parse-errors")).toContainText("cannot be assigned to today");
  await expect(page.getByText("ledger is quiet")).toHaveCount(0);
});

test("retries source discovery when catalog initialization fails", async ({ page }) => {
  await injectBridge(page, { failCatalogOnce: true });
  await page.goto("/");

  await expect(page.getByRole("alert")).toContainText("catalog is unavailable");
  await page.getByRole("button", { name: "Try again" }).click();
  await expect(page.getByTestId("total-tokens")).toHaveText("1,240");
  await expect
    .poll(() => page.evaluate(() => window.__CCSTATS_TEST_CATALOG_CALLS__))
    .toBe(2);
});

test("opens diagnostics when no source is detected or configured", async ({ page }) => {
  await injectBridge(page, { noReadySources: true });
  await page.goto("/");

  await expect(page.getByRole("heading", { name: "Source diagnostics", level: 1 })).toBeVisible();
  await expect.poll(() => page.evaluate(() => window.__CCSTATS_TEST_CALLS__)).toEqual([]);
  await expect(page.getByText("2 registered · 0 ready")).toBeVisible();
});

test("native Tauri app crosses IPC into the real Rust SDK", async () => {
  test.setTimeout(180_000);
  const port = await reserveAvailablePort();
  const isolatedHome = await mkdtemp(join(tmpdir(), "ccstats-native-e2e-"));
  const configDir = join(isolatedHome, ".config", "ccstats");
  await mkdir(configDir, { recursive: true });
  await writeFile(join(configDir, "config.toml"), "offline = true\n", "utf8");
  const binary = fileURLToPath(
    new URL("../src-tauri/target/debug/ccstats-desktop", import.meta.url),
  );
  const nativeProcess = spawn(binary, [], {
    cwd: isolatedHome,
    env: {
      HOME: isolatedHome,
      LANG: process.env.LANG,
      LC_ALL: process.env.LC_ALL,
      PATH: process.env.PATH,
      TMPDIR: process.env.TMPDIR,
      TAURI_WEBDRIVER_PORT: String(port),
    },
    stdio: ["ignore", "ignore", "pipe"],
  });
  let nativeStderr = "";
  nativeProcess.stderr.on("data", (chunk: Buffer) => {
    nativeStderr = `${nativeStderr}${chunk.toString()}`.slice(-4_000);
  });
  let sessionId: string | null = null;

  try {
    sessionId = await startWebdriverSession(port);
    await expect
      .poll(() => webdriverRequest<string>(port, `/session/${sessionId}/title`), {
        timeout: 120_000,
      })
      .toBe("ccstats · Local ledger");
    const heading = await waitForElement(port, sessionId, "h1");
    expect(["Source diagnostics", "Usage overview"]).toContain(
      await webdriverRequest<string>(port, `/session/${sessionId}/element/${heading}/text`),
    );

    const sourceSelect = await waitForElement(port, sessionId, "#source-select");
    await expect
      .poll(
        async () =>
          (
            await webdriverRequest<WebDriverElement[]>(
              port,
              `/session/${sessionId}/element/${sourceSelect}/elements`,
              {
                method: "POST",
                body: JSON.stringify({ using: "css selector", value: "option" }),
              },
            )
          ).length,
        { timeout: 120_000, message: "all Rust-registered sources did not cross the Tauri IPC boundary" },
      )
      .toBe(30);

    await webdriverRequest<null>(port, `/session/${sessionId}/execute/sync`, {
      method: "POST",
      body: JSON.stringify({
        script:
          "const select = arguments[0]; select.value = 'dsh'; select.dispatchEvent(new Event('change', { bubbles: true }));",
        args: [{ [WEB_ELEMENT_ID]: sourceSelect }],
      }),
    });
    await expect
      .poll(
        () =>
          webdriverRequest<string>(
            port,
            `/session/${sessionId}/element/${sourceSelect}/property/value`,
          ),
        { timeout: 120_000 },
      )
      .toBe("dsh");
    const overviewButton = await waitForElement(port, sessionId, "button[aria-label='Overview']");
    await webdriverRequest<null>(
      port,
      `/session/${sessionId}/element/${overviewButton}/click`,
      { method: "POST", body: "{}" },
    );
    const reportSource = await waitForElement(port, sessionId, ".report-meta strong");
    expect(
      await webdriverRequest<string>(
        port,
        `/session/${sessionId}/element/${reportSource}/text`,
      ),
      `DSH overview did not load through the native command: ${nativeStderr}`,
    ).toBe("DeepSeek Harness");

    const trustButton = await waitForElement(port, sessionId, "button[aria-label='Cost evidence']");
    await webdriverRequest<null>(
      port,
      `/session/${sessionId}/element/${trustButton}/click`,
      { method: "POST", body: "{}" },
    );
    const trustTitle = await waitForElement(port, sessionId, "#trust-title");
    expect(
      await webdriverRequest<string>(
        port,
        `/session/${sessionId}/element/${trustTitle}/text`,
      ),
      `cost provenance did not cross IPC: ${nativeStderr}`,
    ).toBe("Cost evidence");

    const activityButton = await waitForElement(port, sessionId, "button[aria-label='Turns & tools']");
    await webdriverRequest<null>(
      port,
      `/session/${sessionId}/element/${activityButton}/click`,
      { method: "POST", body: "{}" },
    );
    const activityTitle = await waitForElement(port, sessionId, "#activity-title");
    expect(
      await webdriverRequest<string>(
        port,
        `/session/${sessionId}/element/${activityTitle}/text`,
      ),
      `turn and tool command did not cross IPC: ${nativeStderr}`,
    ).toBe("Turn and tool evidence");
    const activityTurnCount = await waitForElement(port, sessionId, "[data-testid='activity-turn-count']");
    expect(
      await webdriverRequest<string>(
        port,
        `/session/${sessionId}/element/${activityTurnCount}/text`,
      ),
    ).toBe("0");

    const machinesButton = await waitForElement(port, sessionId, "button[aria-label='Machines']");
    await webdriverRequest<null>(
      port,
      `/session/${sessionId}/element/${machinesButton}/click`,
      { method: "POST", body: "{}" },
    );
    const machineTitle = await waitForElement(port, sessionId, "#machines-title");
    expect(
      await webdriverRequest<string>(
        port,
        `/session/${sessionId}/element/${machineTitle}/text`,
      ),
      `machine SQLite command did not cross IPC: ${nativeStderr}`,
    ).toBe("Machine rollup");
    expect(await findElement(port, sessionId, ".empty-state")).toBeTruthy();
  } finally {
    if (sessionId) {
      await webdriverRequest<null>(port, `/session/${sessionId}`, { method: "DELETE" }).catch(
        (error: unknown) => {
          nativeStderr = `${nativeStderr}\nWebDriver cleanup failed: ${String(error)}`;
        },
      );
    }
    if (nativeProcess.exitCode === null) {
      const exitPromise = once(nativeProcess, "exit");
      nativeProcess.kill("SIGTERM");
      const exited = await Promise.race([
        exitPromise.then(() => true),
        new Promise<false>((resolve) => setTimeout(() => resolve(false), 5_000)),
      ]);
      if (!exited) {
        nativeProcess.kill("SIGKILL");
        await exitPromise;
      }
      await rm(isolatedHome, { recursive: true, force: true });
      if (!exited) throw new Error("native app required SIGKILL during cleanup");
    } else {
      await rm(isolatedHome, { recursive: true, force: true });
    }
  }
});
