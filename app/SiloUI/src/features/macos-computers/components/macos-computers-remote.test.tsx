import { render, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, expect, it, vi } from "vitest"

import { TooltipProvider } from "@/components/ui/tooltip"
import { createFixtureMacosComputersBackend, createFixtureMacosRemoteBackend } from "@/fixtures/macos-computers"
import { MacosComputerRow } from "./macos-computer-row"
import { ComputerConfigurationList } from "@/features/computers/components/computer-configuration-list"
import { productionComputerDefaults } from "@/features/onboarding/model/computer-configuration"
import { showActionFailure } from "@/lib/operation-toast"
import { createMacosComputersStore, MacosComputersContext, remoteMacosComputerId, type MacosComputer, type MacosComputersState } from "../model/macos-computers"

vi.mock("@/lib/operation-toast", async importOriginal => ({ ...await importOriginal<typeof import("@/lib/operation-toast")>(), showActionFailure: vi.fn() }))

afterEach(() => vi.clearAllMocks())

const linuxComputer = productionComputerDefaults[0]
const running: MacosComputer = { id: "r1", name: "remote-run", cpus: 4, memoryGiB: 8, diskGiB: 64, osVersion: "26.6.2 (25G83)", state: "running", progress: null, detail: null, displayOpen: false, installed: true, setupComplete: true, checkpoints: [{ id: "c1", name: "saved", createdAt: "2026-10-09T10:00:00Z", scope: "disk", reason: "manual" }] }
const stopped: MacosComputer = { ...running, id: "r2", name: "remote-off", state: "stopped" }
const hosting: MacosComputersState = { supported: true, unsupportedReason: null, computers: [running, stopped], template: { macosVersion: "26.6.2", build: "25G83", current: true }, minDiskGiB: 80 }
const unsupported: MacosComputersState = { supported: false, unsupportedReason: "Requires Apple silicon.", computers: [], template: null, minDiskGiB: 32 }

const studio = { id: "studio", name: "Studio", address: "studio.local", connected: true }
const linuxBox = { id: "box", name: "Linux box", address: "box.local", connected: true }

function setup(devices: Record<string, MacosComputersState | Error>, options: { local?: readonly MacosComputer[]; deviceList?: typeof studio[] } = {}) {
  const remote = createFixtureMacosRemoteBackend(devices)
  const spies = { snapshot: vi.fn(remote.snapshot), create: vi.fn(remote.create), action: vi.fn(remote.action), openDisplay: vi.fn(remote.openDisplay) }
  const local = createFixtureMacosComputersBackend(options.local ?? [])
  const backend = { ...local, create: vi.fn(local.create), remote: spies }
  const store = createMacosComputersStore(backend)
  const deviceList = options.deviceList ?? [studio, linuxBox]
  render(<TooltipProvider><MacosComputersContext.Provider value={store}>
    <ComputerConfigurationList includeMacosComputers configurations={[linuxComputer]} devices={deviceList} getDeviceId={() => undefined} onConfigurationsChange={vi.fn()} />
  </MacosComputersContext.Provider></TooltipProvider>)
  return { spies, backend }
}

function row(name: string) {
  return document.querySelector<HTMLElement>(`[data-macos-computer-id="${remoteMacosComputerId("studio", name)}"]`)!
}

async function openMacosForm(user: ReturnType<typeof userEvent.setup>) {
  await user.click(await screen.findByRole("button", { name: "Add" }))
  await user.click(await screen.findByRole("menuitem", { name: "New computer" }))
  await user.selectOptions(await screen.findByRole("combobox", { name: "Operating system" }), "macOS")
  return await screen.findByTestId("macos-computer-form")
}

it("lists a hosting device's macOS computers with its badge and counts them as being on other devices", async () => {
  setup({ studio: hosting, box: unsupported })
  await screen.findByText("remote-run")
  expect(within(row("r1")).getByRole("note", { name: /Computer on Studio · Connected · studio.local/ })).toBeVisible()
  expect(row("r1")).toHaveTextContent("Running · macOS 26.6.2 (25G83) · 4 CPUs · 8 GiB memory · 64 GiB disk")
  expect(screen.getByText(/3 computers · 1 on this device · 2 on other devices/)).toBeVisible()
  // A device that cannot host macOS contributes no rows.
  expect(screen.queryByRole("note", { name: /Linux box/ })).toBeNull()
})

it("keeps the local rows when a device fails to answer and says which device", async () => {
  const local: MacosComputer = { ...stopped, id: "local-1", name: "mine" }
  setup({ studio: new Error("Studio is not responding."), box: unsupported }, { local: [local] })
  await screen.findByText("mine")
  expect(await screen.findByRole("alert")).toHaveTextContent("Could not read the macOS computers on Studio. Studio is not responding.")
})

it("routes start, stop, the screen and delete to the device that hosts the computer", async () => {
  const { spies } = setup({ studio: hosting, box: unsupported })
  const user = userEvent.setup()
  await screen.findByText("remote-run")
  await user.click(within(row("r2")).getByRole("button", { name: "Start remote-off" }))
  expect(spies.action).toHaveBeenCalledWith("studio", "r2", "start")
  await user.click(await within(row("r2")).findByRole("button", { name: "Stop remote-off" }))
  expect(spies.action).toHaveBeenCalledWith("studio", "r2", "stop")
  await user.click(within(row("r1")).getByRole("button", { name: "Show screen of remote-run" }))
  expect(spies.openDisplay).toHaveBeenCalledWith("studio", "r1")
  await user.click(within(row("r1")).getByRole("button", { name: "More actions for remote-run" }))
  await user.click(screen.getByRole("menuitem", { name: "Force stop remote-run" }))
  expect(spies.action).toHaveBeenCalledWith("studio", "r1", "force-stop")
  await user.click(await within(row("r1")).findByRole("button", { name: "More actions for remote-run" }))
  await user.click(await screen.findByRole("menuitem", { name: "Delete remote-run" }))
  await user.click(await screen.findByRole("button", { name: "Delete permanently" }))
  expect(spies.action).toHaveBeenCalledWith("studio", "r1", "delete")
  await waitFor(() => expect(screen.queryByText("remote-run")).toBeNull())
  expect(showActionFailure).not.toHaveBeenCalled()
})

it("offers no checkpoints or clipboard transfer for a computer of another device", async () => {
  setup({ studio: hosting, box: unsupported })
  const user = userEvent.setup()
  await screen.findByText("remote-run")
  expect(within(row("r1")).queryByRole("button", { name: /Checkpoints of/ })).toBeNull()
  await user.click(within(row("r1")).getByRole("button", { name: "More actions for remote-run" }))
  expect(screen.queryByRole("menuitem", { name: "Paste into remote-run" })).toBeNull()
  expect(screen.queryByRole("menuitem", { name: "Copy from remote-run" })).toBeNull()
  expect(screen.getByRole("menuitem", { name: "Force stop remote-run" })).toBeVisible()
})

it("reports a failing remote action with the backend text", async () => {
  const { spies } = setup({ studio: hosting, box: unsupported })
  spies.action.mockRejectedValue("Macs run at most two macOS computers at once.")
  const user = userEvent.setup()
  await screen.findByText("remote-off")
  await user.click(within(row("r2")).getByRole("button", { name: "Start remote-off" }))
  await waitFor(() => expect(showActionFailure).toHaveBeenCalledWith("Could not start remote-off", "Macs run at most two macOS computers at once.", undefined, { native: false }))
})

it("disables the actions of a computer whose device is offline and says so", () => {
  const store = createMacosComputersStore(createFixtureMacosComputersBackend([]))
  render(<TooltipProvider><MacosComputerRow computer={{ ...stopped, id: remoteMacosComputerId("studio", "r2") }} store={store} device={{ ...studio, connected: false, computerId: "r2" }} /></TooltipProvider>)
  expect(screen.getByText(/Offline · last known status/)).toBeVisible()
  expect(screen.getByRole("button", { name: "Start remote-off" })).toBeDisabled()
  expect(screen.getByRole("button", { name: "More actions for remote-off" })).toBeVisible()
})

it("offers Run on for the devices that can host macOS only, and validates names against the chosen device", async () => {
  const { spies, backend } = setup({ studio: hosting, box: unsupported }, { local: [{ ...stopped, id: "local-1", name: "mine" }] })
  const user = userEvent.setup()
  await screen.findByText("remote-run")
  const form = await openMacosForm(user)
  const runOn = within(form).getByRole("combobox", { name: "Run on" })
  expect(within(runOn).getAllByRole("option").map(option => option.textContent)).toEqual(["This device", "Studio"])
  const name = within(form).getByRole("textbox", { name: "Computer name" })
  await user.type(name, "mine")
  expect(within(form).getByText("A macOS computer with this name exists.")).toBeVisible()
  await user.selectOptions(runOn, "Studio")
  expect(within(form).queryByText("A macOS computer with this name exists.")).toBeNull()
  await user.clear(name)
  await user.type(name, "remote-off")
  expect(within(form).getByText("A macOS computer with this name exists.")).toBeVisible()
  // The device's template sets the smallest disk, and the local template can only be removed locally.
  expect(within(form).queryByRole("button", { name: "Remove template" })).toBeNull()
  await user.clear(name)
  await user.type(name, "fresh")
  await user.clear(within(form).getByRole("spinbutton", { name: "Disk (GiB)" }))
  await user.type(within(form).getByRole("spinbutton", { name: "Disk (GiB)" }), "64")
  expect(within(form).getByText(/Use 80 to 1024 GiB of disk/)).toBeVisible()
  await user.clear(within(form).getByRole("spinbutton", { name: "Disk (GiB)" }))
  await user.type(within(form).getByRole("spinbutton", { name: "Disk (GiB)" }), "80")
  await user.click(within(form).getByRole("button", { name: "Create" }))
  await waitFor(() => expect(spies.create).toHaveBeenCalledWith("studio", { name: "fresh", cpus: 4, memoryGiB: 8, diskGiB: 80 }))
  expect(backend.create).not.toHaveBeenCalled()
  expect(await screen.findByText("fresh")).toBeVisible()
})

it("creates on this device when it is chosen", async () => {
  const { spies, backend } = setup({ studio: hosting, box: unsupported })
  const user = userEvent.setup()
  await screen.findByText("remote-run")
  const form = await openMacosForm(user)
  await user.type(within(form).getByRole("textbox", { name: "Computer name" }), "here")
  await user.click(within(form).getByRole("button", { name: "Create" }))
  await waitFor(() => expect(backend.create).toHaveBeenCalledWith({ name: "here", cpus: 4, memoryGiB: 8, diskGiB: 64 }))
  expect(spies.create).not.toHaveBeenCalled()
})

it("shows no Run on when no other device can host macOS", async () => {
  setup({ studio: unsupported, box: unsupported })
  const user = userEvent.setup()
  await waitFor(() => expect(screen.queryByText("remote-run")).toBeNull())
  const form = await openMacosForm(user)
  expect(within(form).queryByRole("combobox", { name: "Run on" })).toBeNull()
})
