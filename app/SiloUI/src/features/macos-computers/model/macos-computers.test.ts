import { describe, expect, it, vi } from "vitest"

import {
  createMacosComputersStore,
  isMacosCreating,
  macosResources,
  macosStateLabel,
  parseMacosComputersState,
  validateMacosRequest,
  type MacosComputer,
  type MacosComputersBackend,
} from "./macos-computers"

const computer: MacosComputer = { id: "a", name: "daily", cpus: 4, memoryGiB: 8, diskGiB: 64, osVersion: "26.6.2 (25G83)", state: "stopped", progress: null, detail: null, displayOpen: false }
const state = { supported: true, unsupportedReason: null, computers: [computer] }

describe("macOS computers payload", () => {
  it("parses the native state", () => {
    expect(parseMacosComputersState(state)).toEqual(state)
    expect(parseMacosComputersState({ supported: false, unsupportedReason: "Requires Apple silicon.", computers: [] }).supported).toBe(false)
  })

  it("rejects unknown states and out-of-range progress", () => {
    expect(() => parseMacosComputersState({ ...state, computers: [{ ...computer, state: "paused" }] })).toThrow()
    expect(() => parseMacosComputersState({ ...state, computers: [{ ...computer, progress: 1.5 }] })).toThrow()
    expect(() => parseMacosComputersState({ supported: true, computers: [] })).toThrow()
  })
})

describe("macOS computer presentation", () => {
  it("labels every state, with progress where it applies", () => {
    const label = (patch: Partial<MacosComputer>) => macosStateLabel({ ...computer, ...patch })
    expect(label({ state: "preparing" })).toBe("Preparing")
    expect(label({ state: "downloading", progress: 0.42 })).toBe("Downloading macOS 42%")
    expect(label({ state: "installing", progress: 0.634 })).toBe("Installing macOS 63%")
    expect(label({ state: "downloading", progress: null })).toBe("Downloading macOS")
    expect((["stopped", "starting", "running", "stopping", "failed"] as const).map(value => label({ state: value }))).toEqual(["Stopped", "Starting", "Running", "Stopping", "Failed"])
  })

  it("describes resources and creation", () => {
    expect(macosResources(computer)).toBe("4 CPUs · 8 GB memory · 64 GB disk")
    expect(isMacosCreating({ ...computer, state: "installing" })).toBe(true)
    expect(isMacosCreating(computer)).toBe(false)
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
    expect(store.getSnapshot().error).toBeTruthy()
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
})
