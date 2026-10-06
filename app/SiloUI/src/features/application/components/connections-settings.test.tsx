import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react"
import { afterEach, describe, expect, it, vi } from "vitest"
import { toast } from "sonner"
import userEvent from "@testing-library/user-event"
import { Toaster } from "@/components/ui/sonner"

import { ConnectDeviceForm, ConnectionsSettings } from "./connections-settings"
import { applicationSourceForScenario } from "@/fixtures/application-scenarios"
import { connectionsStatusSchema } from "../model/connections"
import type { ApplicationActions, ApplicationSource } from "../model/application-source"

function source(connections: ApplicationSource["connections"]): ApplicationSource {
  return { ...applicationSourceForScenario("running"), connections, devices: [] }
}
function actions(overrides: Partial<ApplicationActions> = {}): ApplicationActions {
  return { connectDevice: vi.fn(), setConnectionsEnabled: vi.fn(), ...overrides } as unknown as ApplicationActions
}
afterEach(() => { toast.dismiss() })

describe("ConnectionsSettings", () => {
  it("disables connection removal when the adapter does not provide it", () => {
    const remote = { id: "office", name: "Office", address: "office.example", connected: false }
    render(<ConnectionsSettings source={{ ...source(undefined), devices: [remote] }} actions={actions()} />)
    expect(screen.getByRole("button", { name: "Remove connection to Office" })).toBeDisabled()
  })

  it("reveals the complete name of a device with a long SSH address", () => {
    const name = "Office workstation ".repeat(20).trim()
    const remote = { id: "office-id", name, address: `${"account".repeat(30)}@office.example`, connected: false }
    render(<ConnectionsSettings source={{ ...source(undefined), devices: [remote] }} actions={actions()} />)
    expect(screen.getByText(name)).toHaveAttribute("title", name)
  })

  it.each(["cancel", "connect"])("returns focus to the opening button after %s", async (close) => {
    const user = userEvent.setup()
    const connectDevice = vi.fn().mockResolvedValue(undefined)
    render(<ConnectionsSettings source={source(undefined)} actions={actions({ connectDevice })} />)
    await user.click(screen.getByRole("button", { name: "Connect device…" }))
    const address = screen.getByRole("textbox", { name: "Device address" })
    expect(address).toHaveFocus()
    if (close === "cancel") await user.click(screen.getByRole("button", { name: "Cancel" }))
    else {
      await user.type(address, "owner@office")
      await user.click(screen.getByRole("button", { name: "Connect" }))
      expect(connectDevice).toHaveBeenCalledExactlyOnceWith("owner@office")
    }
    expect(await screen.findByRole("button", { name: "Connect device…" })).toHaveFocus()
  })

  it("trims the address and blocks resubmission and cancellation while connecting", async () => {
    let finish!: () => void
    const connect = vi.fn(() => new Promise<void>(resolve => { finish = resolve }))
    const onClose = vi.fn()
    render(<ConnectDeviceForm connect={connect} onClose={onClose} />)
    fireEvent.change(screen.getByRole("textbox", { name: "Device address" }), { target: { value: "  owner@office  " } })
    fireEvent.click(screen.getByRole("button", { name: "Connect" }))
    fireEvent.submit(screen.getByRole("form", { name: "Connect device" }))
    expect(screen.getByRole("textbox", { name: "Device address" })).toBeDisabled()
    expect(screen.getByRole("button", { name: "Cancel" })).toBeDisabled()
    expect(connect).toHaveBeenCalledExactlyOnceWith("owner@office")
    expect(onClose).not.toHaveBeenCalled()
    await act(async () => finish())
    expect(onClose).toHaveBeenCalledOnce()
  })

  it.each([
    ["authorize", "Authorize SSH in Terminal…", "Opening Terminal…"],
    ["setupKey", "Set up Silo SSH key…", "Setting up SSH key…"],
  ] as const)("retries %s repair and requires an explicit reconnect afterwards", async (kind, label, progress) => {
    let fail!: (error: Error) => void
    const repair = vi.fn().mockImplementationOnce(() => new Promise<void>((_, reject) => { fail = reject })).mockResolvedValueOnce(undefined)
    const connect = vi.fn().mockRejectedValueOnce(new Error("SSH authentication failed")).mockResolvedValueOnce(undefined)
    const onClose = vi.fn()
    render(<ConnectDeviceForm connect={connect} onClose={onClose} {...{ [kind]: repair }} />)
    fireEvent.change(screen.getByRole("textbox", { name: "Device address" }), { target: { value: " owner@office " } })
    fireEvent.click(screen.getByRole("button", { name: "Connect" }))
    await screen.findByRole("alert")
    fireEvent.click(screen.getByRole("button", { name: label }))
    expect(screen.getByRole("button", { name: progress })).toBeDisabled()
    expect(screen.getByRole("button", { name: "Connect" })).toBeDisabled()
    expect(screen.queryByRole("button", { name: "Connecting…" })).not.toBeInTheDocument()
    expect(repair).toHaveBeenCalledExactlyOnceWith("owner@office")
    await act(async () => fail(new Error("Repair unavailable")))
    expect(screen.getByRole("alert")).toHaveTextContent("Repair unavailable")
    fireEvent.click(screen.getByRole("button", { name: label }))
    await waitFor(() => expect(screen.getByRole("button", { name: "Connect" })).toBeEnabled())
    expect(repair).toHaveBeenCalledTimes(2)
    expect(connect).toHaveBeenCalledOnce()
    expect(onClose).not.toHaveBeenCalled()
    fireEvent.click(screen.getByRole("button", { name: "Connect" }))
    await waitFor(() => expect(onClose).toHaveBeenCalledOnce())
    expect(connect).toHaveBeenLastCalledWith("owner@office")
  })

  it("retains a failed removal and retries the same device after its display name changes", async () => {
    const removeDevice = vi.fn().mockRejectedValueOnce(new Error("Connection could not be removed")).mockResolvedValueOnce(undefined)
    const remote = { id: "office-id", name: "Office", address: "owner@office", connected: false }
    const api = actions({ removeDevice })
    const view = render(<><Toaster /><ConnectionsSettings source={{ ...source(undefined), devices: [remote] }} actions={api} /></>)
    fireEvent.click(screen.getByRole("button", { name: "Remove connection to Office" }))
    expect(await screen.findByText("Device setting not changed")).toBeVisible()
    expect(screen.getByRole("button", { name: "Remove connection to Office" })).toBeEnabled()
    view.rerender(<><Toaster /><ConnectionsSettings source={{ ...source(undefined), devices: [{ ...remote, name: "Renamed Office" }] }} actions={api} /></>)
    fireEvent.click(screen.getByRole("button", { name: "Retry" }))
    await waitFor(() => expect(removeDevice).toHaveBeenCalledTimes(2))
    expect(removeDevice.mock.calls).toEqual([["office-id"], ["office-id"]])
    expect(screen.getByRole("button", { name: "Remove connection to Renamed Office" })).toBeVisible()
  })

  it("explains why Connections does not work on this device", () => {
    const status = connectionsStatusSchema.parse({ enabled: true, deviceId: "office", name: "Office Mac", address: "owner@office", error: "Another Silo instance owns Connections." })
    render(<ConnectionsSettings source={source(status)} actions={actions()} />)
    expect(screen.getByRole("alert")).toHaveTextContent("Another Silo instance owns Connections.")
    expect(screen.getByRole("switch", { name: "Allow connections from other devices" })).toBeEnabled()
  })

  it("offers every address another device may reach this one at", async () => {
    const writeText = vi.spyOn(navigator.clipboard, "writeText").mockResolvedValue(undefined)
    const status = connectionsStatusSchema.parse({
      enabled: true, deviceId: "office", name: "studio", address: "ana@studio.local",
      addresses: [
        { address: "ana@studio.local", kind: "name" },
        { address: "ana@100.101.102.103", kind: "tailscale" },
        { address: "ana@192.168.1.4", kind: "network" },
      ],
    })
    render(<ConnectionsSettings source={source(status)} actions={actions()} />)
    const list = screen.getByRole("list", { name: "Addresses for other devices" })
    expect(within(list).getAllByRole("listitem").map(item => item.textContent)).toEqual([
      expect.stringContaining("ana@studio.local"),
      expect.stringContaining("Tailscale"),
      expect.stringContaining("Local network"),
    ])
    fireEvent.click(within(list).getByRole("button", { name: "Copy ana@100.101.102.103" }))
    await waitFor(() => expect(writeText).toHaveBeenLastCalledWith("ana@100.101.102.103"))
  })

  it("falls back to the single address from an older status", () => {
    const status = connectionsStatusSchema.parse({ enabled: true, deviceId: "office", name: "studio", address: "ana@studio" })
    render(<ConnectionsSettings source={source(status)} actions={actions()} />)
    expect(screen.getByRole("button", { name: "Copy ana@studio" })).toBeInTheDocument()
  })

  it("replaces a saved device's address only after the user confirms", async () => {
    const connect = vi.fn()
      .mockRejectedValueOnce(new Error("Office is already saved at office.local. Use 10.0.0.9 for it instead only if that device moved to this address."))
      .mockResolvedValueOnce(undefined)
    const onClose = vi.fn()
    render(<ConnectDeviceForm connect={connect} onClose={onClose} />)
    fireEvent.change(screen.getByRole("textbox", { name: "Device address" }), { target: { value: "10.0.0.9" } })
    fireEvent.click(screen.getByRole("button", { name: "Connect" }))
    expect(await screen.findByRole("alert")).toHaveTextContent("already saved at office.local")
    expect(connect).toHaveBeenCalledWith("10.0.0.9")
    expect(onClose).not.toHaveBeenCalled()
    fireEvent.click(screen.getByRole("button", { name: "Use this address" }))
    await waitFor(() => expect(onClose).toHaveBeenCalled())
    expect(connect).toHaveBeenLastCalledWith("10.0.0.9", { replaceAddress: true })
  })

  it("offers no replacement for other connection errors", async () => {
    const connect = vi.fn().mockRejectedValue(new Error("SSH authentication failed."))
    render(<ConnectDeviceForm connect={connect} onClose={vi.fn()} />)
    fireEvent.change(screen.getByRole("textbox", { name: "Device address" }), { target: { value: "office" } })
    fireEvent.click(screen.getByRole("button", { name: "Connect" }))
    await screen.findByRole("alert")
    expect(screen.queryByRole("button", { name: "Use this address" })).not.toBeInTheDocument()
  })

  it("shows no problem when Connections works", () => {
    const status = connectionsStatusSchema.parse({ enabled: true, deviceId: "office", name: "Office Mac", address: "owner@office", error: null })
    render(<ConnectionsSettings source={source(status)} actions={actions()} />)
    expect(screen.queryByRole("alert")).not.toBeInTheDocument()
  })
})


it("explains connection removal before it is selected", async () => {
  const removeDevice = vi.fn().mockResolvedValue(undefined)
  const remote = { id: "office", name: "Office", address: "office.example", connected: false, busy: false }
  render(<ConnectionsSettings source={{ ...source(undefined), devices: [remote] }} actions={actions({ removeDevice })} />)
  expect(screen.getByText("Removing the connection leaves computers on Office unchanged.")).toBeVisible()
  fireEvent.click(screen.getByRole("button", { name: "Remove connection to Office" }))
  await waitFor(() => expect(removeDevice).toHaveBeenCalledExactlyOnceWith("office"))
})

it("blocks removal Retry while a Connections change is pending", async () => {
  let finish!: () => void
  const setConnectionsEnabled = vi.fn(() => new Promise<void>(resolve => { finish = resolve }))
  const removeDevice = vi.fn().mockRejectedValueOnce(new Error("Connection could not be removed")).mockResolvedValue(undefined)
  const remote = { id: "office", name: "Office", address: "owner@office", connected: false }
  const management = connectionsStatusSchema.parse({ enabled: true, deviceId: "local", name: "This device", address: "owner@local" })
  render(<><Toaster /><ConnectionsSettings source={{ ...source(management), devices: [remote] }} actions={actions({ setConnectionsEnabled, removeDevice })} /></>)
  fireEvent.click(screen.getByRole("button", { name: "Remove connection to Office" }))
  const retry = await screen.findByRole("button", { name: "Retry" })
  const toggle = screen.getByRole("switch", { name: "Allow connections from other devices" })
  fireEvent.click(toggle)
  expect(toggle).toBeDisabled()
  await act(async () => fireEvent.click(retry))
  expect(removeDevice).toHaveBeenCalledOnce()
  expect(toggle).toBeDisabled()
  await act(async () => finish())
  expect(toggle).toBeEnabled()
})


it("ignores an obsolete management Retry after a newer choice succeeds", async () => {
  const user = userEvent.setup()
  const management = { enabled: false, deviceId: "local", name: "Laptop", address: "owner@laptop" }
  const setConnectionsEnabled = vi.fn().mockRejectedValueOnce(new Error("Reply unavailable")).mockResolvedValue(undefined)
  const api = actions({ setConnectionsEnabled })
  const view = (enabled: boolean) => <><Toaster /><ConnectionsSettings source={source({ ...management, enabled })} actions={api} /></>
  const { rerender } = render(view(false))
  await user.click(screen.getByRole("switch", { name: "Allow connections from other devices" }))
  const retry = await screen.findByRole("button", { name: "Retry" })
  rerender(view(true))
  await user.click(screen.getByRole("switch", { name: "Allow connections from other devices" }))
  await waitFor(() => expect(screen.getByRole("switch", { name: "Allow connections from other devices" })).toBeEnabled())
  await user.click(retry)
  expect(setConnectionsEnabled).toHaveBeenCalledTimes(2)
  expect(setConnectionsEnabled).toHaveBeenLastCalledWith(false)
})

it("keeps a removal Retry after unrelated device changes succeed", async () => {
  const user = userEvent.setup()
  const management = { enabled: false, deviceId: "local", name: "Laptop", address: "owner@laptop" }
  const setConnectionsEnabled = vi.fn().mockResolvedValue(undefined)
  const removeDevice = vi.fn().mockRejectedValueOnce(new Error("Connection could not be removed")).mockResolvedValue(undefined)
  const remotes = [{ id: "office", name: "Office", address: "owner@office", connected: false }, { id: "lab", name: "Lab", address: "owner@lab", connected: false }]
  render(<><Toaster /><ConnectionsSettings source={{ ...source(management), devices: remotes }} actions={actions({ setConnectionsEnabled, removeDevice })} /></>)
  await user.click(screen.getByRole("button", { name: "Remove connection to Office" }))
  const retry = await screen.findByRole("button", { name: "Retry" })
  await user.click(screen.getByRole("switch", { name: "Allow connections from other devices" }))
  await waitFor(() => expect(screen.getByRole("switch", { name: "Allow connections from other devices" })).toBeEnabled())
  await user.click(screen.getByRole("button", { name: "Remove connection to Lab" }))
  await waitFor(() => expect(removeDevice).toHaveBeenCalledTimes(2))
  await waitFor(() => expect(screen.getByRole("switch", { name: "Allow connections from other devices" })).toBeEnabled())
  await user.click(retry)
  expect(removeDevice).toHaveBeenCalledTimes(3)
  expect(removeDevice).toHaveBeenLastCalledWith("office")
})

it("ignores a device setting Retry after the settings controls unmount", async () => {
  const user = userEvent.setup()
  const management = { enabled: false, deviceId: "local", name: "Laptop", address: "owner@laptop" }
  const setConnectionsEnabled = vi.fn().mockRejectedValue(new Error("Reply unavailable"))
  const { rerender } = render(<><Toaster /><ConnectionsSettings source={source(management)} actions={actions({ setConnectionsEnabled })} /></>)
  await user.click(screen.getByRole("switch", { name: "Allow connections from other devices" }))
  const retry = await screen.findByRole("button", { name: "Retry" })
  rerender(<Toaster />)
  await user.click(retry)
  expect(setConnectionsEnabled).toHaveBeenCalledOnce()
})


it.each([false, true])("ignores connection completion after its form unmounts (replace=%s)", async replace => {
  let finish!: () => void
  const connect = vi.fn(() => new Promise<void>(resolve => { finish = resolve }))
  if (replace) connect.mockRejectedValueOnce(new Error("Office is already saved at owner@old-office"))
  const onClose = vi.fn()
  const { unmount } = render(<ConnectDeviceForm connect={connect} onClose={onClose} />)
  fireEvent.change(screen.getByRole("textbox", { name: "Device address" }), { target: { value: "owner@office" } })
  fireEvent.submit(screen.getByRole("form", { name: "Connect device" }))
  if (replace) {
    await screen.findByRole("alert")
    fireEvent.click(screen.getByRole("button", { name: "Use this address" }))
  }
  unmount()
  await act(async () => finish())
  expect(onClose).not.toHaveBeenCalled()
})
