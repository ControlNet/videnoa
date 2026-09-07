import { defineConfig, devices } from "@playwright/test"

import base from "./playwright.config"

// A .test hostname remains an insecure context even though DNS maps it to loopback.
export default defineConfig({
  ...base,
  outputDir: "../.omo/evidence/videnoa-controller/http-compatibility/results",
  reporter: [["list"]],
  projects: [{
    name: "chromium-http",
    use: {
      ...devices["Desktop Chrome"],
      baseURL: "http://controller-http.test:4173",
      launchOptions: {
        args: ["--host-resolver-rules=MAP controller-http.test 127.0.0.1", "--no-proxy-server"],
      },
    },
  }],
  webServer: {
    command: "npm run build && npm run preview -- --host 127.0.0.1",
    port: 4173,
    reuseExistingServer: false,
    env: { __VITE_ADDITIONAL_SERVER_ALLOWED_HOSTS: "controller-http.test" },
  },
})
