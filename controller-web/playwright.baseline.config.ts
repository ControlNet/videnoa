import { defineConfig, devices } from "@playwright/test"

// Redesign baseline capture only. Kept out of the default suite on purpose.
export default defineConfig({
  testDir: "./tests/e2e",
  testMatch: "**/redesign-baseline.capture.ts",
  outputDir: "/tmp/baseline-results",
  reporter: [["list"]],
  fullyParallel: false,
  workers: 1,
  use: {
    baseURL: "http://127.0.0.1:4174",
    trace: "off",
    video: "off",
  },
  projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"] } }],
  webServer: {
    command: "npm run build && npm run preview -- --host 127.0.0.1 --port 4174",
    port: 4174,
    reuseExistingServer: false,
  },
})
