import { afterEach, describe, expect, it, vi } from "vitest"

import { createFixtureMacosComputersBackend, createFixtureMacosRemoteBackend } from "@/fixtures/macos-computers"
import { createMacosComputersStore, remoteMacosComputerId, remoteMacosPollMs, type MacosComputer, type MacosComputersState } from "./macos-computers"

const computer: MacosComputer = { id: "a", name: "build", cpus: 4, memoryGiB: 8, diskGiB: 64, osVersion: "26.6.2 (25G83)", state: "stopped", progress: null, detail: null, displayOpen: false, installed: true, setupComplete: true }
const hosting: MacosComputersState = { supported: true, unsupportedReason: null, computers: [computer], template: null, minDiskGiB: 64 }
const unsupported: MacosComputersState = { supported: false, unsupportedReason: "Requires Apple silicon.", computers: [], template: null, minDiskGiB: 32 }

afterEach(() => vi.useRealTimers())

function setup(devices: Record<string, MacosComputersState | Error>) {
  const remote = createFixtureMacosRemoteBackend(devices)
  const spies = { snapshot: vi.fn(remote.snapshot), create: vi.fn(remote.create), action: vi.fn(remote.action), openDisplay: vi.fn(remote.openDisplay) }
  const local = { ...createFixtureMacosComputersBackend([]), remote: spies, clipboard: vi.fn(async () => ({ action: "paste" as const, status: "pasted" as const, content: "text" as const, message: null })), createCheckpoint: vi.fn(async () => {}) }
  return { store: createMacosComputersStore(local), spies, local }
}

describe("macOS computers on other devices", () => {
  it("reads each connected device once, keeps unsupported devices without computers and skips disconnected ones", async () => {
    const { store, spies } = setup({ studio: hosting, linux: unsupported, away: hosting })
    const stop = store.subscribe(() => {})
    store.setRemoteDevices("test", [{ id: "studio", connected: true }, { id: "linux", connected: true }, { id: "away", connected: false }])
    await vi.waitFor(() => expect(store.getSnapshot().remote.linux?.state?.supported).toBe(false))
    expect(store.getSnapshot().remote.studio?.state?.computers).toHaveLength(1)
    expect(spies.snapshot.mock.calls.map(([id]) => id).sort()).toEqual(["linux", "studio"])
    expect(store.getSnapshot().remote.away).toBeUndefined()
    stop()
  })

  it("keeps the local state when a device fails and reads the device again at the polling interval", async () => {
    vi.useFakeTimers()
    const { store, spies, local } = setup({ studio: new Error("Device is not responding.") })
    local.read = async () => hosting
    const stop = store.subscribe(() => {})
    store.setRemoteDevices("test", [{ id: "studio", connected: true }])
    await vi.advanceTimersByTimeAsync(0)
    expect(store.getSnapshot().state?.computers).toHaveLength(1)
    expect(store.getSnapshot().remote.studio).toEqual({ state: null, error: "Device is not responding.", updating: false })
    await vi.advanceTimersByTimeAsync(remoteMacosPollMs)
    expect(spies.snapshot).toHaveBeenCalledTimes(2)
    stop()
  })

  it("stops polling when the last subscriber leaves and when the owner withdraws its devices", async () => {
    vi.useFakeTimers()
    const { store, spies } = setup({ studio: hosting })
    const stop = store.subscribe(() => {})
    store.setRemoteDevices("test", [{ id: "studio", connected: true }])
    await vi.advanceTimersByTimeAsync(0)
    expect(spies.snapshot).toHaveBeenCalledTimes(1)
    stop()
    await vi.advanceTimersByTimeAsync(remoteMacosPollMs * 3)
    expect(spies.snapshot).toHaveBeenCalledTimes(1)
    const stopAgain = store.subscribe(() => {})
    await vi.advanceTimersByTimeAsync(0)
    expect(spies.snapshot).toHaveBeenCalledTimes(2)
    store.setRemoteDevices("test", [])
    await vi.advanceTimersByTimeAsync(remoteMacosPollMs * 3)
    expect(spies.snapshot).toHaveBeenCalledTimes(2)
    stopAgain()
  })

  it("shows an owner that is updating, or that predates remote macOS, without an error", async () => {
    const { store, spies } = setup({ studio: hosting, old: hosting })
    spies.snapshot.mockImplementation(async deviceId => { throw deviceId === "studio" ? { code: "update_in_progress", message: "Updating." } : { code: "unsupported_remote_operation", message: "Update Silo." } })
    const stop = store.subscribe(() => {})
    store.setRemoteDevices("test", [{ id: "studio", connected: true }, { id: "old", connected: true }])
    await vi.waitFor(() => expect(store.getSnapshot().remote.old).toBeDefined())
    expect(store.getSnapshot().remote.studio).toEqual({ state: null, error: null, updating: true })
    expect(store.getSnapshot().remote.old).toEqual({ state: null, error: null, updating: false })
    stop()
  })

  it("routes actions and the screen by the device in the id, then reads the device again", async () => {
    const { store, spies, local } = setup({ studio: hosting })
    local.action = vi.fn(local.action)
    local.openDisplay = vi.fn(local.openDisplay)
    const stop = store.subscribe(() => {})
    store.setRemoteDevices("test", [{ id: "studio", connected: true }])
    await vi.waitFor(() => expect(store.getSnapshot().remote.studio?.state).not.toBeNull())
    const id = remoteMacosComputerId("studio", "a")
    await store.action(id, "start")
    expect(spies.action).toHaveBeenCalledWith("studio", "a", "start")
    expect(local.action).not.toHaveBeenCalled()
    await vi.waitFor(() => expect(store.getSnapshot().remote.studio?.state?.computers[0].state).toBe("running"))
    await store.openDisplay(id)
    expect(spies.openDisplay).toHaveBeenCalledWith("studio", "a")
    expect(local.openDisplay).not.toHaveBeenCalled()
    await store.action("local-id", "stop")
    expect(local.action).toHaveBeenCalledWith("local-id", "stop")
    stop()
  })

  it("creates on the chosen device and refuses clipboard and checkpoints for its computers", async () => {
    const { store, spies, local } = setup({ studio: hosting })
    local.create = vi.fn(local.create)
    const stop = store.subscribe(() => {})
    store.setRemoteDevices("test", [{ id: "studio", connected: true }])
    await vi.waitFor(() => expect(store.getSnapshot().remote.studio?.state).not.toBeNull())
    await store.createRemote("studio", { name: "fresh", cpus: 4, memoryGiB: 8, diskGiB: 64 })
    expect(spies.create).toHaveBeenCalledWith("studio", { name: "fresh", cpus: 4, memoryGiB: 8, diskGiB: 64 })
    expect(local.create).not.toHaveBeenCalled()
    expect(store.getSnapshot().remote.studio?.state?.computers.map(({ name }) => name)).toEqual(["build", "fresh"])
    const id = remoteMacosComputerId("studio", "a")
    await expect(store.clipboard(id, "paste-into")).rejects.toThrow("not supported")
    await expect(store.createCheckpoint(id, "x")).rejects.toThrow("not supported")
    expect(local.clipboard).not.toHaveBeenCalled()
    expect(local.createCheckpoint).not.toHaveBeenCalled()
    stop()
  })
})
