import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, expect, it, vi } from "vitest"
import { toast } from "sonner"
import { Toaster } from "@/components/ui/sonner"
import { SettingsProvider } from "@/features/preferences/settings-store"
import type { ApplicationActions, ApplicationComputer } from "@/features/application/model/application-source"
import { CheckpointPanel } from "./checkpoint-panel"

const computer = {
  configuration: { id: "vm-dev", name: "dev" },
  checkpoints: [
    { id: "point-1", name: "Before refactor", createdAt: "2026-09-25T10:00:00Z", scope: "full", reason: "manual" },
    { id: "point-2", name: "Disk snapshot", createdAt: "2026-09-24T10:00:00Z", scope: "disk", reason: "manual" },
    { id: "point-3", name: "Before restore", createdAt: "2026-09-23T10:00:00Z", scope: "full", reason: "before-restore" },
  ],
} as ApplicationComputer

afterEach(() => { toast.dismiss() })
function withToaster(node: React.ReactNode) { return <SettingsProvider initialSettings={{ theme: "light" }}><Toaster />{node}</SettingsProvider> }

function deferred() {
  let resolve!: () => void
  let reject!: (error: Error) => void
  const promise = new Promise<void>((yes, no) => { resolve = yes; reject = no })
  return { promise, resolve, reject }
}

function confirmButton(name: string) { return within(document.querySelector<HTMLElement>("[data-slot=popover-content]")!).getByRole("button", { name }) }

it("orders checkpoint history newest first without reordering the supplied snapshot", () => {
  const checkpoints = [computer.checkpoints![2], computer.checkpoints![0], computer.checkpoints![1]]
  render(<CheckpointPanel computer={{ ...computer, checkpoints }} target="dev" actions={{} as ApplicationActions} disabled={false} />)
  expect([...document.querySelectorAll('[data-checkpoint-name]')].map(row => row.getAttribute("data-checkpoint-name"))).toEqual(["Before refactor", "Disk snapshot", "Before restore"])
  expect(checkpoints.map(checkpoint => checkpoint.id)).toEqual(["point-3", "point-1", "point-2"])
})

it("blocks a conflicting fork name and submits the selected checkpoint and trimmed name", async () => {
  const user = userEvent.setup()
  const forkCheckpoint = vi.fn().mockResolvedValue(undefined)
  const target = "silo-remote:office:vm-dev"
  const remote = { ...computer, device: { id: "office", computerId: "vm-dev", name: "Office", address: "office", connected: true } }
  render(withToaster(<CheckpointPanel computer={remote} target={target} actions={{ forkCheckpoint } as unknown as ApplicationActions} disabled={false} takenNames={["existing"]} />))
  await user.click(screen.getByRole("button", { name: "Checkpoint actions for Disk snapshot" }))
  await user.click(screen.getByRole("menuitem", { name: "Fork Disk snapshot" }))
  const name = screen.getByRole("textbox", { name: "New computer name" })
  fireEvent.change(name, { target: { value: "existing" } })
  expect(name).toHaveAttribute("aria-invalid", "true")
  expect(confirmButton("Fork")).toBeDisabled()
  expect(forkCheckpoint).not.toHaveBeenCalled()
  fireEvent.change(name, { target: { value: "  new-fork  " } })
  await user.click(confirmButton("Fork"))
  await waitFor(() => expect(forkCheckpoint).toHaveBeenCalledExactlyOnceWith(target, "point-2", "new-fork"))
})

it("ignores a previous computer's late usage response when the selected computer changes", async () => {
  const user = userEvent.setup()
  let finish!: (value: typeof usage) => void
  const readCheckpointUsage = vi.fn().mockImplementationOnce(() => new Promise(resolve => { finish = resolve }))
    .mockResolvedValueOnce({ totalBytes: gib, checkpoints: [{ id: "point-1", sizeBytes: gib }] })
  const actions = { readCheckpointUsage, deleteCheckpoint: vi.fn() } as unknown as ApplicationActions
  const view = render(<CheckpointPanel computer={computer} target="dev" actions={actions} disabled={false} />)
  const other = { ...computer, configuration: { ...computer.configuration, id: "vm-other", name: "other" } }
  view.rerender(<CheckpointPanel computer={other} target="other" actions={actions} disabled={false} />)
  const row = within(screen.getByText("Before refactor").closest("[data-checkpoint-name]")!)
  expect(await row.findByText(/1\.00 GiB/)).toBeVisible()
  await act(async () => finish(usage))
  expect(readCheckpointUsage.mock.calls).toEqual([["vm-dev"], ["vm-other"]])
  expect(row.getByText(/1\.00 GiB/)).toBeVisible()
  expect(row.queryByText(/Used by experiment/)).not.toBeInTheDocument()
  await user.click(row.getByRole("button", { name: "Checkpoint actions for Before refactor" }))
  expect(screen.getByRole("menuitem", { name: "Delete Before refactor" })).not.toHaveAttribute("data-disabled")
})

it("shows a Restore button and a menu with Fork and Export on a local checkpoint", async () => {
  const onExport = vi.fn()
  const user = userEvent.setup()
  render(<CheckpointPanel computer={computer} target="dev" actions={{ forkCheckpoint: vi.fn(), restoreCheckpoint: vi.fn() } as unknown as ApplicationActions} disabled={false} onExport={onExport} />)
  const row = within(screen.getByText("Before refactor").closest("[data-checkpoint-name]")!)
  expect(row.getByRole("button", { name: "Restore" })).toBeVisible()
  await user.click(row.getByRole("button", { name: "Checkpoint actions for Before refactor" }))
  expect(screen.getByRole("menuitem", { name: "Fork Before refactor" })).toBeVisible()
  await user.click(screen.getByRole("menuitem", { name: "Export Before refactor" }))
  expect(onExport).toHaveBeenCalledWith(computer.checkpoints![0])
})

it("labels checkpoint scope and recovery with a short tag", () => {
  render(<CheckpointPanel computer={computer} target="dev" actions={{} as ApplicationActions} disabled={false} />)
  expect(within(screen.getByText("Before refactor").closest("[data-checkpoint-name]")!).getByText(/Includes memory/)).toBeVisible()
  expect(within(screen.getByText("Disk snapshot").closest("[data-checkpoint-name]")!).getByText(/Disks only/)).toBeVisible()
  expect(within(screen.getByText("Before restore").closest("[data-checkpoint-name]")!).getByText(/Recovery/)).toBeVisible()
})

it("hides Export for a remote computer checkpoint", async () => {
  const remote = { ...computer, device: { id: "mac", name: "Ada’s Mac mini", connected: true } } as ApplicationComputer
  const user = userEvent.setup()
  render(<CheckpointPanel computer={remote} target="dev" actions={{ forkCheckpoint: vi.fn(), restoreCheckpoint: vi.fn() } as unknown as ApplicationActions} disabled={false} onExport={vi.fn()} />)
  await user.click(within(screen.getByText("Before refactor").closest("[data-checkpoint-name]")!).getByRole("button", { name: "Checkpoint actions for Before refactor" }))
  expect(screen.queryByRole("menuitem", { name: "Export Before refactor" })).toBeNull()
  expect(screen.getByRole("menuitem", { name: "Fork Before refactor" })).toBeVisible()
})

it("disables Export while a transfer is running", async () => {
  const user = userEvent.setup()
  render(<CheckpointPanel computer={computer} target="dev" actions={{ forkCheckpoint: vi.fn(), restoreCheckpoint: vi.fn() } as unknown as ApplicationActions} disabled={false} onExport={vi.fn()} exportDisabled />)
  await user.click(within(screen.getByText("Before refactor").closest("[data-checkpoint-name]")!).getByRole("button", { name: "Checkpoint actions for Before refactor" }))
  expect(screen.getByRole("menuitem", { name: "Export Before refactor" })).toHaveAttribute("aria-disabled", "true")
})

it("explains the pause, memory checkpoint and force-stop before restoring a running computer", async () => {
  const user = userEvent.setup()
  render(<CheckpointPanel computer={{ ...computer, state: "running" }} target="dev" actions={{ restoreCheckpoint: vi.fn() } as unknown as ApplicationActions} disabled={false} />)
  await user.click(within(screen.getByText("Before refactor").closest("[data-checkpoint-name]")!).getByRole("button", { name: "Restore" }))
  const description = await screen.findByText(/dev is running/)
  expect(description).toHaveTextContent(/pauses it/)
  expect(description).toHaveTextContent(/includes its memory/)
  expect(description).toHaveTextContent(/force-stops it/)
  expect(description).toHaveTextContent(/not undone/)
})

it("explains Restore and Fork in visible helper text", () => {
  render(<CheckpointPanel computer={computer} target="dev" actions={{} as ApplicationActions} disabled={false} />)
  expect(screen.getByText("Checkpoints let you rewind this computer. Restore replaces its current files; Fork creates a new stopped computer with a copy of its files.")).toBeVisible()
})

it("confirms a restore in a popover before restoring", async () => {
  const restoreCheckpoint = vi.fn().mockResolvedValue(undefined)
  const user = userEvent.setup()
  render(<CheckpointPanel computer={computer} target="dev" actions={{ restoreCheckpoint } as unknown as ApplicationActions} disabled={false} />)
  await user.click(within(screen.getByText("Before refactor").closest("[data-checkpoint-name]")!).getByRole("button", { name: "Restore" }))
  expect(await screen.findByText("Restore “Before refactor”?")).toBeVisible()
  expect(screen.getByText(/saves a recovery checkpoint first/i)).toBeVisible()
  expect(restoreCheckpoint).not.toHaveBeenCalled()
  await user.click(screen.getByRole("button", { name: "Cancel" }))
  expect(screen.queryByText("Restore “Before refactor”?")).toBeNull()
  await user.click(within(screen.getByText("Before refactor").closest("[data-checkpoint-name]")!).getByRole("button", { name: "Restore" }))
  await user.click(confirmButton("Restore"))
  await waitFor(() => expect(restoreCheckpoint).toHaveBeenCalledWith("dev", "point-1"))
})

it("names a stopped fork from the selected checkpoint", async () => {
  const forkCheckpoint = vi.fn().mockResolvedValue(undefined)
  const user = userEvent.setup()
  render(<CheckpointPanel computer={computer} target="dev" actions={{ forkCheckpoint } as unknown as ApplicationActions} disabled={false} />)
  await user.click(within(screen.getByText("Before refactor").closest("[data-checkpoint-name]")!).getByRole("button", { name: "Checkpoint actions for Before refactor" }))
  await user.click(screen.getByRole("menuitem", { name: "Fork Before refactor" }))
  expect(await screen.findByText("Fork from “Before refactor”")).toBeVisible()
  await user.type(await screen.findByRole("textbox", { name: "New computer name" }), "experiment")
  await user.click(screen.getByRole("button", { name: "Fork" }))
  await waitFor(() => expect(forkCheckpoint).toHaveBeenCalledWith("dev", "point-1", "experiment"))
})

it("creates a checkpoint from a prefilled name in a popover", async () => {
  const createCheckpoint = vi.fn().mockResolvedValue(undefined)
  const user = userEvent.setup()
  render(<CheckpointPanel computer={computer} target="dev" actions={{ createCheckpoint } as unknown as ApplicationActions} disabled={false} />)
  await user.click(screen.getByRole("button", { name: "New checkpoint" }))
  const input = screen.getByRole("textbox", { name: "Checkpoint name" })
  expect((input as HTMLInputElement).value).toMatch(/^Checkpoint /)
  await user.clear(input)
  await user.type(input, "before deploy")
  await user.click(screen.getByRole("button", { name: "Create" }))
  await waitFor(() => expect(createCheckpoint).toHaveBeenCalledWith("dev", "before deploy"))
})

it("shows a calm empty state with the New checkpoint button", () => {
  const empty = { ...computer, checkpoints: [] } as ApplicationComputer
  render(<CheckpointPanel computer={empty} target="dev" actions={{ createCheckpoint: vi.fn() } as unknown as ApplicationActions} disabled={false} />)
  expect(screen.getByText("No checkpoints yet")).toBeVisible()
  expect(screen.getByRole("button", { name: "New checkpoint" })).toBeVisible()
})

it("notes a failed operation from before this session inline without a notification", () => {
  const failed = { ...computer, checkpointOperation: { kind: "capture", status: "failed", stage: "Capture failed", error: "Disk is full" } } as ApplicationComputer
  render(withToaster(<CheckpointPanel computer={failed} target="dev" actions={{ createCheckpoint: vi.fn() } as unknown as ApplicationActions} disabled={false} />))
  expect(screen.getByText("Disk is full")).toBeVisible()
  expect(document.querySelector("[data-sonner-toast]")).toBeNull()
})

it("notifies about a failed create with Retry instead of inserting an error", async () => {
  const createCheckpoint = vi.fn().mockRejectedValueOnce(new Error("Disk is full")).mockResolvedValue(undefined)
  const user = userEvent.setup()
  render(withToaster(<CheckpointPanel computer={computer} target="dev" actions={{ createCheckpoint } as unknown as ApplicationActions} disabled={false} />))
  await user.click(screen.getByRole("button", { name: "New checkpoint" }))
  await user.clear(screen.getByRole("textbox", { name: "Checkpoint name" }))
  await user.type(screen.getByRole("textbox", { name: "Checkpoint name" }), "before deploy")
  await user.click(screen.getByRole("button", { name: "Create" }))
  expect(await screen.findByText("Disk is full")).toBeInTheDocument()
  expect(screen.getByText("Could not create checkpoint “before deploy”")).toBeInTheDocument()
  expect(screen.queryByRole("alert")).toBeNull()
  await user.click(screen.getByRole("button", { name: "Retry" }))
  expect(await screen.findByText("Checkpoint created")).toBeInTheDocument()
  expect(createCheckpoint).toHaveBeenCalledTimes(2)
})

it("shows a loading notification while a create is pending and marks the region busy", async () => {
  const task = deferred()
  const createCheckpoint = vi.fn(() => task.promise)
  const user = userEvent.setup()
  render(withToaster(<CheckpointPanel computer={computer} target="dev" actions={{ createCheckpoint } as unknown as ApplicationActions} disabled={false} />))
  await user.click(screen.getByRole("button", { name: "New checkpoint" }))
  await user.clear(screen.getByRole("textbox", { name: "Checkpoint name" }))
  await user.type(screen.getByRole("textbox", { name: "Checkpoint name" }), "before deploy")
  await user.click(screen.getByRole("button", { name: "Create" }))
  expect(await screen.findByText("Creating checkpoint “before deploy”")).toBeInTheDocument()
  expect(screen.getByRole("region", { name: "Checkpoints for dev" })).toHaveAttribute("aria-busy", "true")
  expect(screen.getByRole("button", { name: "New checkpoint" })).toBeDisabled()
  task.resolve()
  expect(await screen.findByText("Checkpoint created")).toBeInTheDocument()
  await waitFor(() => expect(screen.getByRole("region", { name: "Checkpoints for dev" })).not.toHaveAttribute("aria-busy"))
})

it("reports a restore as one progress notification that turns into the result, with no dialog", async () => {
  const task = deferred()
  const restoreCheckpoint = vi.fn(() => task.promise)
  const user = userEvent.setup()
  render(withToaster(<CheckpointPanel computer={computer} target="dev" actions={{ restoreCheckpoint } as unknown as ApplicationActions} disabled={false} restoredAction={() => ({ label: "Start", onClick: vi.fn() })} />))
  await user.click(within(screen.getByText("Before refactor").closest("[data-checkpoint-name]")!).getByRole("button", { name: "Restore" }))
  await user.click(within(document.querySelector<HTMLElement>("[data-slot=popover-content]")!).getByRole("button", { name: "Restore" }))
  expect(await screen.findByText("Restoring “Before refactor”")).toBeVisible()
  expect(screen.getByRole("list", { name: "Steps" })).toHaveTextContent("Save recovery checkpoint")
  expect(screen.queryByRole("dialog")).toBeNull()
  task.resolve()
  expect(await screen.findByText("Restored “Before refactor”")).toBeVisible()
  expect(screen.queryByText("Restoring “Before refactor”")).toBeNull()
  expect(screen.getByRole("button", { name: "Start" })).toBeVisible()
})

it("keeps a failed restore in its notification with Retry", async () => {
  const restoreCheckpoint = vi.fn().mockRejectedValueOnce(new Error("Disk is busy")).mockResolvedValue(undefined)
  const user = userEvent.setup()
  render(withToaster(<CheckpointPanel computer={computer} target="dev" actions={{ restoreCheckpoint } as unknown as ApplicationActions} disabled={false} />))
  await user.click(within(screen.getByText("Before refactor").closest("[data-checkpoint-name]")!).getByRole("button", { name: "Restore" }))
  await user.click(within(document.querySelector<HTMLElement>("[data-slot=popover-content]")!).getByRole("button", { name: "Restore" }))
  expect(await screen.findByText("Disk is busy")).toBeVisible()
  await user.click(screen.getByRole("button", { name: "Retry" }))
  expect(await screen.findByText("Restored “Before refactor”")).toBeInTheDocument()
  expect(restoreCheckpoint).toHaveBeenCalledTimes(2)
})

it("suggests an English checkpoint name", async () => {
  const user = userEvent.setup()
  render(<CheckpointPanel computer={computer} target="dev" actions={{ createCheckpoint: vi.fn() } as unknown as ApplicationActions} disabled={false} />)
  await user.click(screen.getByRole("button", { name: "New checkpoint" }))
  const value = (screen.getByRole("textbox", { name: "Checkpoint name" }) as HTMLInputElement).value
  expect(value).toMatch(/^Checkpoint [A-Z][a-z]{2} \d{1,2}, \d{2}:\d{2}$/)
})

const gib = 1024 ** 3
const usage = {
  totalBytes: 6 * gib,
  checkpoints: [
    { id: "point-1", sizeBytes: 4 * gib, usedBy: ["experiment"], deleteBlocker: "Used by experiment. experiment was started from this checkpoint and still builds on it." },
    { id: "point-2", sizeBytes: gib },
    { id: "point-3", sizeBytes: gib },
  ],
}

it("confirms Delete in destructive style, deletes in a notification, and refreshes sizes", async () => {
  const deleteCheckpoint = vi.fn().mockResolvedValue(undefined)
  const readCheckpointUsage = vi.fn().mockResolvedValue(usage)
  const user = userEvent.setup()
  render(withToaster(<CheckpointPanel computer={computer} target="dev" actions={{ deleteCheckpoint, readCheckpointUsage } as unknown as ApplicationActions} disabled={false} />))
  const row = within(screen.getByText("Before restore").closest("[data-checkpoint-name]")!)
  expect(await row.findByText(/1\.00 GiB/)).toBeVisible()
  expect(readCheckpointUsage).toHaveBeenCalledWith("vm-dev")

  await user.click(row.getByRole("button", { name: "Checkpoint actions for Before restore" }))
  await user.click(screen.getByRole("menuitem", { name: "Delete Before restore" }))
  expect(screen.getByText("Delete “Before restore”?")).toBeVisible()
  expect(screen.getByText(/freeing up to 1\.00 GiB/)).toBeVisible()
  expect(screen.getByText(/can no longer undo the Restore/)).toBeVisible()
  const confirm = confirmButton("Delete")
  expect(confirm).toHaveAttribute("data-variant", "destructive")
  await user.click(confirm)

  await waitFor(() => expect(deleteCheckpoint).toHaveBeenCalledWith("dev", "point-3"))
  expect(await screen.findByText("Checkpoint deleted")).toBeVisible()
  await waitFor(() => expect(readCheckpointUsage).toHaveBeenCalledTimes(2))
})

it("shows who uses a pinned checkpoint and keeps its Delete unavailable with the reason", async () => {
  const deleteCheckpoint = vi.fn()
  const user = userEvent.setup()
  render(<CheckpointPanel computer={computer} target="dev" actions={{ deleteCheckpoint, readCheckpointUsage: vi.fn().mockResolvedValue(usage) } as unknown as ApplicationActions} disabled={false} />)
  const row = within(screen.getByText("Before refactor").closest("[data-checkpoint-name]")!)
  expect(await row.findByText("Used by experiment. experiment was started from this checkpoint and still builds on it.")).toBeVisible()
  await user.click(row.getByRole("button", { name: "Checkpoint actions for Before refactor" }))
  const item = screen.getByRole("menuitem", { name: "Delete Before refactor" })
  expect(item).toHaveAttribute("data-disabled")
  expect(screen.getByLabelText(/experiment was started from this checkpoint/)).toBeInTheDocument()
  expect(deleteCheckpoint).not.toHaveBeenCalled()
})

it("reports a refused delete and keeps the checkpoint", async () => {
  const deleteCheckpoint = vi.fn().mockRejectedValue(new Error("“After deploy” was saved after this checkpoint and builds on it. Delete it first."))
  const user = userEvent.setup()
  render(withToaster(<CheckpointPanel computer={computer} target="dev" actions={{ deleteCheckpoint } as unknown as ApplicationActions} disabled={false} />))
  const row = within(screen.getByText("Disk snapshot").closest("[data-checkpoint-name]")!)
  await user.click(row.getByRole("button", { name: "Checkpoint actions for Disk snapshot" }))
  await user.click(screen.getByRole("menuitem", { name: "Delete Disk snapshot" }))
  expect(screen.getByText(/This can’t be undone/)).toBeVisible()
  await user.click(confirmButton("Delete"))
  expect(await screen.findByText("Could not delete “Disk snapshot”")).toBeVisible()
  expect(screen.getByText(/Delete it first/)).toBeVisible()
  expect(screen.getByText("Disk snapshot")).toBeVisible()
})

it("explains an unfinished Restore, names its checkpoint, and offers Retry and Abandon", async () => {
  const restoreCheckpoint = vi.fn().mockResolvedValue(undefined)
  const abandonRestore = vi.fn().mockResolvedValue(undefined)
  const unfinished = { ...computer, unfinishedRestore: { checkpointId: "point-2", checkpointName: "Disk snapshot", phase: "capturing" }, checkpointOperation: { kind: "restore", status: "failed", stage: "Recovery checkpoint failed", error: "resume failed on this host." } } as ApplicationComputer
  const user = userEvent.setup()
  render(withToaster(<CheckpointPanel computer={unfinished} target="dev" actions={{ restoreCheckpoint, abandonRestore } as unknown as ApplicationActions} disabled={false} />))
  const notice = within(screen.getByRole("group", { name: "Unfinished Restore" }))
  expect(notice.getByText(/The Restore to “Disk snapshot” did not finish/)).toBeVisible()
  expect(notice.getByText(/dev was not changed/)).toBeVisible()
  expect(notice.getByText(/resume failed on this host/)).toBeVisible()

  await user.click(notice.getByRole("button", { name: "Abandon Restore…" }))
  expect(screen.getByText(/dev keeps its current state/)).toBeVisible()
  await user.click(confirmButton("Abandon"))
  await waitFor(() => expect(abandonRestore).toHaveBeenCalledWith("dev"))
  expect(await screen.findByText("Restore abandoned")).toBeVisible()

  await user.click(notice.getByRole("button", { name: "Retry Restore" }))
  await waitFor(() => expect(restoreCheckpoint).toHaveBeenCalledWith("dev", "point-2"))
})

it("offers only Retry once the Restore already replaced the computer", () => {
  const replaced = { ...computer, unfinishedRestore: { checkpointId: "point-2", checkpointName: "Disk snapshot", phase: "secured" }, pendingCheckpointRestore: { checkpointId: "point-2", sourceComputer: "dev", state: "disk" } } as ApplicationComputer
  render(<CheckpointPanel computer={replaced} target="dev" actions={{ restoreCheckpoint: vi.fn(), abandonRestore: vi.fn() } as unknown as ApplicationActions} disabled={false} />)
  const notice = within(screen.getByRole("group", { name: "Unfinished Restore" }))
  expect(notice.getByText(/Start dev to finish it/)).toBeVisible()
  expect(notice.queryByRole("button", { name: "Abandon Restore…" })).toBeNull()
  expect(notice.getByRole("button", { name: "Retry Restore" })).toBeVisible()
})

it("offers Delete only for checkpoints on this device", async () => {
  const remote = { ...computer, device: { id: "mac", name: "Ada’s Mac mini", connected: true } } as ApplicationComputer
  const readCheckpointUsage = vi.fn()
  const user = userEvent.setup()
  render(<CheckpointPanel computer={remote} target="dev" actions={{ forkCheckpoint: vi.fn(), deleteCheckpoint: vi.fn(), readCheckpointUsage } as unknown as ApplicationActions} disabled={false} />)
  await user.click(within(screen.getByText("Before refactor").closest("[data-checkpoint-name]")!).getByRole("button", { name: "Checkpoint actions for Before refactor" }))
  expect(screen.queryByRole("menuitem", { name: "Delete Before refactor" })).toBeNull()
  expect(readCheckpointUsage).not.toHaveBeenCalled()
})


it("blocks a failed capture's Retry while a Restore for the same computer is pending", async () => {
  const user = userEvent.setup()
  const restore = deferred()
  const createCheckpoint = vi.fn().mockRejectedValueOnce(new Error("Capture failed")).mockResolvedValue(undefined)
  const restoreCheckpoint = vi.fn(() => restore.promise)
  render(withToaster(<CheckpointPanel computer={computer} target="dev" actions={{ createCheckpoint, restoreCheckpoint } as unknown as ApplicationActions} disabled={false} />))
  await user.click(screen.getByRole("button", { name: "New checkpoint" }))
  await user.click(screen.getByRole("button", { name: "Create" }))
  const retry = await screen.findByRole("button", { name: "Retry" })
  const row = within(screen.getByText("Before refactor").closest("[data-checkpoint-name]")!)
  await user.click(row.getByRole("button", { name: "Restore" }))
  await user.click(confirmButton("Restore"))
  await waitFor(() => expect(restoreCheckpoint).toHaveBeenCalledOnce())
  try {
    expect(screen.getByRole("button", { name: "New checkpoint" })).toBeDisabled()
    await user.click(retry)
    expect(createCheckpoint).toHaveBeenCalledOnce()
    expect(screen.getByRole("button", { name: "New checkpoint" })).toBeDisabled()
  } finally { await act(async () => restore.resolve()) }
  expect(screen.getByRole("button", { name: "New checkpoint" })).toBeEnabled()
})
