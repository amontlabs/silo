import { act, fireEvent, render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it, vi } from "vitest"
import { UpdatesCard, UpdateNotice } from "./updates"
import { UpdatesProvider, type UpdateBackend, type UpdateSnapshot } from "./update-store"

const state: UpdateSnapshot = { phase: "idle", lastChecked: null, retryAction: null, currentVersion: "0.1.0", availableVersion: null, releaseNotes: null, downloadedBytes: 0, totalBytes: null, automaticChecks: true, packageKind: "macos", releaseUrl: "https://github.com/amontlabs/silo/releases", error: null, errorDetails: null, installBlockReason: null, runningComputers: [], canInstall: true }
function mount(initial: Partial<UpdateSnapshot> = {}, adjust: (backend: UpdateBackend) => void = () => {}) {
  let emit!: (value: UpdateSnapshot) => void
  const backend: UpdateBackend = {
    read: vi.fn(async () => ({ ...state, ...initial })),
    subscribe: vi.fn(async (receive) => { emit = receive; return vi.fn() }),
    check: vi.fn(async () => ({ ...state, phase: "available" as const, availableVersion: "0.2.0" })),
    download: vi.fn(async () => ({ ...state, phase: "ready" as const, availableVersion: "0.2.0" })),
    install: vi.fn(async () => ({ ...state, phase: "installing" as const })),
    setAutomaticChecks: vi.fn(async (enabled) => ({ ...state, automaticChecks: enabled })),
    openRelease: vi.fn(async () => {}),
  }
  const open = vi.fn()
  adjust(backend)
  const view = render(<UpdatesProvider backend={backend}><UpdateNotice onOpen={open} /><UpdatesCard /></UpdatesProvider>)
  return { backend, open, view, emit: (patch: Partial<UpdateSnapshot>) => act(() => emit({ ...state, ...initial, ...patch })) }
}

it("reconnects after a failed initial read without running an update action", async () => {
  const user = userEvent.setup()
  const stop = vi.fn()
  const read = vi.fn().mockRejectedValueOnce(new Error("private native failure")).mockResolvedValueOnce(state)
  const subscribe = vi.fn(async () => stop)
  const { backend, view } = mount({}, backend => { backend.read = read; backend.subscribe = subscribe })
  expect(await screen.findByRole("alert")).toHaveTextContent("Silo could not load updates. Try again.")
  expect(screen.getByRole("alert")).not.toHaveTextContent("private native failure")
  await user.click(screen.getByRole("button", { name: "Retry" }))
  expect(await screen.findByText("Version 0.1.0")).toBeVisible()
  expect(screen.queryByRole("alert")).not.toBeInTheDocument()
  expect(read).toHaveBeenCalledTimes(2)
  expect(subscribe).toHaveBeenCalledTimes(2)
  expect(stop).toHaveBeenCalledOnce()
  expect(backend.check).not.toHaveBeenCalled()
  expect(backend.download).not.toHaveBeenCalled()
  expect(backend.install).not.toHaveBeenCalled()
  view.unmount()
  expect(stop).toHaveBeenCalledTimes(2)
})

it("cleans up a subscription that registers after the updates view unmounts", async () => {
  let register!: (stop: () => void) => void
  const stop = vi.fn()
  const subscribe = vi.fn(() => new Promise<() => void>(resolve => { register = resolve }))
  const { backend, view } = mount({}, backend => { backend.subscribe = subscribe })
  view.unmount()
  await act(async () => register(stop))
  expect(stop).toHaveBeenCalledOnce()
  expect(backend.read).not.toHaveBeenCalled()
})

it("ignores an obsolete initial read failure after a newer native event", async () => {
  let reject!: (error: Error) => void
  const read = vi.fn(() => new Promise<UpdateSnapshot>((_, fail) => { reject = fail }))
  const { emit } = mount({}, backend => { backend.read = read })
  await waitFor(() => expect(read).toHaveBeenCalledOnce())
  emit({ phase: "ready", availableVersion: "0.2.0" })
  expect(screen.getByRole("button", { name: "Restart and update" })).toBeEnabled()
  await act(async () => reject(new Error("Obsolete read failed")))
  expect(screen.queryByRole("alert")).not.toBeInTheDocument()
})
it("restores update events after returning to a window whose subscription failed", async () => {
  const { backend, emit } = mount({}, backend => {
    vi.mocked(backend.subscribe).mockRejectedValueOnce(new Error("event registration failed"))
  })
  expect(await screen.findByRole("alert")).toHaveTextContent("Silo could not load updates. Try again.")
  fireEvent.focus(window)
  await waitFor(() => expect(backend.subscribe).toHaveBeenCalledTimes(2))
  expect(await screen.findByText("Version 0.1.0")).toBeVisible()
  expect(screen.queryByRole("alert")).not.toBeInTheDocument()
  emit({ phase: "available", availableVersion: "0.2.0" })
  expect(screen.getByRole("status")).toHaveTextContent("Silo 0.2.0 is available.")
  expect(backend.check).not.toHaveBeenCalled()
})
it("keeps a failed update connection visible when focus returns during listener recovery", async () => {
  let rejectRegistration!: (cause: Error) => void
  let resolveRead: ((next: UpdateSnapshot) => void) | undefined
  const { backend } = mount({}, backend => {
    vi.mocked(backend.subscribe).mockRejectedValueOnce(new Error("event registration failed"))
      .mockImplementationOnce(() => new Promise((_, reject) => { rejectRegistration = reject }))
    vi.mocked(backend.read).mockImplementation(() => new Promise(resolve => { resolveRead = resolve }))
  })
  await screen.findByRole("alert")
  fireEvent.focus(window)
  await waitFor(() => expect(backend.subscribe).toHaveBeenCalledTimes(2))
  fireEvent.focus(window)
  await act(async () => rejectRegistration(new Error("registration failed again")))
  await act(async () => resolveRead?.(state))
  expect(screen.getByRole("alert")).toHaveTextContent("Silo could not load updates. Try again.")
  expect(screen.getByRole("button", { name: "Retry" })).toBeEnabled()
})
it("loads the installed version without a fake up-to-date result and persists automatic checks", async () => {
  const user = userEvent.setup()
  const { backend } = mount()
  expect(await screen.findByText("Version 0.1.0")).toBeVisible()
  expect(screen.queryByText("Silo is up to date")).not.toBeInTheDocument()
  expect(screen.getByRole("switch", { name: "Automatically check for updates" })).toBeChecked()
  await user.click(screen.getByRole("switch", { name: "Automatically check for updates" }))
  expect(backend.setAutomaticChecks).toHaveBeenCalledWith(false)
  expect(screen.getByRole("switch", { name: "Automatically check for updates" })).not.toBeChecked()
})
it("checks and downloads only on request, displays real progress, and requires confirmation before stopping computers", async () => {
  const user = userEvent.setup()
  const { backend, emit } = mount()
  await user.click(await screen.findByRole("button", { name: "Check for updates" }))
  expect(backend.download).not.toHaveBeenCalled()
  expect(await screen.findByRole("button", { name: "Download update" })).toBeEnabled()
  emit({ phase: "downloading", availableVersion: "0.2.0", downloadedBytes: 25, totalBytes: 100 })
  expect(screen.getByRole("progressbar", { name: "Update download" })).toHaveAttribute("aria-valuenow", "25")
  emit({ phase: "downloading", downloadedBytes: 25, totalBytes: null })
  expect(screen.getByRole("progressbar")).not.toHaveAttribute("aria-valuenow")
  emit({ phase: "ready" as const, availableVersion: "0.2.0", runningComputers: ["dev"] })
  await user.click(screen.getByRole("button", { name: "Restart and update" }))
  expect(backend.install).not.toHaveBeenCalled()
  expect(screen.getByText(/dev will stop/)).toBeVisible()
  await user.keyboard("{Escape}")
  expect(screen.queryByRole("button", { name: "Stop computers and update" })).not.toBeInTheDocument()
  await user.click(screen.getByRole("button", { name: "Restart and update" }))
  await user.click(screen.getByRole("button", { name: "Stop computers and update" }))
  expect(backend.install).toHaveBeenCalledWith(true)
})
it("does not permit installation during active operations", async () => {
  mount({ phase: "ready" as const, availableVersion: "0.2.0", canInstall: false, installBlockReason: "Wait for active operations to finish." })
  expect(await screen.findByRole("button", { name: "Restart and update" })).toBeDisabled()
  expect(screen.getByText("Wait for active operations to finish.")).toBeVisible()
})
it("opens the package release for manual installations instead of offering native installation", async () => {
  const user = userEvent.setup()
  const { backend } = mount({ packageKind: "manual", phase: "available" as const, availableVersion: "0.2.0" })
  await user.click(await screen.findByRole("button", { name: "View installers on GitHub" }))
  expect(screen.getByText("Version 0.2.0 is available.")).toBeVisible()
  const help = screen.getByText("How to install").closest("details")
  expect(help).not.toHaveAttribute("open")
  await user.click(screen.getByText("How to install"))
  expect(help).toHaveAttribute("open")
  expect(screen.getByText(/sudo apt update/)).toBeVisible()
  expect(screen.getByText(/sudo apt install \/path\/to\/silo.deb/)).toBeVisible()
  expect(screen.queryByText(/installer below/)).not.toBeInTheDocument()
  expect(backend.openRelease).toHaveBeenCalledOnce()
  expect(backend.download).not.toHaveBeenCalled()
  expect(screen.queryByRole("button", { name: "Restart and update" })).not.toBeInTheDocument()
})
it("retries opening manual installers after the release page failed to open", async () => {
  const user = userEvent.setup()
  const { backend } = mount({ packageKind: "manual", phase: "available", availableVersion: "0.2.0" }, backend => {
    vi.mocked(backend.openRelease).mockRejectedValueOnce(new Error("browser unavailable"))
  })
  await user.click(await screen.findByRole("button", { name: "View installers on GitHub" }))
  await user.click(await screen.findByRole("button", { name: "Retry" }))
  expect(backend.openRelease).toHaveBeenCalledTimes(2)
  expect(backend.check).not.toHaveBeenCalled()
  expect(backend.download).not.toHaveBeenCalled()
})
it("keeps native failure details collapsed and shows a useful retry", async () => {
  const user = userEvent.setup()
  const { backend } = mount({ phase: "error", error: "The download was interrupted.", errorDetails: "Network connection closed.", availableVersion: "0.2.0" })
  expect(await screen.findByRole("alert")).toHaveTextContent("The download was interrupted.")
  expect(screen.getByText("Details").closest("details")).not.toHaveAttribute("open")
  await user.click(screen.getByRole("button", { name: "Retry" }))
  expect(backend.check).toHaveBeenCalledOnce()
})
it("does not replace newer native download progress with a stale command result", async () => {
  const user = userEvent.setup()
  const { backend, emit } = mount()
  let resolve!: (value: UpdateSnapshot) => void
  vi.mocked(backend.check).mockImplementation(() => new Promise((done) => { resolve = done }))
  await user.click(await screen.findByRole("button", { name: "Check for updates" }))
  emit({ phase: "downloading", downloadedBytes: 40, totalBytes: 100 })
  await act(async () => resolve({ ...state, phase: "checking" }))
  await waitFor(() => expect(screen.getByRole("progressbar")).toHaveAttribute("aria-valuenow", "40"))
})
it("shows up to date only after the native checker confirms it", async () => {
  mount({ lastChecked: "2026-09-10T12:00:00Z" })
  expect(await screen.findByText("Version 0.1.0 · Silo is up to date")).toBeVisible()
})
it("retries a failed download directly without discarding the selected update", async () => {
  const user = userEvent.setup()
  const { backend } = mount({ phase: "error", error: "Download interrupted.", retryAction: "download", availableVersion: "0.2.0" })
  await user.click(await screen.findByRole("button", { name: "Retry" }))
  expect(backend.download).toHaveBeenCalledOnce()
  expect(backend.check).not.toHaveBeenCalled()
})
it("requires a fresh stop confirmation when retrying installation and dismisses it outside", async () => {
  const user = userEvent.setup()
  const { backend } = mount({ phase: "error", error: "Could not stop dev.", retryAction: "install", availableVersion: "0.2.0", runningComputers: ["dev"] })
  await user.click(await screen.findByRole("button", { name: "Retry" }))
  expect(backend.install).not.toHaveBeenCalled()
  expect(screen.getByRole("button", { name: "Cancel" })).toBeVisible()
  await user.click(screen.getByRole("heading", { name: "Updates" }))
  expect(screen.queryByRole("button", { name: "Cancel" })).not.toBeInTheDocument()
  await user.click(screen.getByRole("button", { name: "Retry" }))
  await user.click(screen.getByRole("button", { name: "Stop computers and update" }))
  expect(backend.install).toHaveBeenCalledWith(true)
})
it("keeps the same update notice dismissed while native state refreshes", async () => {
  const user = userEvent.setup()
  const { emit } = mount({ phase: "available", availableVersion: "0.2.0" })
  await user.click(await screen.findByRole("button", { name: "Dismiss update notice" }))
  emit({ phase: "available", availableVersion: "0.2.0" })
  expect(screen.queryByRole("button", { name: "View update" })).not.toBeInTheDocument()
  emit({ phase: "available", availableVersion: "0.3.0" })
  expect(screen.getByRole("button", { name: "View update" })).toBeVisible()
})
it("reports a failed native action without exposing its raw rejection", async () => {
  const user = userEvent.setup()
  const { backend } = mount()
  vi.mocked(backend.check).mockRejectedValue(new Error("unfiltered internal paths"))
  await user.click(await screen.findByRole("button", { name: "Check for updates" }))
  expect(await screen.findByRole("alert")).toHaveTextContent("The update action could not finish. Try again.")
  expect(screen.queryByText("unfiltered internal paths")).not.toBeInTheDocument()
})
it("retries downloading the available release after a command rejection without checking again", async () => {
  const user = userEvent.setup()
  const { backend } = mount({ packageKind: "appimage", phase: "available", availableVersion: "0.2.0" })
  vi.mocked(backend.download).mockRejectedValueOnce(new Error("Download command unavailable"))
  await user.click(await screen.findByRole("button", { name: "Download update" }))
  expect(await screen.findByRole("alert")).toHaveTextContent("The update action could not finish")
  await user.click(screen.getByRole("button", { name: "Retry" }))
  expect(backend.download).toHaveBeenCalledTimes(2)
  expect(backend.check).not.toHaveBeenCalled()
})
it.each([
  ["macos", "ready", "Restart and update"],
  ["debian", "available", "Update"],
] as const)("retries %s installation after frontend preparation fails without checking again", async (packageKind, phase, label) => {
  const user = userEvent.setup()
  const { backend } = mount({ packageKind, phase, availableVersion: "0.2.0" })
  vi.mocked(backend.install).mockRejectedValueOnce(new Error("Settings delivery failed"))
  await user.click(await screen.findByRole("button", { name: label }))
  expect(await screen.findByRole("alert")).toHaveTextContent("The update action could not finish")
  await user.click(screen.getByRole("button", { name: "Retry" }))
  expect(backend.install).toHaveBeenCalledTimes(2)
  expect(backend.check).not.toHaveBeenCalled()
})

it("does not overwrite saved automatic-check settings with an older focus refresh", async () => {
  const user = userEvent.setup()
  const { backend } = mount()
  await screen.findByText("Version 0.1.0")
  let resolve!: (value: UpdateSnapshot) => void
  vi.mocked(backend.read).mockImplementationOnce(() => new Promise((done) => { resolve = done }))
  fireEvent.focus(window)
  await user.click(screen.getByRole("switch", { name: "Automatically check for updates" }))
  expect(screen.getByRole("switch", { name: "Automatically check for updates" })).not.toBeChecked()
  await act(async () => resolve(state))
  expect(screen.getByRole("switch", { name: "Automatically check for updates" })).not.toBeChecked()
})
it("explains an inspection failure without claiming an operation is still running", async () => {
  mount({ phase: "ready", canInstall: false, installBlockReason: "Silo could not verify computer status. Check Computers before updating." })
  expect(await screen.findByText("Silo could not verify computer status. Check Computers before updating.")).toBeVisible()
  expect(screen.getByRole("button", { name: "Restart and update" })).toBeDisabled()
  expect(screen.queryByText("Wait for active operations to finish.")).not.toBeInTheDocument()
})
it("leaves installation status to the application guard without offering another update action", async () => {
  mount({ phase: "installing" })
  expect(await screen.findByText("Installing update. Silo will restart…")).toBeVisible()
  expect(screen.queryByRole("status")).not.toBeInTheDocument()
  expect(screen.queryByRole("button", { name: "View update" })).not.toBeInTheDocument()
  expect(screen.queryByRole("button", { name: "Check for updates" })).not.toBeInTheDocument()
})

it("surfaces a native automatic discovery without a manual check or download", async () => {
  const user = userEvent.setup()
  const { backend, emit, open } = mount({ packageKind: "appimage" })
  await screen.findByText("Version 0.1.0")
  emit({ phase: "checking" })
  emit({ phase: "available", availableVersion: "0.2.0" })
  expect(screen.getByRole("status")).toHaveTextContent("Silo 0.2.0 is available.")
  await user.click(screen.getByRole("button", { name: "View update" }))
  expect(open).toHaveBeenCalledOnce()
  expect(backend.check).not.toHaveBeenCalled()
  expect(backend.download).not.toHaveBeenCalled()
})

it("updates Debian through the authenticated installer and confirms running computers", async () => {
  const user = userEvent.setup()
  const { backend, emit } = mount({ packageKind: "debian", phase: "available", availableVersion: "0.5.1", runningComputers: ["dev"] })
  await user.click(await screen.findByRole("button", { name: "Update" }))
  expect(backend.install).not.toHaveBeenCalled()
  await user.click(screen.getByRole("button", { name: "Stop computers and update" }))
  expect(backend.install).toHaveBeenCalledWith(true)
  expect(backend.openRelease).not.toHaveBeenCalled()
  expect(backend.download).not.toHaveBeenCalled()
  emit({ phase: "installing", installStatus: "Refreshing Silo’s package list…" })
  expect(screen.getByText("Refreshing Silo’s package list…")).toBeVisible()
})
it("keeps Debian installation disabled while operations are active", async () => {
  mount({ packageKind: "debian", phase: "available", availableVersion: "0.5.1", canInstall: false, installBlockReason: "Wait for computer operations to finish." })
  expect(await screen.findByRole("button", { name: "Update" })).toBeDisabled()
  expect(screen.getByText("Wait for computer operations to finish.")).toBeVisible()
})
it("asks for a manual relaunch when an installed update could not restart, without offering a retry", async () => {
  const { backend } = mount({ packageKind: "debian", phase: "error", retryAction: "relaunch", canInstall: false,
    error: "Silo was updated but could not restart. Quit and reopen Silo to finish.", errorDetails: "exec failed" })
  expect(await screen.findByText("Silo was updated but could not restart. Quit and reopen Silo to finish.")).toBeVisible()
  expect(screen.queryByRole("button", { name: "Retry" })).not.toBeInTheDocument()
  expect(screen.queryByRole("button", { name: /Check for updates|Update|Restart and update/ })).not.toBeInTheDocument()
  expect(backend.check).not.toHaveBeenCalled()
  expect(backend.install).not.toHaveBeenCalled()
})

it.each(["initial", "focus"] as const)("ignores a failed %s status read superseded by a native update event", async kind => {
  let reject!: (cause: Error) => void
  const pending = new Promise<UpdateSnapshot>((_resolve, fail) => { reject = fail })
  const { backend, emit } = mount({}, backend => {
    if (kind === "initial") vi.mocked(backend.read).mockReturnValueOnce(pending)
  })
  if (kind === "focus") {
    await screen.findByText("Version 0.1.0")
    vi.mocked(backend.read).mockReturnValueOnce(pending)
    fireEvent.focus(window)
  }
  await waitFor(() => expect(backend.read).toHaveBeenCalledTimes(kind === "initial" ? 1 : 2))
  emit({ currentVersion: "0.2.0" })
  await act(async () => reject(new Error("Old status read failed")))
  expect(screen.getByText("Version 0.2.0")).toBeVisible()
  expect(screen.queryByRole("alert")).not.toBeInTheDocument()
})
