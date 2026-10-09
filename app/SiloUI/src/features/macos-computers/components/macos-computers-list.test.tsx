import { act, render, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { mockIPC } from "@tauri-apps/api/mocks"
import { afterEach, expect, it, vi } from "vitest"

import { TooltipProvider } from "@/components/ui/tooltip"
import { nativeMacosComputersBackend } from "@/desktop/macos-computers"
import { createFixtureMacosComputersBackend, macosComputerFixtures } from "@/fixtures/macos-computers"
import { assertNativeBridgeMocksHandled, nativeBridgeMock } from "@/test/native-bridge-mock"
import {
  createMacosComputersStore,
  MacosComputersContext,
  type MacosComputer,
  type MacosComputersBackend,
  type MacosComputersState,
} from "../model/macos-computers"
import { showActionFailure } from "@/lib/operation-toast"
import { ComputerConfigurationList } from "@/features/computers/components/computer-configuration-list"
import { productionComputerDefaults } from "@/features/onboarding/model/computer-configuration"

vi.mock("@/lib/operation-toast", async importOriginal => ({ ...await importOriginal<typeof import("@/lib/operation-toast")>(), showActionFailure: vi.fn() }))

afterEach(assertNativeBridgeMocksHandled)

function popoverButton(name: string) { return within(document.querySelector<HTMLElement>("[data-slot=popover-content]")!).getByRole("button", { name }) }

const linuxComputer = productionComputerDefaults[0]

function ui(store: ReturnType<typeof createMacosComputersStore> | null, capacity?: { logicalCPUs: number; memoryGiB: number }) {
  const list = <ComputerConfigurationList includeMacosComputers configurations={[linuxComputer]} onConfigurationsChange={vi.fn()} getDeviceCapacity={() => capacity} />
  return <TooltipProvider>{store ? <MacosComputersContext.Provider value={store}>{list}</MacosComputersContext.Provider> : list}</TooltipProvider>
}

function renderSection(backend: MacosComputersBackend, capacity?: { logicalCPUs: number; memoryGiB: number }) {
  render(ui(createMacosComputersStore(backend), capacity))
}

/** Waits until the macOS computers have been read into the list. */
async function loaded() {
  await waitFor(() => expect(document.querySelector("[data-macos-computer-id]")).not.toBeNull())
}

async function openNewMacosForm(user: ReturnType<typeof userEvent.setup>) {
  await user.click(await screen.findByRole("button", { name: "Add" }))
  await user.click(await screen.findByRole("menuitem", { name: "New computer" }))
  await user.selectOptions(await screen.findByRole("combobox", { name: "Operating system" }), "macOS")
  return await screen.findByTestId("macos-computer-form")
}

function row(name: string) {
  return within(screen.getByRole("list", { name: "Configured computers" })).getAllByRole("listitem").find(item => within(item).queryByRole("button", { name: `Open ${name}` }) || item.textContent?.startsWith(name))!
}

function backendFor(computers: readonly MacosComputer[]) {
  const base = createFixtureMacosComputersBackend(computers)
  return { ...base, create: vi.fn(base.create), action: vi.fn(base.action), openDisplay: vi.fn(base.openDisplay), clipboard: vi.fn(base.clipboard) }
}

it("renders every state with its label, resources and progress", async () => {
  renderSection(backendFor(macosComputerFixtures))
  await loaded()
  expect(row("sequoia-test")).toHaveTextContent("Downloading macOS 42%")
  expect(within(row("sequoia-test")).getByRole("progressbar", { name: "sequoia-test progress" })).toHaveAttribute("aria-valuenow", "42")
  expect(row("release-check")).toHaveTextContent("Installing macOS 63%")
  expect(row("agent-box")).toHaveTextContent("Setting up macOS")
  expect(row("agent-box")).toHaveTextContent("Creating the account")
  expect(within(row("agent-box")).getByRole("progressbar", { name: "agent-box progress" })).toBeVisible()
  expect(row("xcode-build")).toHaveTextContent("Stopped")
  expect(row("xcode-build")).toHaveTextContent("macOS 26.6.2 (25G83)")
  expect(row("xcode-build")).toHaveTextContent("6 CPUs · 16 GiB memory · 128 GiB disk")
  expect(row("daily")).toHaveTextContent("Running")
  expect(row("broken")).toHaveTextContent("Failed")
  expect(within(row("broken")).getByRole("alert")).toHaveTextContent("The macOS download did not finish.")
})

it("offers the actions that fit each state", async () => {
  renderSection(backendFor(macosComputerFixtures))
  await loaded()
  const names = (item: HTMLElement) => within(item).getAllByRole("button").map(button => button.getAttribute("aria-label") ?? button.textContent)
  expect(names(row("sequoia-test"))).toEqual(["Cancel creating sequoia-test"])
  expect(names(row("release-check"))).toEqual(["Cancel creating release-check"])
  expect(names(row("agent-box"))).toEqual(["Cancel setting up agent-box"])
  expect(names(row("xcode-build"))).toEqual(["Start xcode-build", "More actions for xcode-build"])
  expect(names(row("daily"))).toEqual(["Show screen of daily", "Stop daily", "More actions for daily"])
  // Never installed, so there is nothing to set up again.
  expect(names(row("broken"))).toEqual(["More actions for broken"])
})

it("starts, shows the screen of and stops a computer", async () => {
  const backend = backendFor(macosComputerFixtures)
  const user = userEvent.setup()
  renderSection(backend)
  await user.click(await screen.findByRole("button", { name: "Start xcode-build" }))
  expect(backend.action).toHaveBeenCalledWith("mac-stopped", "start")
  await user.click(await screen.findByRole("button", { name: "Show screen of xcode-build" }))
  expect(backend.openDisplay).toHaveBeenCalledWith("mac-stopped")
  await user.click(screen.getByRole("button", { name: "Stop xcode-build" }))
  expect(backend.action).toHaveBeenCalledWith("mac-stopped", "stop")
  await waitFor(() => expect(screen.getByRole("button", { name: "Start xcode-build" })).toBeVisible())
})

it("force stops from the menu", async () => {
  const backend = backendFor(macosComputerFixtures)
  const user = userEvent.setup()
  renderSection(backend)
  await user.click(await screen.findByRole("button", { name: "More actions for daily" }))
  await user.click(screen.getByRole("menuitem", { name: "Force stop daily" }))
  expect(backend.action).toHaveBeenCalledWith("mac-running", "force-stop")
})

it("pastes into and copies from a running computer from the menu", async () => {
  const backend = backendFor(macosComputerFixtures)
  const user = userEvent.setup()
  renderSection(backend)
  await user.click(await screen.findByRole("button", { name: "More actions for daily" }))
  await user.click(screen.getByRole("menuitem", { name: "Paste into daily" }))
  expect(backend.clipboard).toHaveBeenCalledWith("mac-running", "paste-into")
  await user.click(screen.getByRole("button", { name: "More actions for daily" }))
  await user.click(screen.getByRole("menuitem", { name: "Copy from daily" }))
  expect(backend.clipboard).toHaveBeenCalledWith("mac-running", "copy-from")
  expect(showActionFailure).not.toHaveBeenCalled()
})

it("reports a clipboard transfer that did not happen", async () => {
  const backend = backendFor(macosComputerFixtures)
  backend.clipboard.mockResolvedValue({ action: "copy", status: "computer-empty", content: null, message: null })
  const user = userEvent.setup()
  renderSection(backend)
  await user.click(await screen.findByRole("button", { name: "More actions for daily" }))
  await user.click(screen.getByRole("menuitem", { name: "Copy from daily" }))
  await waitFor(() => expect(showActionFailure).toHaveBeenCalledWith("Could not copy from daily", "Nothing to copy from daily", undefined, { native: false }))
})

it("offers the clipboard actions only while a computer runs", async () => {
  renderSection(backendFor(macosComputerFixtures))
  const user = userEvent.setup()
  await user.click(await screen.findByRole("button", { name: "More actions for xcode-build" }))
  expect(screen.queryByRole("menuitem", { name: "Paste into xcode-build" })).toBeNull()
  expect(screen.queryByRole("menuitem", { name: "Copy from xcode-build" })).toBeNull()
})

it("confirms before deleting a stopped computer", async () => {
  const backend = backendFor(macosComputerFixtures)
  const user = userEvent.setup()
  renderSection(backend)
  await user.click(await screen.findByRole("button", { name: "More actions for xcode-build" }))
  await user.click(screen.getByRole("menuitem", { name: "Delete xcode-build" }))
  expect(await screen.findByText("Delete xcode-build permanently?")).toBeVisible()
  expect(backend.action).not.toHaveBeenCalled()
  await user.click(popoverButton("Delete permanently"))
  await waitFor(() => expect(backend.action).toHaveBeenCalledWith("mac-stopped", "delete"))
  await waitFor(() => expect(screen.queryByText("xcode-build")).not.toBeInTheDocument())
})

it("confirms before cancelling a creation, and keeping it does nothing", async () => {
  const backend = backendFor(macosComputerFixtures)
  const user = userEvent.setup()
  renderSection(backend)
  await user.click(await screen.findByRole("button", { name: "Cancel creating sequoia-test" }))
  expect(await screen.findByText("Cancel creating sequoia-test?")).toBeVisible()
  await user.click(popoverButton("Keep creating"))
  expect(backend.action).not.toHaveBeenCalled()
  await user.click(screen.getByRole("button", { name: "Cancel creating sequoia-test" }))
  await user.click(popoverButton("Cancel creation"))
  await waitFor(() => expect(backend.action).toHaveBeenCalledWith("mac-download", "delete"))
})

it("confirms before cancelling a setup", async () => {
  const backend = backendFor(macosComputerFixtures)
  const user = userEvent.setup()
  renderSection(backend)
  await user.click(await screen.findByRole("button", { name: "Cancel setting up agent-box" }))
  expect(await screen.findByText("Cancel setting up agent-box?")).toBeVisible()
  await user.click(popoverButton("Cancel setup"))
  await waitFor(() => expect(backend.action).toHaveBeenCalledWith("mac-setup", "delete"))
})

it("retries the setup of an idle computer that is not set up", async () => {
  const unfinished: MacosComputer = { ...macosComputerFixtures[0], id: "mac-unfinished", name: "unfinished", state: "failed", detail: "Turning off System Integrity Protection is not implemented yet.", installed: true, setupComplete: false }
  const backend = backendFor([unfinished])
  const user = userEvent.setup()
  renderSection(backend)
  expect(await screen.findByRole("alert")).toHaveTextContent("not implemented yet")
  await user.click(screen.getByRole("button", { name: "Retry setup of unfinished" }))
  expect(backend.action).toHaveBeenCalledWith("mac-unfinished", "setup")
  await waitFor(() => expect(row("unfinished")).toHaveTextContent("Setting up macOS"))
  expect(screen.queryByRole("button", { name: "Retry setup of unfinished" })).not.toBeInTheDocument()
})

it("reports an action failure", async () => {
  const backend = { ...backendFor(macosComputerFixtures), action: vi.fn().mockRejectedValue("Not enough free disk space.") }
  const user = userEvent.setup()
  renderSection(backend)
  await user.click(await screen.findByRole("button", { name: "Start xcode-build" }))
  await waitFor(() => expect(backend.action).toHaveBeenCalled())
  await waitFor(() => expect(showActionFailure).toHaveBeenCalledWith("Could not start xcode-build", "Not enough free disk space.", undefined, { native: false }))
})

it("shows both kinds in one list with an operating system badge and a combined count", async () => {
  renderSection(backendFor(macosComputerFixtures))
  await loaded()
  const list = screen.getByRole("list", { name: "Configured computers" })
  const items = within(list).getAllByRole("listitem")
  expect(items).toHaveLength(1 + macosComputerFixtures.length)
  const badges = (item: HTMLElement) => within(item).getAllByText(/^(Linux|macOS)$/).map(badge => badge.textContent)
  expect(badges(items.find(item => item.hasAttribute("data-computer-id"))!)).toEqual(["Linux"])
  for (const computer of macosComputerFixtures) expect(badges(row(computer.name))).toEqual(["macOS"])
  expect(screen.getByText(`${1 + macosComputerFixtures.length} computers · ${1 + macosComputerFixtures.length} on this device · 0 on other devices`)).toBeVisible()
  expect(screen.queryByRole("heading", { name: "macOS computers" })).not.toBeInTheDocument()
  expect(screen.queryByRole("button", { name: "New macOS computer" })).not.toBeInTheDocument()
})

it("keeps the reorder handle for Linux computers only", async () => {
  renderSection(backendFor(macosComputerFixtures))
  await loaded()
  expect(screen.getByRole("button", { name: `Reorder ${linuxComputer.name}` })).toBeVisible()
  expect(screen.queryByRole("button", { name: "Reorder daily" })).not.toBeInTheDocument()
})

it("switches a new computer between Linux and macOS fields", async () => {
  const backend = backendFor([])
  const user = userEvent.setup()
  renderSection(backend)
  await screen.findByRole("button", { name: "Add" })
  await user.click(screen.getByRole("button", { name: "Add" }))
  await user.click(await screen.findByRole("menuitem", { name: "New computer" }))
  const os = await screen.findByRole("combobox", { name: "Operating system" })
  expect(within(os).getAllByRole("option").map(option => option.textContent)).toEqual(["Linux", "macOS"])
  expect(os).toHaveValue("linux")
  expect(screen.getByRole("combobox", { name: "CPUs ceiling" })).toBeVisible()
  await user.selectOptions(os, "macOS")
  expect(screen.queryByRole("combobox", { name: "CPUs ceiling" })).not.toBeInTheDocument()
  expect(screen.getByLabelText("Disk (GiB)")).toBeVisible()
  await user.selectOptions(screen.getByRole("combobox", { name: "Operating system" }), "Linux")
  expect(screen.getByRole("combobox", { name: "CPUs ceiling" })).toBeVisible()
})

it("creates a macOS computer from the editor with the defaults and notice", async () => {
  const backend = backendFor([])
  const user = userEvent.setup()
  renderSection(backend)
  const form = await openNewMacosForm(user)
  expect(within(form).getByText(/Silo downloads macOS from Apple \(about 20 GB\)/)).toHaveTextContent("Apple's macOS license allows up to two macOS virtual computers per Mac")
  expect(within(form).getByRole("button", { name: "Create" })).toBeDisabled()
  expect(within(form).getByLabelText("CPUs")).toHaveValue(4)
  expect(within(form).getByLabelText("Memory (GiB)")).toHaveValue(8)
  expect(within(form).getByLabelText("Disk (GiB)")).toHaveValue(64)
  await user.type(within(form).getByLabelText("Computer name"), "daily")
  await user.click(within(form).getByRole("button", { name: "Create" }))
  await waitFor(() => expect(backend.create).toHaveBeenCalledWith({ name: "daily", cpus: 4, memoryGiB: 8, diskGiB: 64 }))
  expect(await screen.findByRole("button", { name: "Cancel creating daily" })).toBeVisible()
  expect(screen.queryByTestId("macos-computer-form")).not.toBeInTheDocument()
  expect(within(row("daily")).getByText("macOS")).toBeVisible()
})

it("keeps the form open and reports a failed creation", async () => {
  const backend = { ...backendFor([]), create: vi.fn().mockRejectedValue("Not enough free disk space.") }
  const user = userEvent.setup()
  renderSection(backend)
  const form = await openNewMacosForm(user)
  await user.type(within(form).getByLabelText("Computer name"), "daily")
  await user.click(within(form).getByRole("button", { name: "Create" }))
  await waitFor(() => expect(showActionFailure).toHaveBeenCalledWith("Could not create daily", "Not enough free disk space.", undefined, { native: false }))
  expect(screen.getByTestId("macos-computer-form")).toBeVisible()
  expect(within(form).getByRole("button", { name: "Create" })).toBeEnabled()
})

it("creates a Linux computer through the unchanged editor", async () => {
  const onCommitComputer = vi.fn().mockResolvedValue(undefined)
  const backend = backendFor([])
  const user = userEvent.setup()
  render(<TooltipProvider><MacosComputersContext.Provider value={createMacosComputersStore(backend)}><ComputerConfigurationList includeMacosComputers configurations={[linuxComputer]} onConfigurationsChange={vi.fn()} onCommitComputer={onCommitComputer} /></MacosComputersContext.Provider></TooltipProvider>)
  await user.click(await screen.findByRole("button", { name: "Add" }))
  await user.click(await screen.findByRole("menuitem", { name: "New computer" }))
  const name = await screen.findByLabelText("Computer name")
  await user.clear(name)
  await user.type(name, "fresh")
  await user.click(screen.getByRole("button", { name: "Create" }))
  await waitFor(() => expect(onCommitComputer).toHaveBeenCalled())
  expect(backend.create).not.toHaveBeenCalled()
})

it("validates the macOS fields against the device and existing names", async () => {
  const backend = backendFor(macosComputerFixtures)
  const user = userEvent.setup()
  renderSection(backend, { logicalCPUs: 8, memoryGiB: 16 })
  await loaded()
  const form = await openNewMacosForm(user)
  const create = within(form).getByRole("button", { name: "Create" })
  await user.type(within(form).getByLabelText("Computer name"), "Bad Name")
  expect(within(form).getByText(/lowercase letters/)).toBeVisible()
  expect(create).toBeDisabled()
  await user.clear(within(form).getByLabelText("Computer name"))
  await user.type(within(form).getByLabelText("Computer name"), "daily")
  expect(within(form).getByText("A macOS computer with this name exists.")).toBeVisible()
  await user.clear(within(form).getByLabelText("Computer name"))
  await user.type(within(form).getByLabelText("Computer name"), "fresh")
  expect(create).toBeEnabled()
  await user.clear(within(form).getByLabelText("CPUs"))
  await user.type(within(form).getByLabelText("CPUs"), "9")
  expect(within(form).getByText("Use 2 to 8 CPUs.")).toBeVisible()
  expect(create).toBeDisabled()
  await user.clear(within(form).getByLabelText("CPUs"))
  await user.type(within(form).getByLabelText("CPUs"), "8")
  await user.clear(within(form).getByLabelText("Memory (GiB)"))
  await user.type(within(form).getByLabelText("Memory (GiB)"), "3")
  expect(within(form).getByText("Use 4 to 16 GiB of memory.")).toBeVisible()
  await user.clear(within(form).getByLabelText("Memory (GiB)"))
  await user.type(within(form).getByLabelText("Memory (GiB)"), "16")
  await user.clear(within(form).getByLabelText("Disk (GiB)"))
  await user.type(within(form).getByLabelText("Disk (GiB)"), "2000")
  expect(within(form).getByText("Use 32 to 1024 GiB of disk.")).toBeVisible()
  await user.clear(within(form).getByLabelText("Disk (GiB)"))
  await user.type(within(form).getByLabelText("Disk (GiB)"), "100")
  await user.click(create)
  await waitFor(() => expect(backend.create).toHaveBeenCalledWith({ name: "fresh", cpus: 8, memoryGiB: 16, diskGiB: 100 }))
})

it("hides macOS entirely where it is not supported", async () => {
  const state: MacosComputersState = { supported: false, unsupportedReason: "macOS computers need a Mac with Apple silicon.", computers: [] }
  const backend = { ...backendFor([]), read: vi.fn(async () => state) }
  const user = userEvent.setup()
  renderSection(backend)
  await waitFor(() => expect(backend.read).toHaveBeenCalled())
  await act(async () => {})
  expect(screen.queryByText(/Apple silicon/)).not.toBeInTheDocument()
  expect(document.querySelector("[data-macos-computer-id]")).toBeNull()
  await user.click(screen.getByRole("button", { name: "Add" }))
  await user.click(await screen.findByRole("menuitem", { name: "New computer" }))
  const os = await screen.findByRole("combobox", { name: "Operating system" })
  expect(within(os).getAllByRole("option").map(option => option.textContent)).toEqual(["Linux"])
})

it("lists only Linux computers without a macOS provider", () => {
  render(ui(null))
  expect(screen.getByText("1 computer · 1 on this device · 0 on other devices")).toBeVisible()
  expect(screen.getByText("Linux")).toBeVisible()
  expect(screen.queryByText("macOS")).not.toBeInTheDocument()
})

it("re-renders when the backend reports a change", async () => {
  let emit: (state: unknown) => void = () => {}
  const base = backendFor(macosComputerFixtures)
  renderSection({ ...base, listen: async handler => { emit = handler; return () => {} } })
  expect(await screen.findByText(/Downloading macOS 42%/)).toBeVisible()
  const [first, ...rest] = macosComputerFixtures
  act(() => emit({ supported: true, unsupportedReason: null, computers: [{ ...first, state: "installing", progress: 0.1 }, ...rest] }))
  expect(await screen.findByText(/Installing macOS 10%/)).toBeVisible()
  expect(screen.queryByText(/Downloading macOS 42%/)).not.toBeInTheDocument()
})

it("invokes the native commands with the contract's payloads", async () => {
  const handlers = {
    read_macos_computers: () => ({ supported: true, unsupportedReason: null, computers: [] }),
    create_macos_computer: () => macosComputerFixtures[0],
    macos_computer_action: () => undefined,
    open_macos_display: () => undefined,
    macos_computer_clipboard: () => ({ action: "copy", status: "copied", content: "text", message: null }),
  }
  const invoke = nativeBridgeMock(handlers)
  mockIPC((command, payload) => invoke(command, payload as Record<string, unknown>), { shouldMockEvents: true })
  await nativeMacosComputersBackend.read()
  await nativeMacosComputersBackend.create({ name: "daily", cpus: 4, memoryGiB: 8, diskGiB: 64 })
  await nativeMacosComputersBackend.action("a", "force-stop")
  await nativeMacosComputersBackend.openDisplay("a")
  await nativeMacosComputersBackend.clipboard("a", "copy-from")
  expect(invoke.mock.calls).toEqual([
    ["read_macos_computers", {}],
    ["create_macos_computer", { request: { name: "daily", cpus: 4, memoryGiB: 8, diskGiB: 64 } }],
    ["macos_computer_action", { id: "a", action: "force-stop" }],
    ["open_macos_display", { id: "a" }],
    ["macos_computer_clipboard", { id: "a", direction: "copy-from" }],
  ])
})

it("shows a read failure with Retry, and recovers", async () => {
  const user = userEvent.setup()
  const backend = { ...backendFor(macosComputerFixtures), read: vi.fn().mockRejectedValueOnce("The display service did not answer.").mockResolvedValue({ supported: true, unsupportedReason: null, computers: [] }) }
  renderSection(backend)
  expect(await screen.findByRole("alert")).toHaveTextContent("Could not read the macOS computers. The display service did not answer.")
  await user.click(screen.getByRole("button", { name: "Retry" }))
  await waitFor(() => expect(screen.queryByRole("alert")).not.toBeInTheDocument())
})

it("warns about a malformed update and keeps the computers", async () => {
  let emit: (state: unknown) => void = () => {}
  const base = backendFor(macosComputerFixtures)
  renderSection({ ...base, listen: async handler => { emit = handler; return () => {} } })
  await loaded()
  act(() => emit({ nonsense: true }))
  expect(await screen.findByRole("status")).toHaveTextContent("unreadable")
  expect(screen.getByText("daily")).toBeVisible()
})

it("shows Retry when the first read is unreadable", async () => {
  const backend = { ...backendFor([]), read: vi.fn().mockResolvedValueOnce({ nonsense: true }).mockResolvedValue({ supported: true, unsupportedReason: null, computers: [] }) }
  const user = userEvent.setup()
  renderSection(backend)
  expect(await screen.findByRole("alert")).toHaveTextContent("unreadable")
  await user.click(screen.getByRole("button", { name: "Retry" }))
  await waitFor(() => expect(screen.queryByRole("alert")).not.toBeInTheDocument())
})
