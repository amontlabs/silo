import { describe, expect, it, vi } from "vitest"

import { applicationSourceForScenario } from "@/fixtures/application-scenarios"
import type { BackupState } from "@/features/application/model/backup-source"
import { createProductionSource, type ProductionBridge } from "./production-source"

const source = applicationSourceForScenario("running")
const backup: BackupState = { snapshotId: "one", availability: "available", requiredSpaceGB: 2, availableSpaceGB: 40, archives: [], operation: null }

function bridge(options: { failListen?: (name: string) => boolean; invoke?: (command: string, args?: Record<string, unknown>) => Promise<unknown> | undefined } = {}) {
  const handlers = new Map<string, (event?: { payload: unknown }) => void>()
  const invoke = vi.fn(async (command: string, args?: Record<string, unknown>): Promise<unknown> => {
    const custom = options.invoke?.(command, args)
    if (custom) return custom
    if (command === "read_application_state") return structuredClone(source)
    if (command === "read_backup_state") return structuredClone(backup)
    if (command === "device_list") return []
    if (command === "read_operation_queue") return { running: [], waiting: [] }
    return undefined
  })
  const listen = vi.fn(async (name: string, handler: (event?: { payload: unknown }) => void) => {
    if (options.failListen?.(name)) throw new Error("event channel closed")
    handlers.set(name, handler)
    return () => { handlers.delete(name) }
  })
  const count = (command: string) => invoke.mock.calls.filter(([name]) => name === command).length
  return { bridge: { invoke, listen } as unknown as ProductionBridge, invoke, listen, handlers, count }
}

describe("production setup drain", () => {
  it("stops waiting for GitHub access when Quit drains setup and names the work meanwhile", async () => {
    const computer = source.computers[0].configuration.name
    const applying = { ...source.github, policyRevision: 3, computerOperations: [{ computer, status: "applying", message: "Applying access" }] }
    let resolveIdentity!: () => void
    const identity = new Promise<void>((resolve) => { resolveIdentity = resolve })
    const mock = bridge({ invoke: (command) => {
      if (command === "save_github_configuration" || command === "read_github_state") return Promise.resolve(structuredClone(applying))
      if (command === "configure_computer_identities") return identity.then(() => null)
      if (command === "verify_computer_identities") return Promise.resolve(false)
      if (command === "read_setup_activity") return Promise.resolve([])
      return undefined
    } })
    const store = createProductionSource(mock.bridge)
    try {
      await store.initialize()
      const request = {
        computerConfiguration: { schemaVersion: 1 as const, computers: source.computers.filter(({ device }) => !device).map(({ configuration }) => configuration) },
        applications: source.preferences,
        github: { connectionState: "connected" as const, computers: source.computers.filter(({ device }) => !device).map(({ configuration }) => ({ computer: configuration.name, repositories: [], identity: { name: "Test", email: "test@example.invalid", apply: true } })) },
      }
      const finished = store.finishSetup(request, vi.fn(async () => {}))
      void finished.catch(() => {})
      await vi.waitFor(() => expect(mock.count("configure_computer_identities")).toBe(1))
      let drained = false
      const drain = store.drainSetup().then(() => { drained = true })
      expect(store.getSnapshot().setupDrain).toBe("Finishing setup (applying Git identities, verifying GitHub access, saving setup)…")
      resolveIdentity()
      await vi.waitFor(() => expect(mock.count("save_github_configuration")).toBe(1))
      // The verification loop ends at once instead of polling for up to five minutes.
      await vi.waitFor(() => expect(drained).toBe(true), { timeout: 2000 })
      await drain
      await expect(finished).rejects.toThrow(/Silo is quitting/)
      expect(mock.count("read_github_state")).toBe(0)
      expect(store.getSnapshot().setupDrain).toBeUndefined()
    } finally {
      store.dispose()
    }
  })

  it.each(["quit", "dispose"])("wakes a GitHub access poll that is already waiting on %s", async reason => {
    vi.useFakeTimers()
    const computer = source.computers[0].configuration.name
    const applying = { ...source.github, policyRevision: 3, computerOperations: [{ computer, status: "applying", message: "Applying access" }] }
    const mock = bridge({ invoke: (command) => {
      if (command === "save_github_configuration" || command === "read_github_state") return Promise.resolve(structuredClone(applying))
      if (command === "configure_computer_identities") return Promise.resolve(null)
      if (command === "verify_computer_identities") return Promise.resolve(false)
      if (command === "read_setup_activity") return Promise.resolve([])
      return undefined
    } })
    const store = createProductionSource(mock.bridge)
    try {
      await store.initialize()
      const configurations = source.computers.filter(({ device }) => !device).map(({ configuration }) => configuration)
      const step = store.submitSetupStep("github", {
        computerConfiguration: { schemaVersion: 1, computers: configurations },
        applications: source.preferences,
        github: { connectionState: "connected", computers: configurations.map((configuration) => ({ computer: configuration.name, repositories: [], identity: { name: "Test", email: "test@example.invalid", apply: true } })) },
      })
      const outcome = expect(step).rejects.toThrow(reason === "quit" ? /Silo is quitting/ : /Silo closed/)
      await vi.advanceTimersByTimeAsync(1_200)
      const polls = mock.count("read_github_state")
      expect(polls).toBeGreaterThan(0)
      let drained = false
      if (reason === "quit") void store.drainSetup().then(() => { drained = true })
      else {
        store.dispose()
        void step.catch(() => { drained = true })
      }
      await vi.advanceTimersByTimeAsync(0)
      expect(drained).toBe(true)
      await outcome
      expect(mock.count("read_github_state")).toBe(polls)
      if (reason === "dispose") expect(vi.getTimerCount()).toBe(0)
    } finally {
      store.dispose()
      vi.useRealTimers()
    }
  })
})

describe("local state updating at launch", () => {
  const office = { id: "office", name: "Office Mac", address: "user@office" }
  function updating(options: { remotes: boolean; updatingReads?: number }) {
    let updatingReads = options.updatingReads ?? Number.POSITIVE_INFINITY
    return bridge({ invoke: (command, args) => {
      if (command === "read_application_state") return updatingReads-- > 0 ? Promise.reject({ code: "update_in_progress", message: "Computer settings are changing." }) : Promise.resolve(structuredClone(source))
      if (command === "device_list") return Promise.resolve(options.remotes ? [office] : [])
      if (command === "device_snapshot") return Promise.resolve(structuredClone(source))
      if (command === "connections_status") return Promise.resolve({ enabled: false, deviceId: "this-mac", name: "This Mac", address: "this-mac.local" })
      if (command === "read_application_shell") return Promise.resolve({ ...structuredClone(source), computers: [], runtimeRepair: { status: "unavailable", reason: String(args?.error) } })
      return undefined
    } })
  }

  it("shows connected devices, not a skeleton, while this device's computers update", async () => {
    const mock = updating({ remotes: true, updatingReads: 2 })
    const store = createProductionSource(mock.bridge)
    try {
      await store.initialize()
      await vi.waitFor(() => expect(store.getSnapshot().source).not.toBeNull())
      const snapshot = store.getSnapshot()
      expect(snapshot.loading).toBe(false)
      expect(snapshot.error).toBeNull()
      expect(snapshot.localUpdating).toBe(true)
      // Updating is not a runtime failure, and local changes wait for it.
      expect(snapshot.source?.runtimeRepair).toBeNull()
      expect(snapshot.source?.computerOperationsUnavailable).toMatch(/updating/)
      expect(snapshot.source?.computers.length).toBeGreaterThan(0)
      expect(snapshot.source?.computers.every((computer) => computer.device?.id === "office")).toBe(true)
      // Still updating: the shell stays.
      await store.refresh()
      expect(store.getSnapshot().localUpdating).toBe(true)
      // The update finished: the real local state replaces the shell.
      await store.refresh()
      expect(store.getSnapshot().localUpdating).toBe(false)
      expect(store.getSnapshot().source?.computers.some((computer) => !computer.device)).toBe(true)
    } finally { store.dispose() }
  })

  it("keeps loading without connected devices", async () => {
    const mock = updating({ remotes: false })
    const store = createProductionSource(mock.bridge)
    try {
      await store.initialize()
      expect(store.getSnapshot().source).toBeNull()
      expect(store.getSnapshot().loading).toBe(true)
      expect(mock.count("read_application_shell")).toBe(0)
    } finally { store.dispose() }
  })
})

describe("saved computer list for the loading skeleton", () => {
  const configuration = source.computers[0].configuration
  it.each([
    ["an unreadable list", () => Promise.reject(new Error("configuration locked")), []],
    ["an over-long list", () => Promise.resolve({ schemaVersion: 1, computers: Array.from({ length: 65 }, () => configuration) }), Array.from({ length: 65 }, () => configuration)],
    ["a newer schema with an unknown entry", () => Promise.resolve({ schemaVersion: 2, computers: [configuration, { id: "x", kind: "future" }] }), [configuration]],
  ] as const)("never fails startup on %s", async (_case, read, expected) => {
    const mock = bridge({ invoke: (command) => command === "read_computer_configuration" ? read() : undefined })
    const logged = vi.spyOn(console, "error").mockImplementation(() => {})
    const store = createProductionSource(mock.bridge)
    try {
      await expect(store.loadConfiguration()).resolves.toBeUndefined()
      expect(store.getSnapshot().savedConfigurations).toEqual(expected)
      if (expected.length === 0) expect(logged).toHaveBeenCalledWith("Silo saved computers:", "configuration locked")
    } finally { store.dispose() }
  })
})

describe("production source start-up", () => {
  it("subscribes, polls and refreshes on focus when Retry initializes again after a failed subscription", async () => {
    vi.useFakeTimers()
    let failures = 1
    const mock = bridge({ failListen: (name) => name === "silo://operation-queue-changed" && failures-- > 0 })
    const store = createProductionSource(mock.bridge)
    try {
      await expect(store.initialize()).rejects.toThrow("Silo could not subscribe to application updates: event channel closed")
      expect(store.getSnapshot().error).toMatch(/could not subscribe/)
      expect(mock.handlers.size).toBe(0)

      await store.initialize()
      expect(store.getSnapshot().source).not.toBeNull()
      expect(store.getSnapshot().error).toBeNull()
      expect(mock.handlers.has("silo://application-state-changed")).toBe(true)
      await vi.advanceTimersByTimeAsync(0)
      const reads = mock.count("read_application_state")
      await vi.advanceTimersByTimeAsync(10_000)
      expect(mock.count("read_application_state")).toBe(reads + 1)
      await vi.advanceTimersByTimeAsync(2_000)
      window.dispatchEvent(new Event("focus"))
      await vi.advanceTimersByTimeAsync(0)
      expect(mock.count("read_application_state")).toBe(reads + 2)
    } finally {
      store.dispose()
      vi.useRealTimers()
    }
  })

  it("only refreshes when initialized again while already live", async () => {
    vi.useFakeTimers()
    const mock = bridge()
    const store = createProductionSource(mock.bridge)
    try {
      await store.initialize()
      await vi.advanceTimersByTimeAsync(0)
      const listens = mock.listen.mock.calls.length
      const reads = mock.count("read_application_state")
      await store.initialize()
      expect(mock.listen.mock.calls.length).toBe(listens)
      expect(mock.count("read_application_state")).toBe(reads + 1)
      // One interval: a single poll per tick.
      await vi.advanceTimersByTimeAsync(10_000)
      expect(mock.count("read_application_state")).toBe(reads + 2)
    } finally {
      store.dispose()
      vi.useRealTimers()
    }
  })
})
