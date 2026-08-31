import { expect, test, type Page } from "@playwright/test";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const WEB_ELEMENT_ID = "element-6066-11e4-a52e-4f735466cecf";

type WebDriverElement = { [WEB_ELEMENT_ID]: string };

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
};

async function injectBridge(page: Page, options: MockOptions = {}) {
  await page.addInitScript(({ delay, qualityWarning, fail, omitWeek, allMalformed, failCatalogOnce }) => {
    const calls: string[] = [];
    let catalogCalls = 0;
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
      usageOverview: async (source: string) => {
        calls.push(source);
        if (fail) throw new Error("ledger is unavailable");
        if (delay) await new Promise((resolve) => setTimeout(resolve, delay));
        return {
          source,
          source_name: source,
          display_name: source === "codex" ? "OpenAI Codex" : "Claude Code",
          currency: "USD",
          generated_at: "2026-08-31T08:00:00Z",
          elapsed_ms: 12.4,
          summaries: summaries
            .filter(({ range }) => !omitWeek || range !== "this_week")
            .map(({ range, total, records }, index) => ({
            source,
            source_name: source,
            display_name: source === "codex" ? "OpenAI Codex" : "Claude Code",
            range,
            since: "2026-08-01",
            until: "2026-08-31",
            currency: "USD",
            cost: index === 0 ? null : index === 1 ? 7.42 : 18.9,
            cost_usd: index === 0 ? null : index === 1 ? 7.42 : 18.9,
            estimated_cost: null,
            estimated_cost_usd: null,
            cost_kind: index === 0 ? "unknown" : "priced",
            api_equivalent_cost_coverage: null,
            tokens: {
              input_tokens: allMalformed ? 0 : total - 300,
              output_tokens: allMalformed ? 0 : 180,
              reasoning_tokens: allMalformed ? 0 : 20,
              cache_creation_tokens: 0,
              cache_read_tokens: allMalformed ? 0 : 100,
              cache_hit_rate: allMalformed ? null : 12.5,
              total_tokens: allMalformed ? 0 : total,
            },
            models: [
              {
                model: source === "codex" ? "gpt-5" : "claude-sonnet-4-6",
                cost: index === 0 ? null : 7.42,
                cost_usd: index === 0 ? null : 7.42,
                estimated_cost: null,
                estimated_cost_usd: null,
                cost_kind: index === 0 ? "unknown" : "priced",
                tokens: {
                  input_tokens: total - 300,
                  output_tokens: 180,
                  reasoning_tokens: 20,
                  cache_creation_tokens: 0,
                  cache_read_tokens: 100,
                  cache_hit_rate: 12.5,
                  total_tokens: total,
                },
              },
            ],
            valid_entries: allMalformed ? 0 : records,
            skipped_entries: qualityWarning ? 2 : 0,
            parse_error_entries: allMalformed ? 3 : qualityWarning ? 1 : 0,
            elapsed_ms: 12.4,
            })),
        };
      },
    };
    window.__CCSTATS_TEST_CALLS__ = calls;
  }, options);
}

test("loads the audit overview and preserves unknown cost", async ({ page }) => {
  await injectBridge(page, { delay: 300 });
  await page.goto("/");

  await expect(page.getByText("Auditing registered sources…")).toBeVisible();
  await expect(page.getByRole("heading", { name: "Usage overview" })).toBeVisible();
  await expect(page.getByTestId("total-tokens")).toHaveText("1,240");
  await expect(page.getByTestId("total-cost")).toHaveText("Unknown");
  await expect(page.getByRole("cell", { name: "Unknown" })).toBeVisible();
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

test("reports malformed records instead of calling the ledger empty", async ({ page }) => {
  await injectBridge(page, { allMalformed: true });
  await page.goto("/");

  await expect(page.getByRole("alert")).toContainText("No records could be parsed");
  await expect(page.getByRole("alert")).toContainText("3 malformed entries");
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
    expect(
      await webdriverRequest<string>(port, `/session/${sessionId}/element/${heading}/text`),
    ).toBe("Usage overview");

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
      .toBe(29);

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
    await expect
      .poll(
        async () => {
          const reportSource = await findElement(port, sessionId!, ".report-meta strong");
          return webdriverRequest<string>(
            port,
            `/session/${sessionId}/element/${reportSource}/text`,
          );
        },
        {
          timeout: 120_000,
          message: `DSH overview did not load through the native command: ${nativeStderr}`,
        },
      )
      .toBe("DeepSeek Harness");
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
