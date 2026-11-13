import { render, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, expect, it, vi } from "vitest"
import { toast } from "sonner"

import { Toaster } from "@/components/ui/sonner"
import { SettingsProvider } from "@/features/preferences/settings-store"
import { applicationSourceForScenario } from "@/fixtures/application-scenarios"
import type { ApplicationActions, ApplicationSource, ApplicationComputer } from "../model/application-source"
import { OverviewPage } from "./overview-page"

afterEach(() => { toast.dismiss() })

function withCheckpoint() {
  const source = structuredClone(applicationSourceForScenario("complete"))
  source.devices = []
  const computer = source.computers.find(item => !item.device)!
  computer.checkpoints = [{ id: "checkpoint-1", name: "Before deploy", createdAt: "2026-09-25T10:00:00.000Z", scope: "full", reason: "manual" }] satisfies NonNullable<ApplicationComputer["checkpoints"]>
  return { source, computer }
}

function Harness({ source, actions }: { source: ApplicationSource; actions: ApplicationActions }) {
  return <SettingsProvider initialSettings={{ theme: "light" }}>
    <Toaster />
    <OverviewPage source={source} actions={actions} onConfigurationsChange={vi.fn()} />
  </SettingsProvider>
}

function confirmButton(name: string) { return within(document.querySelector<HTMLElement>("[data-slot=popover-content]")!).getByRole("button", { name }) }

it("toasts a created fork with an Open action that navigates to the new computer", async () => {
  const { source, computer } = withCheckpoint()
  const forkCheckpoint = vi.fn().mockResolvedValue(undefined)
  const actions = { forkCheckpoint, restoreCheckpoint: vi.fn(), openTerminal: vi.fn() } as unknown as ApplicationActions
  const user = userEvent.setup()
  const view = render(<Harness source={source} actions={actions} />)

  await user.click(screen.getByRole("button", { name: `More actions for ${computer.configuration.name}` }))
  await user.click(await screen.findByRole("menuitem", { name: `Checkpoints for ${computer.configuration.name}` }))
  await user.click(within((await screen.findByText("Before deploy")).closest("[data-checkpoint-name]")!).getByRole("button", { name: "Checkpoint actions for Before deploy" }))
  await user.click(await screen.findByRole("menuitem", { name: "Fork Before deploy" }))
  expect(await screen.findByText("Fork from “Before deploy”")).toBeVisible()
  await user.type(await screen.findByRole("textbox", { name: "New computer name" }), "experiment")
  await user.click(await screen.findByRole("button", { name: "Fork" }))

  await waitFor(() => expect(forkCheckpoint).toHaveBeenCalledWith(computer.configuration.name, "checkpoint-1", "experiment"))
  expect(await screen.findByText("Fork created")).toBeVisible()
  expect(await screen.findByText(/experiment is stopped/)).toBeVisible()

  // The backend snapshot now includes the forked computer; Open resolves it fresh and navigates.
  const forked = structuredClone(source)
  const clone = structuredClone(computer)
  clone.configuration = { ...clone.configuration, id: "vm-experiment", name: "experiment" }
  clone.checkpoints = []
  forked.computers.push(clone)
  view.rerender(<Harness source={forked} actions={actions} />)

  await user.click(await screen.findByRole("button", { name: "Open" }))
  await waitFor(() => expect(screen.getByRole("navigation", { name: "Breadcrumb" })).toHaveTextContent("Computersexperiment"))
})

it("toasts a created fork from the computer current-state menu", async () => {
  const { source, computer } = withCheckpoint()
  const forkCheckpoint = vi.fn().mockResolvedValue(undefined)
  const actions = { forkCheckpoint } as unknown as ApplicationActions
  const user = userEvent.setup()
  render(<Harness source={source} actions={actions} />)

  await user.click(screen.getByRole("button", { name: `More actions for ${computer.configuration.name}` }))
  await user.click(await screen.findByRole("menuitem", { name: `Fork ${computer.configuration.name}` }))
  await user.type(await screen.findByRole("textbox", { name: "New computer name" }), "spinoff")
  await user.click(await screen.findByRole("button", { name: "Fork" }))

  await waitFor(() => expect(forkCheckpoint).toHaveBeenCalledWith(computer.configuration.name, null, "spinoff"))
  expect(await screen.findByText("Fork created")).toBeVisible()
  expect(await screen.findByText(/spinoff is stopped/)).toBeVisible()
})

it("toasts a restored checkpoint with a Start action that runs the guarded start", async () => {
  const { source, computer } = withCheckpoint()
  const restoreCheckpoint = vi.fn().mockResolvedValue(undefined)
  const startComputer = vi.fn()
  const actions = { restoreCheckpoint, forkCheckpoint: vi.fn(), startComputer, openTerminal: vi.fn() } as unknown as ApplicationActions
  const user = userEvent.setup()
  const view = render(<Harness source={source} actions={actions} />)

  await user.click(screen.getByRole("button", { name: `More actions for ${computer.configuration.name}` }))
  await user.click(await screen.findByRole("menuitem", { name: `Checkpoints for ${computer.configuration.name}` }))
  await user.click(within((await screen.findByText("Before deploy")).closest("[data-checkpoint-name]")!).getByRole("button", { name: "Restore" }))
  expect(await screen.findByText("Restore “Before deploy”?")).toBeVisible()
  await user.click(confirmButton("Restore"))

  await waitFor(() => expect(restoreCheckpoint).toHaveBeenCalledWith(computer.configuration.name, "checkpoint-1"))
  expect(await screen.findByText("Restored “Before deploy”")).toBeVisible()
  expect(await screen.findByText(new RegExp(`${computer.configuration.name} is stopped\\. A recovery checkpoint was saved first\\.`))).toBeVisible()

  // The snapshot then reports the restored computer stopped; Start acts on that state.
  view.rerender(<Harness source={{ ...source, computers: source.computers.map(item => item === computer ? { ...item, state: "stopped" as const } : item) }} actions={actions} />)
  await user.click(await screen.findByRole("button", { name: "Start" }))
  expect(startComputer).toHaveBeenCalledWith(computer.configuration.name)
})

it("routes the restored-toast Start through the guard, so an unavailable computer operation is blocked", async () => {
  const { source, computer } = withCheckpoint()
  source.computerOperationsUnavailable = "Local computers are unavailable while the runtime is repairing."
  const restoreCheckpoint = vi.fn().mockResolvedValue(undefined)
  const startComputer = vi.fn()
  const actions = { restoreCheckpoint, forkCheckpoint: vi.fn(), startComputer, openTerminal: vi.fn() } as unknown as ApplicationActions
  const user = userEvent.setup()
  render(<Harness source={source} actions={actions} />)

  await user.click(screen.getByRole("button", { name: `More actions for ${computer.configuration.name}` }))
  await user.click(await screen.findByRole("menuitem", { name: `Checkpoints for ${computer.configuration.name}` }))
  await user.click(within((await screen.findByText("Before deploy")).closest("[data-checkpoint-name]")!).getByRole("button", { name: "Restore" }))
  await user.click(confirmButton("Restore"))

  await screen.findByText("Restored “Before deploy”")
  await user.click(await screen.findByRole("button", { name: "Start" }))
  expect(startComputer).not.toHaveBeenCalled()
  expect(await screen.findByText("Computer operation unavailable")).toBeVisible()
})
