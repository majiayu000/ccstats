import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "./tests",
  timeout: 20_000,
  fullyParallel: false,
  use: {
    baseURL: "http://127.0.0.1:1420",
    colorScheme: "dark",
    viewport: { width: 1440, height: 1000 },
  },
  webServer: {
    command: "npm run dev",
    url: "http://127.0.0.1:1420",
    reuseExistingServer: false,
    timeout: 30_000,
  },
});
