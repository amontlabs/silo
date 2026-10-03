import { act, render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it, vi } from "vitest"
import { ApplicationCommandMenu } from "@/features/application/components/application-command-menu"
import { updateCommands } from "./update-commands"
import { UpdatesCard } from "./updates"
import { UpdatesProvider, useUpdates, type UpdateBackend, type UpdateSnapshot, type Updates } from "./update-store"

const initial: UpdateSnapshot = {
  phase: "idle", lastChecked: null, retryAction: null, currentVersion: "0.3.3", availableVersion: null,
  releaseNotes: null, downloadedBytes: 0, totalBytes: null, automaticChecks: true, packageKind: "appimage",
  releaseUrl: "https://github.com/amontlabs/silo/releases", error: null, errorDetails: null,
  installBlockReason: null, runningComputers: [], canInstall: true,
}
function mount(patch: Partial<UpdateSnapshot> = {}) {
  const state = { ...initial, ...patch }
  const open = vi.fn()
  let receive!: (next: UpdateSnapshot) => void
  const backend: UpdateBackend = {
    read: vi.fn(async () => state), subscribe: vi.fn(async (listener) => { receive = listener; return () => {} }),
    check: vi.fn(async () => ({ ...initial, phase: "available" as const, availableVersion: "0.3.4" })),
    download: vi.fn(async () => ({ ...state, phase: "ready" as const, availableVersion: "0.3.4" })),
    install: vi.fn(async () => ({ ...state, phase: "installing" as const })),
    setAutomaticChecks: vi.fn(async () => state), openRelease: vi.fn(async () => {}),
  }
  function Harness() {
    return <><ApplicationCommandMenu commands={updateCommands(useUpdates(), open)} /><UpdatesCard /></>
  }
  render(<UpdatesProvider backend={backend}><Harness /></UpdatesProvider>)
  return { backend, open, emit: (next: Partial<UpdateSnapshot>) => act(() => receive({ ...state, ...next })) }
}
async function selectCommand(user: ReturnType<typeof userEvent.setup>, name: string) {
  await user.keyboard("{Control>}k{/Control}")
  await user.click(await screen.findByRole("option", { name }))
}

it("checks for updates from Ctrl+K and offers downloading only once a release is available", async () => {
  const user = userEvent.setup()
  const { backend, open } = mount()
  await screen.findByText("Version 0.3.3")
  await selectCommand(user, "Check for updates")
  expect(backend.check).toHaveBeenCalledOnce()
  expect(open).toHaveBeenCalledOnce()
  await selectCommand(user, "Download update")
  expect(backend.download).toHaveBeenCalledOnce()
  expect(await screen.findByRole("button", { name: "Restart and update" })).toBeEnabled()
})

it("requires the existing computer-stop confirmation for a palette install and allows cancellation", async () => {
  const user = userEvent.setup()
  const { backend } = mount({ phase: "ready", availableVersion: "0.3.4", runningComputers: ["dev", "build"] })
  await screen.findByRole("button", { name: "Restart and update" })
  await selectCommand(user, "Restart and update")
  expect(backend.install).not.toHaveBeenCalled()
  expect(screen.getByText(/dev, build will stop and restart/)).toBeVisible()
  await user.click(screen.getByRole("button", { name: "Cancel" }))
  expect(backend.install).not.toHaveBeenCalled()
  await selectCommand(user, "Restart and update")
  await user.click(screen.getByRole("button", { name: "Stop computers and update" }))
  expect(backend.install).toHaveBeenCalledExactlyOnceWith(true)
})

it("installs directly when no computers are running and suppresses duplicate actions while pending", async () => {
  const user = userEvent.setup()
  const { backend } = mount({ phase: "ready", availableVersion: "0.3.4" })
  vi.mocked(backend.install).mockReturnValue(new Promise(() => {}))
  await screen.findByRole("button", { name: "Restart and update" })
  await selectCommand(user, "Restart and update")
  expect(backend.install).toHaveBeenCalledExactlyOnceWith(false)
  await user.keyboard("{Control>}k{/Control}")
  expect(screen.queryByRole("option", { name: /update/i })).not.toBeInTheDocument()
  expect(backend.install).toHaveBeenCalledOnce()
})

it("opens manual installers instead of invoking the AppImage downloader", async () => {
  const user = userEvent.setup()
  const { backend } = mount({ phase: "available", availableVersion: "0.3.4", packageKind: "manual" })
  await screen.findByRole("button", { name: "View installers on GitHub" })
  await selectCommand(user, "View installers on GitHub")
  expect(backend.openRelease).toHaveBeenCalledOnce()
  expect(backend.download).not.toHaveBeenCalled()
  expect(backend.install).not.toHaveBeenCalled()
})

it("retries installation through the same confirmation and removes the command when native admission blocks it", async () => {
  const user = userEvent.setup()
  const { backend, emit } = mount({ phase: "error", availableVersion: "0.3.4", retryAction: "install", error: "Could not stop dev", runningComputers: ["dev"] })
  await screen.findByRole("button", { name: "Retry" })
  await selectCommand(user, "Retry update installation")
  expect(backend.install).not.toHaveBeenCalled()
  await user.click(screen.getByRole("button", { name: "Cancel" }))
  emit({ canInstall: false, installBlockReason: "Wait for backup" })
  await user.keyboard("{Control>}k{/Control}")
  expect(screen.queryByRole("option", { name: "Retry update installation" })).not.toBeInTheDocument()
})

it("invalidates confirmation when a newer update replaces the requested version", async () => {
  const user = userEvent.setup()
  const { emit, backend } = mount({ phase: "ready", availableVersion: "0.3.4", runningComputers: ["dev"] })
  await screen.findByRole("button", { name: "Restart and update" })
  await selectCommand(user, "Restart and update")
  expect(screen.getByRole("button", { name: "Stop computers and update" })).toBeVisible()
  emit({ availableVersion: "0.3.5" })
  expect(screen.queryByRole("button", { name: "Stop computers and update" })).not.toBeInTheDocument()
  expect(backend.install).not.toHaveBeenCalled()
})

it.each(["version", "admission"] as const)("does not restore a withdrawn stop confirmation when %s returns to its previous state", async (change) => {
  const user = userEvent.setup()
  const { emit, backend } = mount({ phase: "ready", availableVersion: "0.3.4", runningComputers: ["dev"] })
  await screen.findByRole("button", { name: "Restart and update" })
  await selectCommand(user, "Restart and update")
  expect(screen.getByRole("button", { name: "Stop computers and update" })).toBeVisible()
  emit(change === "version" ? { availableVersion: "0.3.5" } : { canInstall: false })
  expect(screen.queryByRole("button", { name: "Stop computers and update" })).not.toBeInTheDocument()
  emit({ availableVersion: "0.3.4", canInstall: true })
  expect(screen.queryByRole("button", { name: "Stop computers and update" })).not.toBeInTheDocument()
  expect(screen.getByRole("button", { name: "Restart and update" })).toBeEnabled()
  expect(backend.install).not.toHaveBeenCalled()
})

it.each([
  ["checking", null], ["downloading", null], ["installing", null],
] as const)("offers no commands during native %s", (phase, retryAction) => {
  expect(updateCommands({ snapshot: { ...initial, phase, retryAction }, pending: false } as Updates, vi.fn())).toEqual([])
})

it.each([
  ["check", "Retry checking for updates", "check"],
  ["download", "Retry update download", "download"],
] as const)("routes %s retries to the matching backend action", (retryAction, label, action) => {
  const check = vi.fn(), download = vi.fn(), open = vi.fn()
  const commands = updateCommands({ snapshot: { ...initial, phase: "error", retryAction }, pending: false, check, download } as unknown as Updates, open)
  expect(commands.map(command => command.label)).toEqual([label])
  commands[0].run()
  expect({ check, download }[action]).toHaveBeenCalledOnce()
  expect(open).toHaveBeenCalledOnce()
})

it("waits for initial state and reconnects only after a connection failure", async () => {
  expect(updateCommands(null, vi.fn())).toEqual([])
  const reconnect = vi.fn()
  const updates = { snapshot: null, pending: false, connectionError: null, reconnect } as unknown as Updates
  expect(updateCommands(updates, vi.fn())).toEqual([])
  updateCommands({ ...updates, connectionError: "Offline" }, vi.fn())[0].run()
  await waitFor(() => expect(reconnect).toHaveBeenCalledOnce())
})

it.each([
  ["ready", "appimage", "install update", "Restart and update"],
  ["available", "manual", "download update", "View installers on GitHub"],
] as const)("finds the %s update action using everyday download/install words", async (phase, packageKind, query, label) => {
  const user = userEvent.setup()
  mount({ phase, packageKind, availableVersion: "0.3.4" })
  await screen.findByRole("button", { name: label })
  await user.keyboard("{Control>}k{/Control}")
  await user.type(screen.getByRole("combobox", { name: "Search commands" }), query)
  expect(screen.getByRole("option", { name: label })).toBeVisible()
})
it("runs the Debian update action from the palette without opening GitHub", async () => {
  const user = userEvent.setup()
  const { backend } = mount({ phase: "available", packageKind: "debian", availableVersion: "0.5.1" })
  await screen.findByRole("button", { name: "Update" })
  await selectCommand(user, "Update Silo")
  expect(backend.install).toHaveBeenCalledExactlyOnceWith(false)
  expect(backend.download).not.toHaveBeenCalled()
  expect(backend.openRelease).not.toHaveBeenCalled()
})

it("offers no update commands while Silo must be relaunched to finish an installed update", () => {
  const snapshot: UpdateSnapshot = { ...initial, packageKind: "debian", phase: "error", retryAction: "relaunch",
    error: "Silo was updated but could not restart. Quit and reopen Silo to finish." }
  const updates = { snapshot, connectionError: null, pending: false } as unknown as Updates
  expect(updateCommands(updates, vi.fn())).toEqual([])
})
