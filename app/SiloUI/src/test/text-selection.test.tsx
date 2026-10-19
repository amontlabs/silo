import { readFileSync } from "node:fs"
import { dirname, resolve } from "node:path"
import { fileURLToPath } from "node:url"
import { render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it, vi } from "vitest"

import { ConnectDeviceForm } from "@/features/application/components/connections-settings"
import { RuntimeMigrationBoundary, type RuntimeMigrationBackend, type RuntimeMigrationState } from "@/desktop/runtime-migration-boundary"

// The window disables text selection; this is the selector list that opts text back in.
const stylesheet = readFileSync(resolve(dirname(fileURLToPath(import.meta.url)), "../index.css"), "utf8")
const selectable = [...stylesheet.matchAll(/([^{}]+)\{[^{}]*user-select:\s*text/g)].map(([, selectors]) => selectors.trim()).join(",")

function expectSelectable(element: Element) {
  expect(selectable).not.toBe("")
  expect(element.matches(selectable), `${element.tagName} ${element.className}`).toBe(true)
}

it("lets a failed SSH connection message be selected", async () => {
  const user = userEvent.setup()
  render(<ConnectDeviceForm connect={vi.fn().mockRejectedValue(new Error("ssh: Permission denied (publickey)"))} onClose={vi.fn()} />)
  await user.type(screen.getByRole("textbox", { name: "Device address" }), "me@office")
  await user.click(screen.getByRole("button", { name: "Connect" }))
  expectSelectable(await screen.findByText("ssh: Permission denied (publickey)"))
})

it("lets the migration's full log path be selected", async () => {
  const state: RuntimeMigrationState = {
    status: "failed", stage: "Converting", logs: ["failed"], migratedCount: 0, failedCount: 1, totalCount: 1, canContinue: true,
    logPath: "/tmp/silo-migration.log", error: "dev could not be converted",
  }
  const backend: RuntimeMigrationBackend = { read: vi.fn().mockResolvedValue(state), retry: vi.fn(), continueAfterFailure: vi.fn(), subscribe: async () => () => {} }
  render(<RuntimeMigrationBoundary backend={backend}><p>App</p></RuntimeMigrationBoundary>)
  await userEvent.setup().click(await screen.findByRole("button", { name: "Show logs" }))
  expectSelectable(screen.getByText(/Full log:/))
})

it("lets error text that is not an alert be selected", () => {
  render(<><p data-testid="error" className="text-xs text-destructive">Failed</p><span data-testid="path" className="break-all">/Users/dev/project</span></>)
  expectSelectable(screen.getByTestId("error"))
  expectSelectable(screen.getByTestId("path"))
})
