import { fireEvent, render, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { toast } from "sonner"
import { afterEach, expect, it, vi } from "vitest"

import { Toaster } from "@/components/ui/sonner"
import { TooltipProvider } from "@/components/ui/tooltip"
import { createFixtureMacosComputersBackend, macosComputerFixtures } from "@/fixtures/macos-computers"
import { SettingsProvider } from "@/features/preferences/settings-store"
import { createMacosComputersStore, MacosComputersContext, useMacosComputers, type MacosComputer } from "../model/macos-computers"
import { MacosComputerRow } from "./macos-computer-row"

afterEach(() => { toast.dismiss() })

function Rows({ names }: { names: readonly string[] }) {
  const macos = useMacosComputers()!
  return <ul>{(macos.snapshot.state?.computers ?? []).map(computer => <MacosComputerRow key={computer.id} computer={computer} store={macos.store} takenNames={names} />)}</ul>
}

function renderRows(computers: readonly MacosComputer[] = macosComputerFixtures, names: readonly string[] = ["xcode-build"]) {
  const base = createFixtureMacosComputersBackend(computers)
  const backend = {
    ...base,
    action: vi.fn(base.action),
    createCheckpoint: vi.fn(base.createCheckpoint),
    restoreCheckpoint: vi.fn(base.restoreCheckpoint),
    forkCheckpoint: vi.fn(base.forkCheckpoint),
    deleteCheckpoint: vi.fn(base.deleteCheckpoint),
  }
  const store = createMacosComputersStore(backend)
  render(<SettingsProvider initialSettings={{ theme: "light" }}><Toaster /><TooltipProvider><MacosComputersContext.Provider value={store}><Rows names={names} /></MacosComputersContext.Provider></TooltipProvider></SettingsProvider>)
  return backend
}

const stoppedRow = () => document.querySelector<HTMLElement>('[data-macos-computer-id="mac-stopped"]')!
const popoverButton = (name: string) => within(document.querySelector<HTMLElement>("[data-slot=popover-content]")!).getByRole("button", { name })

async function openCheckpoints(user: ReturnType<typeof userEvent.setup>, computer = "xcode-build") {
  await user.click(await screen.findByRole("button", { name: `Checkpoints of ${computer}` }))
  return await screen.findByRole("region", { name: `Checkpoints for ${computer}` })
}

it("offers checkpoints only on computers that have finished setting up", async () => {
  renderRows()
  await screen.findByText("xcode-build")
  expect(screen.getByRole("button", { name: "Checkpoints of xcode-build" })).toHaveAttribute("aria-expanded", "false")
  expect(screen.getByRole("button", { name: "Checkpoints of daily" })).toBeVisible()
  for (const name of ["sequoia-test", "fast-one", "release-check", "agent-box", "broken"]) {
    expect(screen.queryByRole("button", { name: `Checkpoints of ${name}` })).not.toBeInTheDocument()
  }
})

it("lists the saved checkpoints newest first with what each includes", async () => {
  const user = userEvent.setup()
  renderRows()
  const panel = await openCheckpoints(user)
  expect(screen.getByRole("button", { name: "Checkpoints of xcode-build" })).toHaveAttribute("aria-expanded", "true")
  expect([...panel.querySelectorAll("[data-checkpoint-name]")].map(row => row.getAttribute("data-checkpoint-name"))).toEqual(["Xcode installed", "Fresh install"])
  expect(within(panel.querySelector<HTMLElement>('[data-checkpoint-name="Xcode installed"]')!).getByText(/Includes memory/)).toBeVisible()
  expect(within(panel.querySelector<HTMLElement>('[data-checkpoint-name="Fresh install"]')!).getByText(/Disks only/)).toBeVisible()
  await user.click(screen.getByRole("button", { name: "Checkpoints of xcode-build" }))
  expect(screen.queryByRole("region", { name: "Checkpoints for xcode-build" })).not.toBeInTheDocument()
})

it("creates a checkpoint of the computer, naming it", async () => {
  const user = userEvent.setup()
  const backend = renderRows()
  const panel = await openCheckpoints(user)
  await user.click(within(panel).getByRole("button", { name: "New checkpoint" }))
  fireEvent.change(screen.getByRole("textbox", { name: "Checkpoint name" }), { target: { value: "Before update" } })
  await user.click(popoverButton("Create"))
  await waitFor(() => expect(backend.createCheckpoint).toHaveBeenCalledExactlyOnceWith("mac-stopped", "Before update"))
  await waitFor(() => expect(within(stoppedRow()).getByText("Before update")).toBeVisible())
})

it("restores a checkpoint, then says the next Start continues from it", async () => {
  const user = userEvent.setup()
  const backend = renderRows()
  const panel = await openCheckpoints(user)
  const target = panel.querySelector<HTMLElement>('[data-checkpoint-name="Xcode installed"]')!
  await user.click(within(target).getByRole("button", { name: "Restore" }))
  expect(document.querySelector("[data-slot=popover-content]")).toHaveTextContent("recovery checkpoint")
  await user.click(popoverButton("Restore"))
  await waitFor(() => expect(backend.restoreCheckpoint).toHaveBeenCalledExactlyOnceWith("mac-stopped", "00000000-0000-4000-8000-000000000002"))
  expect(await within(stoppedRow()).findByText(/Restored to “Xcode installed”/)).toHaveTextContent("saved memory")
  expect(within(stoppedRow()).getByText("Before restore")).toBeVisible()
})

it("forks a checkpoint under a name no computer on the device has", async () => {
  const user = userEvent.setup()
  const backend = renderRows()
  const panel = await openCheckpoints(user)
  await user.click(within(panel).getByRole("button", { name: "Checkpoint actions for Fresh install" }))
  await user.click(screen.getByRole("menuitem", { name: "Fork Fresh install" }))
  expect(document.querySelector("[data-slot=popover-content]")).toHaveTextContent("Memory can’t move to a new computer")
  const name = screen.getByRole("textbox", { name: "New computer name" })
  fireEvent.change(name, { target: { value: "xcode-build" } })
  expect(name).toHaveAttribute("aria-invalid", "true")
  expect(popoverButton("Fork")).toBeDisabled()
  fireEvent.change(name, { target: { value: "build-two" } })
  await user.click(popoverButton("Fork"))
  await waitFor(() => expect(backend.forkCheckpoint).toHaveBeenCalledExactlyOnceWith("mac-stopped", "00000000-0000-4000-8000-000000000001", "build-two"))
  expect(await screen.findByText("build-two")).toBeVisible()
  expect(await screen.findByText(/being set up/)).toBeVisible()
})

it("deletes a checkpoint after confirmation", async () => {
  const user = userEvent.setup()
  const backend = renderRows()
  const panel = await openCheckpoints(user)
  await user.click(within(panel).getByRole("button", { name: "Checkpoint actions for Fresh install" }))
  await user.click(screen.getByRole("menuitem", { name: "Delete Fresh install" }))
  await user.click(popoverButton("Delete"))
  await waitFor(() => expect(backend.deleteCheckpoint).toHaveBeenCalledExactlyOnceWith("mac-stopped", "00000000-0000-4000-8000-000000000001"))
  await waitFor(() => expect(panel.querySelector('[data-checkpoint-name="Fresh install"]')).toBeNull())
})

it("reports a failed checkpoint operation and keeps the list", async () => {
  const user = userEvent.setup()
  const backend = renderRows()
  backend.createCheckpoint.mockRejectedValueOnce(new Error("This computer's memory can't be saved (audio). Stop it to save a checkpoint of its disk."))
  const panel = await openCheckpoints(user, "daily")
  await user.click(within(panel).getByRole("button", { name: "New checkpoint" }))
  fireEvent.change(screen.getByRole("textbox", { name: "Checkpoint name" }), { target: { value: "Live" } })
  await user.click(popoverButton("Create"))
  expect(await screen.findByText(/Stop it to save a checkpoint of its disk/)).toBeVisible()
  expect(screen.getByText("No checkpoints yet")).toBeVisible()
})

it("locks the computer while a checkpoint operation runs on it", async () => {
  const user = userEvent.setup()
  const busy: MacosComputer = { ...macosComputerFixtures.find(computer => computer.id === "mac-running")!, checkpointOperation: { kind: "capture", status: "running", stage: "Copying the disk" } }
  renderRows([busy])
  await screen.findByText("daily")
  const row = document.querySelector<HTMLElement>('[data-macos-computer-id="mac-running"]')!
  expect(row).toHaveAttribute("aria-busy", "true")
  expect(within(row).getByText("Copying the disk…")).toBeVisible()
  expect(within(row).getByRole("button", { name: "Stop daily" })).toBeDisabled()
  const panel = await openCheckpoints(user, "daily")
  expect(within(panel).getByRole("button", { name: "New checkpoint" })).toBeDisabled()
  await user.click(within(row).getByRole("button", { name: "More actions for daily" }))
  expect(screen.queryByRole("menuitem", { name: "Delete daily" })).not.toBeInTheDocument()
})

it("keeps a stopped computer's Start disabled during an operation", async () => {
  const busy: MacosComputer = { ...macosComputerFixtures.find(computer => computer.id === "mac-stopped")!, checkpointOperation: { kind: "restore", status: "running", stage: "Saving a recovery checkpoint" } }
  renderRows([busy])
  expect(await screen.findByRole("button", { name: "Start xcode-build" })).toBeDisabled()
})
