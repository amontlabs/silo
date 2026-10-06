import { render, screen, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it, vi } from "vitest"

import { applicationSourceForScenario } from "@/fixtures/application-scenarios"
import type { ApplicationActions, ApplicationSource, ApplicationComputer } from "../model/application-source"
import { OverviewPage } from "./overview-page"

function sourceWith(change: (computer: ApplicationComputer) => void): ApplicationSource {
  const source = structuredClone(applicationSourceForScenario("complete"))
  source.devices = []
  source.runtimeRepair = null
  source.computerConfigurationOperation = null
  source.activities = []
  const dev = source.computers.find(({ configuration }) => configuration.name === "dev")!
  Object.assign(dev, { state: "running", freshness: "fresh", attention: undefined, lifecycleAction: undefined })
  change(dev)
  return source
}

const editor = applicationSourceForScenario("complete").preferences.editor
const terminal = applicationSourceForScenario("complete").preferences.terminal

async function reasonFor(user: ReturnType<typeof userEvent.setup>, control: HTMLElement) {
  expect(control).toBeDisabled()
  await user.hover(control.closest<HTMLElement>("[data-disabled-reason]")!)
  return (await screen.findByRole("tooltip")).textContent
}

it("explains the computer page's disabled Terminal, Editor and Start controls", async () => {
  const user = userEvent.setup()
  render(<OverviewPage source={sourceWith(computer => { computer.state = "stopped" })} actions={{} as ApplicationActions} onConfigurationsChange={vi.fn()} />)
  await user.click(screen.getByRole("button", { name: "Open dev" }))
  const header = within(screen.getByRole("navigation", { name: "Breadcrumb" }).parentElement!.parentElement!)

  expect(await reasonFor(user, header.getByRole("button", { name: `Open dev in ${terminal}` }))).toContain("Start dev to open it.")
  await user.unhover(header.getByRole("button", { name: `Open dev in ${terminal}` }).closest<HTMLElement>("[data-disabled-reason]")!)
  expect(header.getByRole("button", { name: `Open dev in ${editor}` })).toBeDisabled()
  expect(header.getByRole("button", { name: "Start dev" })).toBeEnabled()
})

it("offers Start for a crashed computer on the page as in the list", async () => {
  const user = userEvent.setup()
  render(<OverviewPage source={sourceWith(computer => { computer.state = "failed"; computer.attention = { level: "error", message: "The computer runtime crashed. Restart it to retry." } })} actions={{} as ApplicationActions} onConfigurationsChange={vi.fn()} />)
  const row = within(screen.getByText("dev").closest("li")!)
  expect(row.getByRole("button", { name: "Start dev" })).toBeEnabled()
  await user.click(row.getByRole("button", { name: "Open dev" }))
  expect(screen.getByRole("button", { name: "Start dev" })).toBeEnabled()
})

it("disables Stop while the computer is still starting, in the list and on the page, with the reason", async () => {
  const user = userEvent.setup()
  render(<OverviewPage source={sourceWith(computer => { computer.state = "starting"; computer.stateDetail = "Starting" })} actions={{} as ApplicationActions} onConfigurationsChange={vi.fn()} />)
  const row = within(screen.getByText("dev").closest("li")!)
  expect(await reasonFor(user, row.getByRole("button", { name: "Stop dev" }))).toContain("Wait for dev to finish starting.")
  await user.click(row.getByRole("button", { name: "Open dev" }))
  expect(screen.getByRole("button", { name: "Stop dev" })).toBeDisabled()
})


it.each(["list", "detail"])("pauses an open %s editor while a checkpoint runs and keeps its draft", async surface => {
  const user = userEvent.setup()
  const source = sourceWith(computer => { computer.state = "stopped" })
  const onConfigurationsChange = vi.fn()
  const view = (current: ApplicationSource) => <OverviewPage source={current} actions={{} as ApplicationActions} onConfigurationsChange={onConfigurationsChange} />
  const result = render(view(source))
  if (surface === "detail") await user.click(screen.getByRole("button", { name: "Open dev" }))
  await user.click(screen.getByRole("button", { name: "More actions for dev" }))
  await user.click(screen.getByRole("menuitem", { name: "Edit dev" }))
  await user.selectOptions(screen.getByRole("combobox", { name: "CPUs at start" }), "4")
  const busy = structuredClone(source)
  busy.computers.find(({ configuration }) => configuration.name === "dev")!.checkpointOperation = {
    kind: "capture", status: "running", stage: "Saving disk copies",
  }
  result.rerender(view(busy))
  expect(screen.getByRole("button", { name: "Save" })).toBeDisabled()
  expect(screen.getByRole("button", { name: "Save" })).toHaveAccessibleDescription("Wait for the checkpoint to finish.")
  expect(onConfigurationsChange).not.toHaveBeenCalled()
  result.rerender(view(source))
  expect(screen.getByRole("combobox", { name: "CPUs at start" })).toHaveValue("4")
  expect(screen.getByRole("button", { name: "Save" })).toBeEnabled()
})


it.each(["list", "detail"])("pauses an open %s editor when the computer status becomes stale", async surface => {
  const user = userEvent.setup()
  const source = sourceWith(computer => { computer.state = "stopped" })
  const onConfigurationsChange = vi.fn()
  const view = (current: ApplicationSource) => <OverviewPage source={current} actions={{} as ApplicationActions} onConfigurationsChange={onConfigurationsChange} />
  const result = render(view(source))
  if (surface === "detail") await user.click(screen.getByRole("button", { name: "Open dev" }))
  await user.click(screen.getByRole("button", { name: "More actions for dev" }))
  await user.click(screen.getByRole("menuitem", { name: "Edit dev" }))
  await user.selectOptions(screen.getByRole("combobox", { name: "CPUs at start" }), "4")
  const stale = structuredClone(source)
  stale.computers.find(({ configuration }) => configuration.name === "dev")!.freshness = "stale"
  result.rerender(view(stale))
  expect(screen.getByRole("button", { name: "Save" })).toBeDisabled()
  expect(screen.getByRole("button", { name: "Save" })).toHaveAccessibleDescription("Silo could not refresh this computer’s status.")
  await user.click(screen.getByRole("button", { name: "Save" }))
  expect(onConfigurationsChange).not.toHaveBeenCalled()
  result.rerender(view(source))
  expect(screen.getByRole("combobox", { name: "CPUs at start" })).toHaveValue("4")
  expect(screen.getByRole("button", { name: "Save" })).toBeEnabled()
})
