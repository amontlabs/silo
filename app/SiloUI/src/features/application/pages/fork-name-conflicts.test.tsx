import { render, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it, vi } from "vitest"
import { createFixtureMacosComputersStore } from "@/fixtures/macos-computers"
import { MacosComputersContext } from "@/features/macos-computers/model/macos-computers"
import { applicationSourceForScenario } from "@/fixtures/application-scenarios"
import type { ApplicationActions, ApplicationComputer } from "../model/application-source"
import { OverviewPage } from "./overview-page"

function localComputer(source: ReturnType<typeof applicationSourceForScenario>, name: string): ApplicationComputer {
  return source.computers.find(item => !item.device && item.configuration.name === name)!
}

it("rejects a current-state fork name that another computer on this device already uses", async () => {
  const forkCheckpoint = vi.fn()
  const source = structuredClone(applicationSourceForScenario("complete"))
  const computer = localComputer(source, "dev")
  const user = userEvent.setup()
  render(<OverviewPage source={source} actions={{ forkCheckpoint } as unknown as ApplicationActions} onConfigurationsChange={vi.fn()} />)

  await user.click(screen.getByRole("button", { name: `More actions for ${computer.configuration.name}` }))
  await user.click(screen.getByRole("menuitem", { name: `Fork ${computer.configuration.name}` }))
  await user.type(await screen.findByRole("textbox", { name: "New computer name" }), "playgrounds")

  expect(screen.getByText("A computer named playgrounds already exists.")).toBeVisible()
  expect(screen.getByRole("button", { name: "Fork" })).toBeDisabled()
  expect(forkCheckpoint).not.toHaveBeenCalled()
})

it("rejects a checkpoint fork name that another computer on this device already uses", async () => {
  const forkCheckpoint = vi.fn()
  const source = structuredClone(applicationSourceForScenario("complete"))
  const computer = localComputer(source, "dev")
  const office = { id: "office", name: "Office", address: "office.local", connected: true }
  const remote = structuredClone(localComputer(source, "playgrounds"))
  source.computers.push({ ...remote, configuration: { ...remote.configuration, id: "remote-computer", name: "remote-only" }, device: { ...office, computerId: "remote-computer" } })
  source.devices = [office]
  const user = userEvent.setup()
  render(<OverviewPage source={source} actions={{ forkCheckpoint } as unknown as ApplicationActions} onConfigurationsChange={vi.fn()} />)

  await user.click(screen.getAllByRole("button", { name: `More actions for ${computer.configuration.name}` })[0])
  await user.click(screen.getByRole("menuitem", { name: `Checkpoints for ${computer.configuration.name}` }))
  const panel = within(screen.getByRole("region", { name: `Checkpoints for ${computer.configuration.name}` }))
  const checkpoint = computer.checkpoints![0]
  await user.click(panel.getByRole("button", { name: `Checkpoint actions for ${checkpoint.name}` }))
  await user.click(screen.getByRole("menuitem", { name: `Fork ${checkpoint.name}` }))
  await user.type(await screen.findByRole("textbox", { name: "New computer name" }), "personal")

  expect(screen.getByText("A computer named personal already exists.")).toBeVisible()
  expect(screen.getByRole("button", { name: "Fork" })).toBeDisabled()

  // A name that is only used on another device stays available here.
  await user.clear(screen.getByRole("textbox", { name: "New computer name" }))
  await user.type(screen.getByRole("textbox", { name: "New computer name" }), "remote-only")
  expect(screen.queryByText(/already exists/)).toBeNull()
  expect(screen.getByRole("button", { name: "Fork" })).toBeEnabled()
})

it("rejects a fork name that a macOS computer on this device already uses", async () => {
  const forkCheckpoint = vi.fn()
  const source = structuredClone(applicationSourceForScenario("complete"))
  const computer = localComputer(source, "dev")
  const user = userEvent.setup()
  render(<MacosComputersContext.Provider value={createFixtureMacosComputersStore()}><OverviewPage source={source} actions={{ forkCheckpoint } as unknown as ApplicationActions} onConfigurationsChange={vi.fn()} /></MacosComputersContext.Provider>)
  await waitFor(() => expect(document.querySelector("[data-macos-computer-id]")).not.toBeNull())

  await user.click(screen.getByRole("button", { name: `More actions for ${computer.configuration.name}` }))
  await user.click(screen.getByRole("menuitem", { name: `Fork ${computer.configuration.name}` }))
  await user.type(await screen.findByRole("textbox", { name: "New computer name" }), "daily")

  expect(screen.getByText("A computer named daily already exists.")).toBeVisible()
  expect(screen.getByRole("button", { name: "Fork" })).toBeDisabled()
  expect(forkCheckpoint).not.toHaveBeenCalled()
})
