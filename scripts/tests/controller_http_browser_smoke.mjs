#!/usr/bin/env node
// Real Docker + browser HTTP smoke. Uses isolated synthetic media and an offline
// test worker; no production workspace, credentials, or GPU jobs are touched.
import assert from "node:assert/strict"
import { execFileSync } from "node:child_process"
import { randomBytes } from "node:crypto"
import { mkdtemp, mkdir, readFile, rm, writeFile } from "node:fs/promises"
import { createRequire } from "node:module"
import { tmpdir } from "node:os"
import { join } from "node:path"

const require = createRequire(new URL("../../controller-web/package.json", import.meta.url))
const { chromium, expect } = require("@playwright/test")
const image = process.argv[2] ?? "videnoa-controller:dev"
const name = `videnoa-http-browser-smoke-${process.pid}`
const root = await mkdtemp(join(tmpdir(), "videnoa-http-browser-"))
const password = randomBytes(24).toString("hex")
const docker = (...args) => execFileSync("docker", args, { encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] }).trim()
let browser
let started = false

try {
  await mkdir(join(root, "media"))
  await writeFile(join(root, "media", "input.mkv"), "synthetic HTTP smoke input")
  docker("run", "-d", "--name", name, "--user", `${process.getuid()}:${process.getgid()}`,
    "-p", "127.0.0.1::3001", "-v", `${root}:/workspace`, image)
  started = true
  let port = docker("port", name, "3001/tcp").split(":").at(-1)
  let localOrigin = `http://127.0.0.1:${port}`
  let origin = `http://controller-http.test:${port}`
  async function healthy() {
    await expect.poll(async () => {
      try { return (await fetch(`${localOrigin}/api/health`)).status } catch { return 0 }
    }, { timeout: 30_000 }).toBe(200)
  }
  await healthy()
  browser = await chromium.launch({ args: ["--host-resolver-rules=MAP controller-http.test 127.0.0.1", "--no-proxy-server"] })
  const context = await browser.newContext()
  const page = await context.newPage()
  page.setDefaultTimeout(15_000)
  const errors = []
  page.on("pageerror", (error) => errors.push(error.message))
  await page.goto(origin)
  assert.deepEqual(await page.evaluate(() => ({ secure: isSecureContext, uuid: typeof crypto.randomUUID })),
    { secure: false, uuid: "undefined" })

  // Real first-access setup, cookie attributes, authenticated reload, and SSE.
  await page.getByLabel("Create password", { exact: true }).fill(password)
  await page.getByLabel("Confirm password", { exact: true }).fill(password)
  await page.getByRole("button", { name: "Create secure access" }).click()
  await expect(page.getByRole("heading", { name: "Tasks", exact: true })).toBeVisible()
  const cookies = await context.cookies(origin)
  const session = cookies.find((cookie) => cookie.name === "videnoa_session")
  assert.ok(session && session.httpOnly && !session.secure && session.sameSite === "Strict")
  await page.reload()
  await expect(page.getByText("Controller connected", { exact: true })).toBeVisible()

  // HTTP Settings persistence and scheduler admission controls.
  await page.getByRole("link", { name: "Settings", exact: true }).click()
  await expect(page.getByLabel("Require secure session cookie")).not.toBeChecked()
  await page.getByLabel("Concurrent uploads").fill("2")
  await page.getByRole("button", { name: "Save and apply settings" }).click()
  await expect(page.locator(".settings-save-receipt")).toContainText("Settings saved and applied")
  await page.getByRole("button", { name: "Pause scheduler" }).click()
  await expect(page.getByRole("button", { name: "Resume scheduler" })).toBeVisible()
  assert.match(await readFile(join(root, "data/controller.toml"), "utf8"), /paused = true/)
  await page.getByRole("button", { name: "Resume scheduler" }).click()
  await expect(page.getByRole("button", { name: "Pause scheduler" })).toBeVisible()
  await page.getByRole("button", { name: "Pause scheduler" }).click()
  await expect(page.getByRole("button", { name: "Resume scheduler" })).toBeVisible()

  // Worker CRUD over authenticated HTTP; the worker is intentionally offline.
  await page.getByRole("link", { name: "Workers", exact: true }).click()
  await page.getByRole("button", { name: "Add Worker" }).click()
  await page.getByLabel("Worker name").fill("http-smoke-offline")
  await page.getByLabel("Worker API URL").fill("http://127.0.0.1:9")
  await page.getByRole("button", { name: "Save Worker" }).click()
  await expect(page.getByRole("button", { name: "Edit http-smoke-offline" })).toBeVisible()
  await page.getByRole("button", { name: "Edit http-smoke-offline" }).click()
  await page.getByLabel("Compute slots").fill("2")
  await page.getByRole("button", { name: "Save Worker" }).click()
  await expect(page.getByRole("dialog")).toHaveCount(0)
  await page.getByRole("button", { name: "Disable http-smoke-offline" }).click()
  await page.getByRole("button", { name: "Enable http-smoke-offline" }).click()
  await expect(page.getByRole("button", { name: "Disable http-smoke-offline" })).toBeVisible()
  await page.getByRole("button", { name: "Delete http-smoke-offline" }).click()
  await page.getByRole("button", { name: "Delete Worker", exact: true }).click()
  await expect(page.getByRole("button", { name: "Edit http-smoke-offline" })).toHaveCount(0)

  // Real task intake and cancellation, using only synthetic input and no compute.
  await page.getByRole("link", { name: "Tasks", exact: true }).click()
  await page.getByRole("button", { name: "Add Task" }).click()
  await page.getByRole("textbox", { name: "Input Path", exact: true }).fill("/workspace/media/input.mkv")
  await page.getByRole("textbox", { name: "Output Path", exact: true }).fill("/workspace/media/output.mp4")
  await page.getByLabel("Workflow").last().fill("synthetic-http-smoke")
  const creation = page.waitForResponse((response) => response.url().endsWith("/api/tasks") && response.request().method() === "POST")
  await page.getByRole("button", { name: "Create Task" }).click()
  const response = await creation
  assert.equal(response.status(), 201)
  const task = await response.json()
  assert.match(response.request().headers()["idempotency-key"], /^[0-9a-f-]{36}$/)
  await expect(page.getByRole("region", { name: "Task Detail" })).toContainText("input.mkv")
  await page.getByRole("button", { name: "Cancel Task", exact: true }).click()
  await page.getByRole("button", { name: "Confirm Cancellation" }).click()
  await expect.poll(async () => page.evaluate(async (id) => {
    const response = await fetch(`/api/tasks/${id}`)
    return (await response.json()).task.status
  }, task.id)).toBe("cancelled")
  await page.getByRole("button", { name: "Close Task Detail" }).click()

  // Cookie-based HTTP requests still require CSRF proof.
  assert.equal(await page.evaluate(async () => (await fetch("/api/auth/logout", { method: "POST" })).status), 403)
  await page.getByRole("button", { name: "Sign out" }).click()
  await expect(page.getByRole("heading", { name: "Sign in to Controller" })).toBeVisible()
  await page.getByLabel("Controller password").fill(password)
  await page.getByRole("button", { name: "Sign in", exact: true }).click()
  await expect(page.getByRole("heading", { name: "Tasks", exact: true })).toBeVisible()

  // A real container restart retains the HTTP session, TOML policy, and task history.
  await page.goto("about:blank")
  docker("restart", "--time", "35", name)
  // Docker can assign a new ephemeral published port when the container restarts.
  port = docker("port", name, "3001/tcp").split(":").at(-1)
  localOrigin = `http://127.0.0.1:${port}`
  origin = `http://controller-http.test:${port}`
  await healthy()
  await page.goto(`${origin}/settings`)
  await expect(page.getByRole("heading", { name: "Settings", exact: true })).toBeVisible()
  await expect(page.getByLabel("Concurrent uploads")).toHaveValue("2")
  await expect(page.getByRole("button", { name: "Resume scheduler" })).toBeVisible()
  await expect(page.getByText("Controller connected", { exact: true })).toBeVisible()
  assert.equal(await page.evaluate(async (id) => (await fetch(`/api/tasks/${id}`)).status, task.id), 200)
  assert.deepEqual(errors, [])
  console.log("PASS: real non-secure HTTP browser + Docker setup/login/logout, HttpOnly cookie, CSRF rejection, SSE, Settings/TOML, pause/resume, Worker CRUD, task create/cancel, restart persistence")
} finally {
  await browser?.close()
  if (started) docker("rm", "-f", name)
  await rm(root, { recursive: true, force: true })
}
