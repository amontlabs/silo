import { afterEach, describe, expect, it, vi } from "vitest"

import { applicationSourceForScenario } from "@/fixtures/application-scenarios"
import type { BackupState } from "@/features/application/model/backup-source"
import { createProductionSource, shareStructure, type ProductionBridge } from "./production-source"

const source = applicationSourceForScenario("running")
const backup: BackupState = { snapshotId: "one", availability: "available", requiredSpaceGB: 2, availableSpaceGB: 40, archives: [], operation: null }

afterEach(() => { vi.useRealTimers(); vi.restoreAllMocks() })

function bridge(custom: (command: string, args?: Record<string, unknown>) => Promise<unknown> | undefined = () => undefined) {
  const handlers = new Map<string, (event?: { payload: unknown }) => void>()
  const invoke = vi.fn(async (command: string, args?: Record<string, unknown>): Promise<unknown> => {
    const result = custom(command, args)
    if (result) return result
    if (command === "read_application_state") return structuredClone(source)
    if (command === "read_backup_state") return structuredClone(backup)
    if (command === "device_list") return []
    if (command === "read_operation_queue") return { running: [], waiting: [] }
    if (command === "read_network_state") return { computers: [] }
    return undefined
  })
  const listen = vi.fn(async (name: string, handler: (event?: { payload: unknown }) => void) => {
    handlers.set(name, handler)
    return () => { handlers.delete(name) }
  })
  const count = (command: string) => invoke.mock.calls.filter(([name]) => name === command).length
  return { bridge: { invoke, listen } as unknown as ProductionBridge, invoke, listen, handlers, count }
}

describe("structural sharing", () => {
  it("returns the previous value when nothing changed, treating undefined as absent", () => {
    const previous = { rows: [{ id: "a", detail: undefined }, { id: "b", nested: { value: 1 } }], total: 2 }
    const next = { rows: [{ id: "a" }, { id: "b", nested: { value: 1 } }], total: 2, extra: undefined }
    expect(shareStructure(previous, next)).toBe(previous)
  })

  it("keeps unchanged rows and replaces only the changed ones", () => {
    const previous = { rows: [{ id: "a", value: 1 }, { id: "b", value: 2 }], other: { kept: true } }
    const next = { rows: [{ id: "a", value: 1 }, { id: "b", value: 3 }], other: { kept: true } }
    const shared = shareStructure(previous, next)
    expect(shared).not.toBe(previous)
    expect(shared).toEqual(next)
    expect(shared.rows[0]).toBe(previous.rows[0])
    expect(shared.rows[1]).not.toBe(previous.rows[1])
    expect(shared.other).toBe(previous.other)
  })

  it("treats a changed length or removed key as a change", () => {
    const previous = { rows: [{ id: "a" }, { id: "b" }], gone: 1 }
    expect(shareStructure(previous, { rows: [{ id: "a" }], gone: 1 }).rows[0]).toBe(previous.rows[0])
    expect(shareStructure(previous, { rows: [{ id: "a" }, { id: "b" }] })).not.toBe(previous)
  })

  it("keeps every unchanged computer, activity and secret identical across a refresh that changes one computer", async () => {
    const two = structuredClone(source)
    two.computers = [two.computers[0], { ...structuredClone(two.computers[0]), configuration: { ...two.computers[0].configuration, id: "second", name: "second" } }]
    two.secrets = [{ id: "s1", name: "TOKEN", computers: [], allowedDomains: [], state: "active" }, { id: "s2", name: "OTHER", computers: [], allowedDomains: [], state: "active" }]
    let current = structuredClone(two)
    const mock = bridge(command => command === "read_application_state" ? Promise.resolve(structuredClone(current)) : undefined)
    const store = createProductionSource(mock.bridge)
    try {
      await store.initialize()
      const before = store.getSnapshot().source!
      const notified = vi.fn()
      store.subscribe(notified)
      await store.refresh()
      expect(store.getSnapshot().source).toBe(before)
      expect(notified).not.toHaveBeenCalled()
      current = structuredClone(two)
      current.computers[1] = { ...current.computers[1], stateDetail: "Different" }
      current.secrets[0] = { ...current.secrets[0], name: "RENAMED" }
      await store.refresh()
      const after = store.getSnapshot().source!
      expect(after).not.toBe(before)
      expect(after.computers[0]).toBe(before.computers[0])
      expect(after.computers[1]).not.toBe(before.computers[1])
      expect(after.computers[1].stateDetail).toBe("Different")
      expect(after.activities).toBe(before.activities)
      expect(after.secrets[0]).not.toBe(before.secrets[0])
      expect(after.secrets[1]).toBe(before.secrets[1])
      expect(notified).toHaveBeenCalledTimes(1)
    } finally { store.dispose() }
  })
})

describe("network reads", () => {
  it("does not read network services on refresh or on a network event until a consumer wants them", async () => {
    const mock = bridge()
    const store = createProductionSource(mock.bridge)
    try {
      await store.initialize()
      await store.refresh()
      mock.handlers.get("silo://network-state-changed")!()
      await vi.waitFor(() => expect(mock.listen).toHaveBeenCalled())
      expect(mock.count("read_network_state")).toBe(0)
    } finally { store.dispose() }
  })

  it("reads them with every refresh and event while watched, and stops when the watcher leaves", async () => {
    const mock = bridge()
    const store = createProductionSource(mock.bridge)
    try {
      await store.initialize()
      const stop = store.watchNetwork()
      await vi.waitFor(() => expect(mock.count("read_network_state")).toBe(1))
      await store.refresh()
      expect(mock.count("read_network_state")).toBe(2)
      mock.handlers.get("silo://network-state-changed")!()
      await vi.waitFor(() => expect(mock.count("read_network_state")).toBe(3))
      stop()
      stop()
      await store.refresh()
      expect(mock.count("read_network_state")).toBe(3)
    } finally { store.dispose() }
  })

  it("reads them on network events while a page keeps requesting them, then lets the interest lapse", async () => {
    vi.useFakeTimers()
    const mock = bridge()
    const store = createProductionSource(mock.bridge)
    try {
      await store.initialize()
      await store.applicationActions.refreshNetwork!()
      expect(mock.count("read_network_state")).toBe(1)
      mock.handlers.get("silo://network-state-changed")!()
      await vi.advanceTimersByTimeAsync(0)
      expect(mock.count("read_network_state")).toBe(2)
      await vi.advanceTimersByTimeAsync(31_000)
      mock.handlers.get("silo://network-state-changed")!()
      await vi.advanceTimersByTimeAsync(0)
      expect(mock.count("read_network_state")).toBe(2)
    } finally { store.dispose() }
  })
})

describe("operation queue polling", () => {
  it("reads the queue on each poll tick and recovers after a failed read", async () => {
    vi.useFakeTimers()
    let failing = false
    let queue = { running: [], waiting: [] } as unknown
    const mock = bridge(command => {
      if (command !== "read_operation_queue") return undefined
      return failing ? Promise.reject(new Error("queue unavailable")) : Promise.resolve(queue)
    })
    const store = createProductionSource(mock.bridge)
    try {
      await store.initialize()
      await vi.advanceTimersByTimeAsync(0)
      const reads = mock.count("read_operation_queue")
      failing = true
      await vi.advanceTimersByTimeAsync(10_000)
      expect(mock.count("read_operation_queue")).toBe(reads + 1)
      failing = false
      await vi.advanceTimersByTimeAsync(10_000)
      expect(mock.count("read_operation_queue")).toBe(reads + 2)
      queue = { running: [], waiting: [] }
    } finally { store.dispose() }
  })
})

describe("refresh gate", () => {
  it("lets polling resume after a hung read, and ignores the hung read's late result when a newer one applied", async () => {
    vi.useFakeTimers()
    let hung = false
    let release!: (value: unknown) => void
    const stale = structuredClone(source)
    stale.computers[0] = { ...stale.computers[0], stateDetail: "From the hung read" }
    const mock = bridge(command => {
      if (command === "read_application_state" && hung) { hung = false; return new Promise(resolve => { release = resolve }) }
      return undefined
    })
    const store = createProductionSource(mock.bridge)
    try {
      await store.initialize()
      await vi.advanceTimersByTimeAsync(0)
      hung = true
      const first = store.refresh()
      await vi.advanceTimersByTimeAsync(0)
      const reads = mock.count("read_application_state")
      // The gate keeps polling away while the read is young.
      await vi.advanceTimersByTimeAsync(20_000)
      expect(mock.count("read_application_state")).toBe(reads)
      await vi.advanceTimersByTimeAsync(50_000)
      await first
      expect(mock.count("read_application_state")).toBeGreaterThan(reads)
      const shown = store.getSnapshot().source!.computers[0].stateDetail
      release(structuredClone(stale))
      await vi.advanceTimersByTimeAsync(0)
      expect(store.getSnapshot().source!.computers[0].stateDetail).toBe(shown)
    } finally { store.dispose() }
  })
})

describe("returning to the window", () => {
  it("reads at most once for a focus and visibility change together, and not again within two seconds", async () => {
    vi.useFakeTimers()
    const mock = bridge()
    const store = createProductionSource(mock.bridge)
    try {
      await store.initialize()
      await vi.advanceTimersByTimeAsync(5_000)
      const reads = mock.count("read_application_state")
      window.dispatchEvent(new Event("focus"))
      document.dispatchEvent(new Event("visibilitychange"))
      await vi.advanceTimersByTimeAsync(0)
      expect(mock.count("read_application_state")).toBe(reads + 1)
      await vi.advanceTimersByTimeAsync(1_000)
      window.dispatchEvent(new Event("focus"))
      await vi.advanceTimersByTimeAsync(0)
      expect(mock.count("read_application_state")).toBe(reads + 1)
      await vi.advanceTimersByTimeAsync(1_500)
      window.dispatchEvent(new Event("focus"))
      await vi.advanceTimersByTimeAsync(0)
      expect(mock.count("read_application_state")).toBe(reads + 2)
    } finally { store.dispose() }
  })

  it("skips the read while another refresh is in flight", async () => {
    vi.useFakeTimers()
    let hold = false
    let release!: (value: unknown) => void
    const mock = bridge(command => {
      if (command === "read_application_state" && hold) return new Promise(resolve => { release = resolve })
      return undefined
    })
    const store = createProductionSource(mock.bridge)
    try {
      await store.initialize()
      await vi.advanceTimersByTimeAsync(5_000)
      hold = true
      const running = store.refresh()
      await vi.advanceTimersByTimeAsync(0)
      const reads = mock.count("read_application_state")
      window.dispatchEvent(new Event("focus"))
      await vi.advanceTimersByTimeAsync(0)
      expect(mock.count("read_application_state")).toBe(reads)
      release(structuredClone(source))
      await running
    } finally { store.dispose() }
  })
})

describe("live update subscription", () => {
  it("registers every listener before any registration finishes", async () => {
    const pending: Array<() => void> = []
    const mock = bridge()
    mock.listen.mockImplementation(() => new Promise<() => void>(resolve => { pending.push(() => resolve(() => {})) }))
    const store = createProductionSource(mock.bridge)
    try {
      const initialized = store.initialize()
      await vi.waitFor(() => expect(mock.listen).toHaveBeenCalledTimes(7))
      expect(mock.count("read_application_state")).toBe(0)
      pending.forEach(finish => finish())
      await initialized
      expect(mock.count("read_application_state")).toBeGreaterThan(0)
    } finally { store.dispose() }
  })
})

describe("secret mutations", () => {
  it("publishes the returned list and follows with one coalesced read", async () => {
    const secrets = [{ id: "s1", name: "TOKEN", computers: [], allowedDomains: [], state: "active" }]
    const mock = bridge(command => command === "save_secret" ? Promise.resolve(secrets) : undefined)
    const store = createProductionSource(mock.bridge)
    try {
      await store.initialize()
      const reads = mock.count("read_application_state")
      await store.applicationActions.saveSecret({ name: "TOKEN", value: "x", computers: [], allowedDomains: [] } as never)
      expect(store.getSnapshot().source!.secrets).toEqual(secrets)
      await vi.waitFor(() => expect(mock.count("read_application_state")).toBe(reads + 1))
    } finally { store.dispose() }
  })
})
