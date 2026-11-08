import { render, screen, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it, vi } from "vitest"

import { applicationSourceForScenario } from "@/fixtures/application-scenarios"
import { computerCan } from "@/features/computers/model/computer-capabilities"
import type { BackupController } from "../model/backup-source"
import type { ApplicationActions } from "../model/application-source"
import { OverviewPage } from "./overview-page"

const backup = { state: { snapshotId: "1", availability: "available", archives: [], operation: null }, actions: {} } as unknown as BackupController

function setup(container: boolean) {
  const source = structuredClone(applicationSourceForScenario("complete"))
  source.devices = []
  source.runtimeRepair = null
  source.computerConfigurationOperation = null
  source.activities = []
  const dev = source.computers.find(({ configuration }) => configuration.name === "dev")!
  Object.assign(dev, { state: "running", freshness: "fresh", attention: undefined, lifecycleAction: undefined })
  delete dev.configuration.desktop
  if (container) dev.configuration.runtime = "container"
  const actions = { readWorkspaceStorage: vi.fn(() => new Promise(() => {})), refreshSshAccess: vi.fn() } as unknown as ApplicationActions
  render(<OverviewPage source={source} actions={actions} backup={backup} onExportComputer={vi.fn()} onConfigurationsChange={vi.fn()} />)
}

it("knows what a container computer cannot do yet, and a VM can do everything", () => {
  for (const capability of ["terminal", "editor", "sshAccess", "export", "desktop"] as const) {
    expect(computerCan({ runtime: "container" }, capability)).toBe(false)
    expect(computerCan({}, capability)).toBe(true)
  }
})

it("offers a virtual machine its terminal, editor, SSH and export", async () => {
  const user = userEvent.setup()
  setup(false)
  const row = within(screen.getByText("dev").closest("li")!)
  expect(row.getAllByRole("button", { name: /^Open dev in / })).toHaveLength(2)
  await user.click(row.getByRole("button", { name: "More actions for dev" }))
  expect(screen.getByRole("menuitem", { name: "SSH for dev" })).toBeInTheDocument()
  expect(screen.getByRole("menuitem", { name: "Export dev" })).toBeInTheDocument()
})

it("hides the terminal, editor, SSH and export of a container computer", async () => {
  const user = userEvent.setup()
  setup(true)
  const row = within(screen.getByText("dev").closest("li")!)
  expect(row.queryByRole("button", { name: /^Open dev in / })).not.toBeInTheDocument()
  await user.click(row.getByRole("button", { name: "More actions for dev" }))
  expect(screen.queryByRole("menuitem", { name: "SSH for dev" })).not.toBeInTheDocument()
  expect(screen.queryByRole("menuitem", { name: "Export dev" })).not.toBeInTheDocument()
  expect(screen.queryByRole("menuitem", { name: "Add Linux desktop" })).not.toBeInTheDocument()
  await user.keyboard("{Escape}")
  await user.click(row.getByRole("button", { name: "Open dev" }))
  expect(screen.queryByRole("button", { name: /^Open dev in / })).not.toBeInTheDocument()
  expect(screen.queryByRole("tab", { name: "SSH" })).not.toBeInTheDocument()
})
