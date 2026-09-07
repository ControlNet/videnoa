import { mkdir } from "node:fs/promises"

const directories = [
  "../.omo/evidence/videnoa-controller/task-19/playwright-report/screenshots/task-15/visual-qa",
  "../.omo/evidence/videnoa-controller/task-19/playwright-report/screenshots/task-16/tasks-table",
  "../.omo/evidence/videnoa-controller/task-19/playwright-report/screenshots/task-17/task-actions",
  "../.omo/evidence/videnoa-controller/task-19/playwright-report/screenshots/task-18/workers-settings",
  "../.omo/evidence/videnoa-controller/task-19/visual-failures",
  "../.omo/evidence/videnoa-controller/controller-local-bootstrap",
] as const

export default async function globalSetup(): Promise<void> {
  await Promise.all(directories.map(async (directory) => mkdir(directory, { recursive: true })))
}
