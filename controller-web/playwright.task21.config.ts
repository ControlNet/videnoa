import { defineConfig, devices } from "@playwright/test"

export default defineConfig({
  testDir: "./tests/e2e",
  testMatch: "task-21.spec.ts",
  outputDir: "../.omo/evidence/videnoa-controller/task-21/playwright/results",
  reporter: [
    ["list"],
    ["html", { outputFolder: "../.omo/evidence/videnoa-controller/task-21/playwright/html", open: "never" }],
  ],
  fullyParallel: false,
  workers: 1,
  use: {
    baseURL: "http://127.0.0.1:4181",
    screenshot: "off",
    trace: "off",
    video: "off",
  },
  projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"] } }],
  webServer: {
    command: "npm run build && npm run preview -- --host 127.0.0.1 --port 4181",
    port: 4181,
    reuseExistingServer: false,
  },
})
