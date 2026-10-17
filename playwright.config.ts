import { defineConfig, devices } from "@playwright/test";

export default defineConfig({
  testDir: "./e2e",
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 2 : 0,
  // why: exactOptionalPropertyTypes rejects `workers: undefined`, so the key is omitted locally
  // and Playwright picks its default worker count.
  ...(process.env.CI ? { workers: 1 } : {}),
  reporter: process.env.CI
    ? [["github"], ["html", { open: "never", outputFolder: "playwright-report" }]]
    : "list",
  use: {
    baseURL: "http://127.0.0.1:43111",
    trace: "on-first-retry",
    permissions: ["clipboard-read", "clipboard-write"],
  },
  projects: [
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"] },
    },
  ],
  webServer: {
    command: "npm run start",
    env: { HOST: "127.0.0.1", NODE_ENV: "production", PORT: "43111" },
    url: "http://127.0.0.1:43111",
    reuseExistingServer: false,
    timeout: 120_000,
  },
});
