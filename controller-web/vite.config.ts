import react from "@vitejs/plugin-react"
import { defineConfig } from "vitest/config"

export default defineConfig({
  plugins: [react()],
  server: {
    proxy: {
      "/api": "http://localhost:3001",
    },
  },
  /*
   * `preview` inherits `server.proxy` by default, which lets any unrouted
   * request in the browser suite escape to whatever Controller happens to be
   * listening locally. Clearing it keeps preview-backed tests hermetic.
   */
  preview: {
    proxy: {},
  },
  test: {
    environment: "jsdom",
    exclude: ["tests/e2e/**", "node_modules/**"],
    setupFiles: ["./src/test/setup.ts"],
  },
})
