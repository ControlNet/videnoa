import { defineConfig, devices } from "@playwright/test"

export default defineConfig({
  testDir: "./tests/e2e",
  testMatch: "task-overflow.spec.ts",
  globalSetup: "./tests/e2e/global-setup.ts",
  outputDir: "../.omo/evidence/videnoa-controller/final/remediation-task-overflow/playwright/results",
  reporter: [["list"]],
  fullyParallel: false,
  workers: 1,
  use: {
    baseURL: "http://127.0.0.1:4197",
    trace: "off",
    video: "off",
  },
  projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"] } }],
  webServer: {
    command: "npm run build && npm run preview -- --host 127.0.0.1 --port 4197",
    port: 4197,
    reuseExistingServer: false,
  },
})
