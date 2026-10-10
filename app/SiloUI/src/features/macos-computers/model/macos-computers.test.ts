import { describe, expect, it, vi } from "vitest"

import {
  canRetryMacosSetup,
  createMacosComputersStore,
  isMacosCreating,
  macosResources,
  macosStateLabel,
  parseMacosComputersState,
  validateMacosRequest,
  type MacosComputer,
  type MacosComputersBackend,
} from "./macos-computers"

const computer: MacosComputer = { id: "a", name: "daily", cpus: 4, memoryGiB: 8, diskGiB: 64, osVersion: "26.6.2 (25G83)", state: "stopped", progress: null, detail: null, displayOpen: false, installed: true, setupComplete: true }
const state = { supported: true, unsupportedReason: null, computers: [computer], template: { macosVersion: "26.6.2", build: "25G83", current: true }, minDiskGiB: 64 }

describe("macOS computers payload", () => {
  it("parses the native state", () => {
    expect(parseMacosComputersState(state)).toEqual(state)
    expect(parseMacosComputersState({ supported: false, unsupportedReason: "Requires Apple silicon.", computers: [], template: null, minDiskGiB: 32 }).supported).toBe(false)
  })

  it("rejects unknown states and out-of-range progress", () => {
    expect(() => parseMacosComputersState({ ...state, computers: [{ ...computer, state: "paused" }] })).toThrow()
    expect(() => parseMacosComputersState({ ...state, computers: [{ ...computer, progress: 1.5 }] })).toThrow()
    expect(() => parseMacosComputersState({ supported: true, computers: [] })).toThrow()
    const { template: _template, ...withoutTemplate } = state
    expect(() => parseMacosComputersState(withoutTemplate)).toThrow()
    expect(() => parseMacosComputersState({ ...state, minDiskGiB: 0 })).toThrow()
    const { setupComplete: _omitted, ...withoutSetup } = computer
    expect(() => parseMacosComputersState({ ...state, computers: [withoutSetup] })).toThrow()
    const { installed: _installed, ...withoutInstalled } = computer
    expect(() => parseMacosComputersState({ ...state, computers: [withoutInstalled] })).toThrow()
    expect(parseMacosComputersState({ ...state, computers: [{ ...computer, state: "setting-up", detail: "Creating the account", setupComplete: false }] }).computers[0].state).toBe("setting-up")
  })
})

describe("macOS computer presentation", () => {
  it("labels every state, with progress where it applies", () => {
    const label = (patch: Partial<MacosComputer>) => macosStateLabel({ ...computer, ...patch })
    expect(label({ state: "preparing" })).toBe("Preparing")
    expect(label({ state: "downloading", progress: 0.42 })).toBe("Downloading macOS 42%")
    expect(label({ state: "installing", progress: 0.634 })).toBe("Installing macOS 63%")
    expect(label({ state: "downloading", progress: null })).toBe("Downloading macOS")
    expect(label({ state: "setting-up", detail: "Creating the account" })).toBe("Setting up macOS")
    expect((["stopped", "starting", "running", "stopping", "failed"] as const).map(value => label({ state: value }))).toEqual(["Stopped", "Starting", "Running", "Stopping", "Failed"])
  })

  it("describes resources and creation", () => {
    expect(macosResources(computer)).toBe("4 CPUs · 8 GiB memory · 64 GiB disk")
    expect(isMacosCreating({ ...computer, state: "installing" })).toBe(true)
    expect(isMacosCreating(computer)).toBe(false)
  })
})

describe("macOS computer setup", () => {
  it("offers a retry only for an idle computer whose setup is unfinished", () => {
    const retry = (patch: Partial<MacosComputer>) => canRetryMacosSetup({ ...computer, ...patch })
    expect(retry({})).toBe(false)
    expect(retry({ setupComplete: false })).toBe(true)
    expect(retry({ setupComplete: false, state: "failed" })).toBe(true)
    expect(retry({ setupComplete: false, state: "failed", installed: false })).toBe(false)
    expect(retry({ setupComplete: false, state: "setting-up" })).toBe(false)
    expect(retry({ setupComplete: false, state: "running" })).toBe(false)
  })
})

describe("validateMacosRequest", () => {
  const valid = { name: "daily", cpus: 4, memoryGiB: 8, diskGiB: 64 }

  it("accepts the defaults", () => {
    expect(validateMacosRequest({ ...valid, name: "new-one" }, ["daily"], { maxCPUs: 10, maxMemoryGiB: 32 })).toEqual({})
  })

  it("checks the name format and uniqueness", () => {
    expect(validateMacosRequest({ ...valid, name: "Daily" }, []).name).toBeDefined()
    expect(validateMacosRequest({ ...valid, name: "1abc" }, []).name).toBeDefined()
    expect(validateMacosRequest({ ...valid, name: "a".repeat(33) }, []).name).toBeDefined()
    expect(validateMacosRequest({ ...valid, name: "a".repeat(32) }, [])).toEqual({})
    expect(validateMacosRequest(valid, ["daily"]).name).toBe("A macOS computer with this name exists.")
  })

  it("bounds resources by the minimums, the device and the disk range", () => {
    expect(validateMacosRequest({ ...valid, cpus: 1 }, []).cpus).toBeDefined()
    expect(validateMacosRequest({ ...valid, cpus: 11 }, [], { maxCPUs: 10 }).cpus).toBeDefined()
    expect(validateMacosRequest({ ...valid, cpus: 10 }, [], { maxCPUs: 10 }).cpus).toBeUndefined()
    expect(validateMacosRequest({ ...valid, memoryGiB: 3 }, []).memoryGiB).toBeDefined()
    expect(validateMacosRequest({ ...valid, memoryGiB: 33 }, [], { maxMemoryGiB: 32 }).memoryGiB).toBeDefined()
    expect(validateMacosRequest({ ...valid, diskGiB: 31 }, []).diskGiB).toBeDefined()
    expect(validateMacosRequest({ ...valid, diskGiB: 63 }, [], { minDiskGiB: 64 }).diskGiB).toContain("template of 64 GiB")
    expect(validateMacosRequest({ ...valid, diskGiB: 64 }, [], { minDiskGiB: 64 }).diskGiB).toBeUndefined()
    expect(validateMacosRequest({ ...valid, diskGiB: 32 }, [], { minDiskGiB: 16 }).diskGiB).toBeUndefined()
    expect(validateMacosRequest({ ...valid, diskGiB: 1025 }, []).diskGiB).toBeDefined()
    expect(validateMacosRequest({ ...valid, diskGiB: Number.NaN }, []).diskGiB).toBeDefined()
  })
})

describe("createMacosComputersStore", () => {
  function backend() {
    let handler: ((payload: unknown) => void) | undefined
    const unlisten = vi.fn()
    const value: MacosComputersBackend = {
      read: vi.fn(async () => state),
      create: vi.fn(async () => ({ ...computer, id: "b", name: "new", state: "preparing" })),
      action: vi.fn(async () => {}),
      openDisplay: vi.fn(async () => {}),
      deleteTemplate: vi.fn(async () => {}),
      createCheckpoint: vi.fn(async () => {}),
      restoreCheckpoint: vi.fn(async () => {}),
      forkCheckpoint: vi.fn(async () => {}),
      deleteCheckpoint: vi.fn(async () => {}),
      clipboard: vi.fn(async () => ({ action: "paste" as const, status: "pasted" as const })),
      listen: vi.fn(async next => { handler = next; return unlisten }),
    }
    return { value, unlisten, emit: (payload: unknown) => handler?.(payload) }
  }

  it("reads on first subscription, follows events and stops with the last listener", async () => {
    const { value, unlisten, emit } = backend()
    const store = createMacosComputersStore(value)
    const stop = store.subscribe(() => {})
    await vi.waitFor(() => expect(store.getSnapshot().state).toEqual(state))
    emit({ ...state, computers: [{ ...computer, state: "running" }] })
    expect(store.getSnapshot().state?.computers[0].state).toBe("running")
    stop()
    expect(unlisten).toHaveBeenCalled()
  })

  it("keeps the last state when an update is unreadable", async () => {
    const { value, emit } = backend()
    const store = createMacosComputersStore(value)
    const stop = store.subscribe(() => {})
    await vi.waitFor(() => expect(store.getSnapshot().state).toEqual(state))
    emit({ nonsense: true })
    expect(store.getSnapshot().state).toEqual(state)
    expect(store.getSnapshot().warning).toBeTruthy()
    stop()
  })

  it("adds the created computer when its event has not arrived", async () => {
    const { value } = backend()
    const store = createMacosComputersStore(value)
    const stop = store.subscribe(() => {})
    await vi.waitFor(() => expect(store.getSnapshot().state).toEqual(state))
    await store.create({ name: "new", cpus: 4, memoryGiB: 8, diskGiB: 64 })
    expect(store.getSnapshot().state?.computers.map(({ name }) => name)).toEqual(["daily", "new"])
    stop()
  })

  it("registers the listener before the first read", async () => {
    const order: string[] = []
    const { value } = backend()
    value.listen = vi.fn(async () => { order.push("listen"); return () => {} })
    value.read = vi.fn(async () => { order.push("read"); return state })
    const stop = createMacosComputersStore(value).subscribe(() => {})
    await vi.waitFor(() => expect(order).toEqual(["listen", "read"]))
    stop()
  })

  it("discards a read that finishes after a newer event", async () => {
    const { value, emit } = backend()
    let finish: (value: unknown) => void = () => {}
    value.read = vi.fn(() => new Promise(resolve => { finish = resolve }))
    const store = createMacosComputersStore(value)
    const stop = store.subscribe(() => {})
    await vi.waitFor(() => expect(value.read).toHaveBeenCalled())
    emit({ ...state, computers: [{ ...computer, state: "running" }] })
    finish(state)
    await Promise.resolve()
    await Promise.resolve()
    expect(store.getSnapshot().state?.computers[0].state).toBe("running")
    stop()
  })

  it("applies a read that finishes before any event", async () => {
    const { value, emit } = backend()
    const store = createMacosComputersStore(value)
    const stop = store.subscribe(() => {})
    await vi.waitFor(() => expect(store.getSnapshot().state).toEqual(state))
    emit({ ...state, computers: [] })
    expect(store.getSnapshot().state?.computers).toEqual([])
    stop()
  })

  it("retries a failed listener registration, warns meanwhile and reads after actions", async () => {
    vi.useFakeTimers()
    const { value } = backend()
    const listen = vi.fn()
      .mockRejectedValueOnce(new Error("no events"))
      .mockImplementation(async () => () => {})
    value.listen = listen
    const store = createMacosComputersStore(value)
    const stop = store.subscribe(() => {})
    await vi.advanceTimersByTimeAsync(0)
    expect(store.getSnapshot().warning).toMatch(/unavailable/)
    expect(store.getSnapshot().state).toEqual(state)
    await store.action("a", "start")
    await vi.advanceTimersByTimeAsync(0)
    expect(value.read).toHaveBeenCalledTimes(2)
    await vi.advanceTimersByTimeAsync(1_000)
    expect(listen).toHaveBeenCalledTimes(2)
    expect(store.getSnapshot().warning).toBeNull()
    stop()
    vi.useRealTimers()
  })

  it("keeps a failed first read as an error and refreshes on request", async () => {
    const { value } = backend()
    value.read = vi.fn().mockRejectedValueOnce("Virtualization is unavailable.").mockResolvedValue(state)
    const store = createMacosComputersStore(value)
    const stop = store.subscribe(() => {})
    await vi.waitFor(() => expect(store.getSnapshot().error).toBe("Virtualization is unavailable."))
    await store.refresh()
    expect(store.getSnapshot()).toMatchObject({ state, error: null })
    stop()
  })

  it("ignores a registration that finishes after its subscription ended", async () => {
    const { value } = backend()
    const registrations: Array<(unlisten: () => void) => void> = []
    value.listen = vi.fn(() => new Promise<() => void>(resolve => { registrations.push(resolve) }))
    let finish: (value: unknown) => void = () => {}
    value.read = vi.fn(() => new Promise(resolve => { finish = resolve }))
    const store = createMacosComputersStore(value)
    store.subscribe(() => {})()
    const stop = store.subscribe(() => {})
    registrations[1](() => {})
    await vi.waitFor(() => expect(value.read).toHaveBeenCalledTimes(1))
    registrations[0](() => {})
    await Promise.resolve()
    await Promise.resolve()
    finish(state)
    await vi.waitFor(() => expect(store.getSnapshot().state).toEqual(state))
    expect(value.read).toHaveBeenCalledTimes(1)
    stop()
  })

  it("applies only the newest of overlapping reads", async () => {
    const { value } = backend()
    const finishers: Array<(value: unknown) => void> = []
    const store = createMacosComputersStore(value)
    const stop = store.subscribe(() => {})
    await vi.waitFor(() => expect(store.getSnapshot().state).toEqual(state))
    value.read = vi.fn(() => new Promise(resolve => { finishers.push(resolve) }))
    const first = store.refresh()
    const second = store.refresh()
    finishers[1]({ ...state, computers: [{ ...computer, state: "running" }] })
    await second
    finishers[0](state)
    await first
    expect(store.getSnapshot().state?.computers[0].state).toBe("running")
    stop()
  })

  it("treats an unreadable first read as a failed read", async () => {
    const { value } = backend()
    value.read = vi.fn().mockResolvedValueOnce({ nonsense: true }).mockResolvedValue(state)
    const store = createMacosComputersStore(value)
    const stop = store.subscribe(() => {})
    await vi.waitFor(() => expect(store.getSnapshot().error).toMatch(/unreadable/))
    expect(store.getSnapshot().state).toBeNull()
    await store.refresh()
    expect(store.getSnapshot()).toMatchObject({ state, error: null })
    stop()
  })

  it("reads again after a malformed event and clears the warning", async () => {
    const { value, emit } = backend()
    const store = createMacosComputersStore(value)
    const stop = store.subscribe(() => {})
    await vi.waitFor(() => expect(store.getSnapshot().state).toEqual(state))
    value.read = vi.fn(async () => ({ ...state, computers: [{ ...computer, state: "running" }] }))
    emit({ nonsense: true })
    expect(store.getSnapshot().warning).toBeTruthy()
    await vi.waitFor(() => expect(store.getSnapshot().state?.computers[0].state).toBe("running"))
    expect(store.getSnapshot().warning).toBeNull()
    stop()
  })
})
