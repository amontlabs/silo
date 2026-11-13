import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it, vi } from "vitest"
import { applicationSourceForScenario } from "@/fixtures/application-scenarios"
import type { ApplicationActions } from "../model/application-source"
import type { OperationQueue } from "../model/operation-queue"
import { Toaster } from "@/components/ui/sonner"
import { OverviewPage } from "./overview-page"
import type { ApplicationComputer } from "../model/application-source"

function confirmButton(name: string) { return within(document.querySelector<HTMLElement>("[data-slot=popover-content]")!).getByRole("button", { name }) }

it("closes the fork popover at once and continues current-state fork progress in a notification", async () => {
  let finishFork!: () => void
  const forkCheckpoint = vi.fn(() => new Promise<void>(resolve => { finishFork = resolve }))
  const actions = { forkCheckpoint } as unknown as ApplicationActions
  const source = structuredClone(applicationSourceForScenario("complete"))
  const computer = source.computers[0]!
  const user = userEvent.setup()
  const view = render(<><OverviewPage source={source} actions={actions} onConfigurationsChange={vi.fn()} /><Toaster /></>)

  await user.click(screen.getByRole("button", { name: `More actions for ${computer.configuration.name}` }))
  await user.click(screen.getByRole("menuitem", { name: `Fork ${computer.configuration.name}` }))
  fireEvent.change(await screen.findByRole("textbox", { name: "New computer name" }), { target: { value: "experiment" } })
  await user.click(screen.getByRole("button", { name: "Fork" }))

  await waitFor(() => expect(forkCheckpoint).toHaveBeenCalledWith(computer.configuration.name, null, "experiment"))
  // The popover closed immediately; progress lives in one notification.
  expect(screen.queryByRole("textbox", { name: "New computer name" })).toBeNull()
  expect(await screen.findByText("Creating fork experiment")).toBeVisible()
  expect(screen.getByRole("progressbar", { name: "Copying from the checkpoint" })).toBeVisible()

  const progressing = structuredClone(source)
  const progressingComputer = progressing.computers.find(item => item.configuration.id === computer.configuration.id)!
  progressingComputer.checkpointOperation = { kind: "fork", status: "running", stage: "copying-disk" }
  view.rerender(<><OverviewPage source={progressing} actions={actions} onConfigurationsChange={vi.fn()} /><Toaster /></>)
  expect(await screen.findByText("copying-disk", { selector: "[data-sonner-toast] span" })).toBeVisible()
  expect(screen.getAllByText("Creating fork experiment")).toHaveLength(1)

  await act(async () => { finishFork() })
  expect(await screen.findByText("Fork created")).toBeVisible()
  expect(screen.queryByText("Creating fork experiment")).toBeNull()
})

it("allows current-state Fork for a pending restored computer without starting it", async () => {
  const forkCheckpoint = vi.fn().mockResolvedValue(undefined)
  const startComputer = vi.fn()
  const actions = { forkCheckpoint, startComputer } as unknown as ApplicationActions
  const source = structuredClone(applicationSourceForScenario("complete"))
  const computer = source.computers[0]!
  computer.state = "stopped"
  computer.stateDetail = "Ready to start from checkpoint"
  computer.pendingCheckpointRestore = { checkpointId: "saved-point", sourceComputer: "dev", state: "full" }
  const user = userEvent.setup()
  render(<OverviewPage source={source} actions={actions} onConfigurationsChange={vi.fn()} />)

  await user.click(screen.getByRole("button", { name: `More actions for ${computer.configuration.name}` }))
  await user.click(screen.getByRole("menuitem", { name: `Fork ${computer.configuration.name}` }))
  await user.type(await screen.findByRole("textbox", { name: "New computer name" }), "pending-fork")
  await user.click(screen.getByRole("button", { name: "Fork" }))

  expect(forkCheckpoint).toHaveBeenCalledWith(computer.configuration.name, null, "pending-fork")
  expect(startComputer).not.toHaveBeenCalled()
})

it("shows persisted checkpoint progress and locks the computer row after remount", async () => {
  const source = structuredClone(applicationSourceForScenario("complete"))
  const computer = source.computers[0]!
  computer.checkpointOperation = { kind: "capture", status: "running", stage: "Capturing computer state" }
  render(<OverviewPage source={source} actions={{ forkCheckpoint: vi.fn() } as unknown as ApplicationActions} onConfigurationsChange={vi.fn()} />)

  expect(screen.getByRole("status")).toHaveTextContent("Capturing computer state")
  expect(screen.getByRole("progressbar", { name: "Checkpoint operation progress" })).toBeVisible()
  expect(document.querySelector(`[data-computer-id="${computer.configuration.id}"]`)).toHaveAttribute("aria-busy", "true")
  // The menu stays available for navigation; the items that change the computer are locked.
  await userEvent.setup().click(screen.getByRole("button", { name: `More actions for ${computer.configuration.name}` }))
  for (const name of [`Fork ${computer.configuration.name}`, `Edit ${computer.configuration.name}`, `Delete ${computer.configuration.name}`]) expect(screen.getByRole("menuitem", { name })).toHaveAttribute("data-disabled")
})

it("opens the Checkpoints tab from the Overview menu and drives fork, restore, and create", async () => {
  const createCheckpoint = vi.fn().mockResolvedValue(undefined)
  const forkCheckpoint = vi.fn().mockResolvedValue(undefined)
  const restoreCheckpoint = vi.fn().mockResolvedValue(undefined)
  const actions = { createCheckpoint, forkCheckpoint, restoreCheckpoint } as unknown as ApplicationActions
  const source = structuredClone(applicationSourceForScenario("complete"))
  const computer = source.computers[0]!
  computer.checkpoints = [{ id: "checkpoint-1", name: "Before deploy", createdAt: "2026-09-25T10:00:00.000Z", scope: "full", reason: "manual" }] satisfies NonNullable<ApplicationComputer["checkpoints"]>
  const user = userEvent.setup()
  render(<><OverviewPage source={source} actions={actions} onConfigurationsChange={vi.fn()} /><Toaster /></>)

  await user.click(screen.getByRole("button", { name: `More actions for ${computer.configuration.name}` }))
  await user.click(await screen.findByRole("menuitem", { name: `Checkpoints for ${computer.configuration.name}` }))
  const panel = within(await screen.findByRole("region", { name: `Checkpoints for ${computer.configuration.name}` }))
  const savedRow = within(panel.getByText("Before deploy").closest("[data-checkpoint-name]")!)
  expect(savedRow.getByRole("button", { name: "Restore" })).toBeVisible()

  await user.click(savedRow.getByRole("button", { name: "Checkpoint actions for Before deploy" }))
  await user.click(screen.getByRole("menuitem", { name: "Fork Before deploy" }))
  await user.type(await screen.findByRole("textbox", { name: "New computer name" }), "experiment")
  await user.click(screen.getByRole("button", { name: "Fork" }))
  await waitFor(() => expect(forkCheckpoint).toHaveBeenCalledWith(computer.configuration.name, "checkpoint-1", "experiment"))

  await user.click(savedRow.getByRole("button", { name: "Restore" }))
  await user.click(confirmButton("Restore"))
  await waitFor(() => expect(restoreCheckpoint).toHaveBeenCalledWith(computer.configuration.name, "checkpoint-1"))

  await user.click(panel.getByRole("button", { name: "New checkpoint" }))
  await user.clear(screen.getByRole("textbox", { name: "Checkpoint name" }))
  await user.type(screen.getByRole("textbox", { name: "Checkpoint name" }), "After deploy")
  await user.click(screen.getByRole("button", { name: "Create" }))
  await waitFor(() => expect(createCheckpoint).toHaveBeenCalledWith(computer.configuration.name, "After deploy"))

  // Create progress and completion are notifications, not inline blocks that push the list.
  expect(await screen.findByText("Checkpoint created")).toBeVisible()
  expect(screen.queryByRole("progressbar", { name: "Checkpoint operation progress" })).not.toBeInTheDocument()
})

it("dismisses a computer's notifications when it is deleted and announces the deletion", async () => {
  const source = structuredClone(applicationSourceForScenario("complete"))
  source.devices = []
  const computer = source.computers.find(item => !item.device)!
  computer.state = "stopped"
  const { showOperationSuccess } = await import("@/lib/operation-toast")
  const actions = { forkCheckpoint: vi.fn() } as unknown as ApplicationActions
  const user = userEvent.setup()
  const onConfigurationsChange = vi.fn().mockResolvedValue(undefined)
  const view = render(<><OverviewPage source={source} actions={actions} onConfigurationsChange={onConfigurationsChange} /><Toaster /></>)
  act(() => showOperationSuccess("import-result", `Imported ${computer.configuration.name}`, { computer: computer.configuration.name, action: { label: "Open", onClick: vi.fn() } }))
  expect(await screen.findByText(`Imported ${computer.configuration.name}`)).toBeVisible()

  await user.click(screen.getByRole("button", { name: `More actions for ${computer.configuration.name}` }))
  await user.click(await screen.findByRole("menuitem", { name: `Delete ${computer.configuration.name}` }))
  await user.click(confirmButton("Delete permanently"))
  expect(await screen.findByText(`Deleted ${computer.configuration.name}`)).toBeVisible()

  const remaining = { ...source, computers: source.computers.filter(item => item.configuration.id !== computer.configuration.id) }
  view.rerender(<><OverviewPage source={remaining} actions={actions} onConfigurationsChange={onConfigurationsChange} /><Toaster /></>)
  await waitFor(() => expect(screen.queryByText(`Imported ${computer.configuration.name}`)).not.toBeInTheDocument())
  expect(screen.getByText(`Deleted ${computer.configuration.name}`)).toBeVisible()
})

it("moves Cancel for a running checkpoint capture into its progress notification", async () => {
  const { runCheckpointOperation, syncCheckpointProgress } = await import("../model/checkpoint-operation-toast")
  const source = structuredClone(applicationSourceForScenario("complete"))
  const computer = source.computers.find(item => !item.device)!
  const cancel = vi.fn()
  render(<Toaster />)
  let finish!: () => void
  void runCheckpointOperation({ id: "checkpoint:dev:capture", kind: "capture", target: computer.configuration.name, computer: computer.configuration.name, title: "Creating checkpoint “A”", run: () => new Promise<void>(resolve => { finish = resolve }), success: { title: "Checkpoint created" }, failureTitle: "Could not create checkpoint" })
  computer.checkpointOperation = { kind: "capture", status: "running", stage: "Saving disk copies" }
  const queue: OperationQueue = { running: [{ id: 42, label: "Creating checkpoint", kind: "checkpointCapture", computerId: computer.configuration.id, computerName: computer.configuration.name, sinceMs: Date.now(), cancellable: true, expectedMs: null, blockedByHidden: false }], waiting: [] }
  act(() => syncCheckpointProgress(source.computers, { queue, cancel }))
  const button = await screen.findByRole("button", { name: "Cancel" })
  await userEvent.setup().click(button)
  expect(cancel).toHaveBeenCalledWith(42)
  await act(async () => { finish() })
})
