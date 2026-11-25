import { expect, test, type Page } from "@playwright/test";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { rankConsumers, type ProjectDrilldownSummary } from "../src/bridge";
import { injectBridge } from "./fixtures/overview-bridge";

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

test("separates model turns from independently counted tool calls", { tag: "@desktop-handover" }, async ({ page }) => {
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

test("shows source diagnostics and actionable setup guidance", { tag: "@desktop-handover" }, async ({ page }) => {
  await injectBridge(page);
  await page.goto("/");

  await page.getByRole("button", { name: "Diagnostics" }).click();

  await expect(page.getByRole("heading", { name: "Source diagnostics", level: 2 })).toBeVisible();
  await expect(page.getByText("1 detected")).toBeVisible();
  await expect(page.getByText("1 missing")).toBeVisible();
  await expect(page.getByRole("cell", { name: "Claude Code" })).toBeVisible();
  await expect(page.getByText("Run Codex once")).toBeVisible();
});

test("shows Codex quota and calculates a monthly budget", { tag: "@desktop-handover" }, async ({ page }) => {
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

test("keeps monthly budget available when quota is unavailable", { tag: "@desktop-handover" }, async ({ page }) => {
  await injectBridge(page, { quotaFail: true });
  await page.goto("/");

  await page.getByRole("button", { name: "Limits" }).click();

  await expect(page.getByText("No current Codex weekly quota snapshot")).toBeVisible();
  await expect(page.getByTestId("budget-spent")).toHaveText("$18.90");
});

test("does not present a lower-bound monthly cost as exact budget spend", { tag: "@desktop-handover" }, async ({ page }) => {
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

test("monitors live growth, preserves trusted data on failure, and pauses polling", { tag: "@desktop-handover" }, async ({ page }) => {
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

test("persists this machine and imports a remote machine rollup", { tag: "@desktop-handover" }, async ({ page }) => {
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
  const exported = await download;
  expect(exported.suggestedFilename()).toBe("ccstats-machine-snapshots.json");
  const bundle = JSON.parse(await readFile(await exported.path(), "utf8"));
  expect(bundle).toMatchObject({
    schema_version: 2,
    machines: [{ machine_id: "local-1", machine_name: "Studio Mac" }],
  });
  const remote = { ...bundle.machines[0], machine_id: "remote-1", machine_name: "Remote laptop" };
  await page.getByLabel("Import machine snapshots").setInputFiles({
    name: "remote.json",
    mimeType: "application/json",
    buffer: Buffer.from(JSON.stringify({ ...bundle, machines: [remote] })),
  });
  await expect(page.getByRole("row", { name: /Remote laptop/ })).toBeVisible();
  await expect(page.getByTestId("machines-month")).toHaveText("49,800");

  await page.getByLabel("Import machine snapshots").setInputFiles({
    name: "unsupported.json",
    mimeType: "application/json",
    buffer: Buffer.from(JSON.stringify({ ...bundle, schema_version: 99, machines: [remote] })),
  });
  await expect(page.getByRole("alert")).toHaveText("unsupported machine snapshot bundle version 99");
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

async function injectMixedSources(page: Page) {
  await injectBridge(page);
  await page.addInitScript(() => {
    const bridge = window.__CCSTATS_TEST_BRIDGE__!;
    const listSources = bridge.listSources;
    bridge.listSources = async () => [...await listSources(), {
      source: "cursor", name: "cursor", display_name: "Cursor", aliases: [],
      has_projects: false, has_reasoning_tokens: false, has_cache_creation: false, has_cache_read: false,
    }];
    const diagnostics = bridge.sourceDiagnostics;
    bridge.sourceDiagnostics = async () => [
      ...(await diagnostics()).map((row) => ({ ...row, status: row.name === "codex" ? "detected" as const : "missing" as const })),
      { source: "cursor", name: "cursor", display_name: "Cursor", status: "configured", files: 0, detail: "Credentials configured", setup: "ccstats login cursor" },
    ];
    const overviews = bridge.usageOverviews;
    bridge.usageOverviews = async (names) => {
      window.__CCSTATS_TEST_CALLS__!.push("usageOverviews");
      return overviews(names);
    };
    const save = bridge.saveMachineSnapshot;
    bridge.saveMachineSnapshot = async (name, snapshots) => {
      window.__CCSTATS_TEST_CALLS__!.push("saveMachineSnapshot");
      return save(name, snapshots);
    };
  });
}

async function corruptCursorCredentials(page: Page) {
  await page.evaluate(() => {
    const bridge = window.__CCSTATS_TEST_BRIDGE__!;
    const diagnostics = bridge.sourceDiagnostics;
    bridge.sourceDiagnostics = async () => (await diagnostics()).map((row) => row.name === "cursor"
      ? { ...row, status: "error", detail: "Failed to parse credentials file: invalid TOML" }
      : row);
    window.__CCSTATS_TEST_CALLS__!.length = 0;
  });
}

const cursorCredentialError = "Cursor: Failed to parse credentials file: invalid TOML";

test("rejects mixed ready/error diagnostics when refreshing All Sources", async ({ page }) => {
  await injectMixedSources(page);
  await page.goto("/");
  await page.getByLabel("Usage source").selectOption("all");
  await expect(page.getByTestId("total-tokens")).toHaveText("2,480");

  await corruptCursorCredentials(page);
  await page.getByRole("button", { name: "Refresh ledger" }).click();
  await expect(page.getByRole("alert")).toContainText(cursorCredentialError);
  await expect(page.getByTestId("overview-content")).toHaveCount(0);
  expect(await page.evaluate(() => window.__CCSTATS_TEST_CALLS__)).toEqual([]);
});

test("rejects mixed ready/error diagnostics before starting live All Sources", async ({ page }) => {
  await injectMixedSources(page);
  await page.goto("/");
  await page.getByLabel("Usage source").selectOption("all");
  await expect(page.getByTestId("total-tokens")).toHaveText("2,480");
  await corruptCursorCredentials(page);

  await page.getByRole("button", { name: "Live", exact: true }).click();
  await expect(page.getByRole("alert")).toContainText(cursorCredentialError);
  await expect(page.getByTestId("live-total-tokens")).toHaveCount(0);
  expect(await page.evaluate(() => window.__CCSTATS_TEST_CALLS__)).toEqual([]);
});

test("rejects mixed ready/error diagnostics on live refresh and labels retained data", { tag: "@desktop-handover" }, async ({ page }) => {
  await page.clock.install();
  await injectMixedSources(page);
  await page.goto("/");
  await page.getByLabel("Usage source").selectOption("all");
  await expect(page.getByTestId("total-tokens")).toHaveText("2,480");
  await page.getByRole("button", { name: "Live", exact: true }).click();
  await expect(page.getByTestId("live-total-tokens")).toHaveText("2,480");
  await corruptCursorCredentials(page);

  await page.clock.fastForward(15_000);
  await expect(page.getByRole("alert")).toContainText(cursorCredentialError);
  await expect(page.getByRole("alert")).toContainText("Refresh failed; showing the last trusted snapshot");
  await expect(page.getByTestId("live-total-tokens")).toHaveText("2,480");
  expect(await page.evaluate(() => window.__CCSTATS_TEST_CALLS__)).toEqual([]);
});

test("rejects mixed ready/error diagnostics before saving a machine snapshot", { tag: "@desktop-handover" }, async ({ page }) => {
  await injectMixedSources(page);
  await page.goto("/");
  await page.getByRole("button", { name: "Machines", exact: true }).click();
  await expect(page.getByText("No machine snapshots yet.")).toBeVisible();
  await corruptCursorCredentials(page);

  await page.getByRole("button", { name: "Capture this machine" }).click();
  await expect(page.getByRole("alert")).toContainText(cursorCredentialError);
  await expect(page.getByText("No machine snapshots yet.")).toBeVisible();
  expect(await page.evaluate(() => window.__CCSTATS_TEST_CALLS__)).toEqual([]);

  await page.evaluate(() => {
    const bridge = window.__CCSTATS_TEST_BRIDGE__!;
    const diagnostics = bridge.sourceDiagnostics;
    bridge.sourceDiagnostics = async () => (await diagnostics()).map((row) => row.name === "cursor"
      ? { ...row, status: "missing", detail: "No credentials configured" } : row);
  });
  await page.getByRole("button", { name: "Capture this machine" }).click();
  await expect(page.getByRole("alert")).toHaveCount(0);
  await expect(page.getByTestId("machines-today")).toHaveText("1,240");
  expect(await page.evaluate(() => window.__CCSTATS_TEST_CALLS__)).toEqual(["usageOverviews", "codex", "saveMachineSnapshot"]);
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
  await expect(page.locator(".session-title-text")).toHaveText("完善用量聚合");
  await expect(page.getByText(/Source summary/)).toBeVisible();
});

async function openSession(page: Page) {
  await page.getByRole("button", { name: "Projects", exact: true }).click();
  await page.getByRole("button", { name: "ccstats", exact: true }).click();
}

test("session titles persist locally, restore source names, and preserve usage and source identity", async ({ page }, testInfo) => {
  await injectBridge(page);
  await page.goto("/");
  await page.evaluate(() => localStorage.setItem('ccstats.session-title:["codex","session-abc123"]', "Other source title"));
  await openSession(page);
  const usage = await page.locator(".session-list dl").innerText();
  await page.getByRole("button", { name: "Rename session session-abc123" }).click();
  await page.getByLabel("Session title", { exact: true }).fill("手动命名 <b>保留文本</b>");
  await page.getByRole("button", { name: "Save title" }).click();
  await expect(page.locator(".session-title-text")).toHaveText("手动命名 <b>保留文本</b>");
  await expect(page.locator(".session-title-text b")).toHaveCount(0);
  await expect(page.locator(".session-list dl")).toHaveText(usage, { useInnerText: true });
  await page.reload();
  await openSession(page);
  await expect(page.locator(".session-title-text")).toHaveText("手动命名 <b>保留文本</b>");
  await expect(page.getByText(/Manual title/)).toBeVisible();
  await page.screenshot({ path: testInfo.outputPath("session-title-saved.png"), fullPage: true });
  await page.getByRole("button", { name: "Use source title" }).click();
  await expect(page.locator(".session-title-text")).toHaveText("完善用量聚合");
  await expect(page.getByText(/Source summary/)).toBeVisible();
  await page.getByRole("button", { name: "Rename session session-abc123" }).click();
  await page.screenshot({ path: testInfo.outputPath("session-title-editor.png"), fullPage: true });
  await page.getByRole("button", { name: "Cancel", exact: true }).click();
  expect(await page.evaluate(() => localStorage.getItem('ccstats.session-title:["codex","session-abc123"]'))).toBe("Other source title");
  await page.reload();
  await openSession(page);
  await expect(page.locator(".session-title-text")).toHaveText("完善用量聚合");
});

test("missing session titles use project, first activity and short ID; empty edits do not save", async ({ page }) => {
  await injectBridge(page, { titleMode: "missing" });
  await page.goto("/");
  await openSession(page);
  const fallback = await page.evaluate(() => `ccstats · ${new Date("2026-08-31T06:00:00Z").toLocaleString()} · session-`);
  await expect(page.locator(".session-title-text")).toHaveText(fallback);
  await expect(page.getByText(/Session label/)).toBeVisible();
  await page.getByRole("button", { name: "Rename session session-abc123" }).click();
  await page.getByLabel("Session title", { exact: true }).fill("   ");
  await page.getByRole("button", { name: "Save title" }).click();
  await expect(page.getByRole("alert")).toContainText("Enter a title");
  await page.getByRole("button", { name: "Cancel", exact: true }).click();
  await expect(page.locator(".session-title-text")).toHaveText(fallback);
});

test("source title errors remain visible while session usage stays available", { tag: "@desktop-handover" }, async ({ page }) => {
  await injectBridge(page, { titleMode: "error" });
  await page.goto("/");
  await openSession(page);
  await expect(page.getByRole("alert")).toContainText("Source titles could not be loaded");
  await expect(page.getByRole("alert")).toContainText("line 2");
  await expect(page.locator(".session-list dl")).toContainText("1,200");
  await expect(page.locator(".session-list dl")).toContainText("$2.40");
});

test("failed session title saves report an error and retain the current title", async ({ page }) => {
  await injectBridge(page);
  await page.goto("/");
  await openSession(page);
  await page.evaluate(() => { Storage.prototype.setItem = () => { throw new DOMException("Storage is full", "QuotaExceededError"); }; });
  await page.getByRole("button", { name: "Rename session session-abc123" }).click();
  await page.getByLabel("Session title", { exact: true }).fill("New title");
  await page.getByRole("button", { name: "Save title" }).click();
  await expect(page.getByRole("alert")).toContainText("Title could not be saved");
  await expect(page.locator(".session-title-text")).toHaveText("完善用量聚合");
  await expect(page.getByLabel("Session title", { exact: true })).toHaveValue("New title");
});

test("failed saved title reads report an error and can be retried", async ({ page }) => {
  await injectBridge(page);
  await page.goto("/");
  await page.evaluate(() => {
    const read = Storage.prototype.getItem;
    Storage.prototype.getItem = function(key) {
      Storage.prototype.getItem = read;
      throw new DOMException(`Storage blocked for ${key}`, "SecurityError");
    };
  });
  await openSession(page);
  await expect(page.getByRole("alert")).toContainText("Saved title could not be read");
  await page.getByRole("button", { name: "Retry saved title" }).click();
  await expect(page.getByRole("alert")).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Rename session session-abc123" })).toBeVisible();
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
    new URL(`../src-tauri/target/debug/ccstats-desktop${process.platform === "win32" ? ".exe" : ""}`, import.meta.url),
  );
  const nativeProcess = spawn(binary, [], {
    cwd: isolatedHome,
    env: {
      SystemRoot: process.env.SystemRoot,
      WINDIR: process.env.WINDIR,
      TEMP: isolatedHome,
      TMP: isolatedHome,
      HOME: isolatedHome,
      XDG_CONFIG_HOME: join(isolatedHome, ".config"),
      XDG_DATA_HOME: join(isolatedHome, ".local", "share"),
      XDG_CACHE_HOME: join(isolatedHome, ".cache"),
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

    const sourceSelect = await waitForElement(port, sessionId, "#source-select:enabled");
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
    const overviewButton = await waitForElement(port, sessionId, "button[aria-label='Overview']:enabled");
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

    const trustButton = await waitForElement(port, sessionId, "button[aria-label='Cost evidence']:enabled");
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

    const activityButton = await waitForElement(port, sessionId, "button[aria-label='Turns & tools']:enabled");
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

    const machinesButton = await waitForElement(port, sessionId, "button[aria-label='Machines']:enabled");
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
