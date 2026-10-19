import { act, renderHook } from "@testing-library/react"
import { describe, expect, it, vi } from "vitest"

import { applicationSourceForScenario } from "@/fixtures/application-scenarios"
import type { BackupState } from "@/features/application/model/backup-source"
import { createProductionSource, parseApplicationSource, useProductionSource, type ProductionBridge } from "./production-source"

// State-handling behaviour of the production source: request deduplication,
// refresh ordering, polling and merges of partial native responses.

const toasts = vi.hoisted(() => ({ showOperationFailure: vi.fn() }))
vi.mock("@/lib/operation-toast", async (importOriginal) => ({ ...await importOriginal<typeof import("@/lib/operation-toast")>(), showOperationFailure: toasts.showOperationFailure }))

const source = applicationSourceForScenario("running")
const backup: BackupState = {
  snapshotId: "one",
  availability: "available",
  requiredSpaceGB: 2,
  availableSpaceGB: 40,
  archives: [],
  operation: null,
}

type Handler = (command: string, args?: Record<string, unknown>) => unknown

/** A native bridge that answers the baseline reads and delegates everything else. */
function bridge(handler: Handler = () => undefined) {
  const handlers = new Map<string, (event?: { payload: unknown }) => void>()
  const invoke = vi.fn(async (command: string, args?: Record<string, unknown>): Promise<unknown> => {
    const answer = await handler(command, args)
    if (answer !== undefined) return answer
    if (command === "read_application_state") return structuredClone(source)
    if (command === "read_backup_state") return structuredClone(backup)
    if (command === "read_setup_activity") return []
    if (command === "read_network_state") return { computers: [] }
    if (command === "read_operation_queue") return { running: [], waiting: [] }
    if (command === "device_list") return []
    if (command === "connections_status") return { enabled: false, deviceId: "local", name: "Laptop", address: "user@laptop" }
    return undefined
  })
  const listen = vi.fn(async (name: string, handler: (event?: { payload: unknown }) => void) => { handlers.set(name, handler); return () => { handlers.delete(name) } })
  return { native: { invoke, listen } as unknown as ProductionBridge, invoke, emit: (name: string, payload?: unknown) => handlers.get(name)?.({ payload }) }
}

const count = (invoke: ReturnType<typeof vi.fn>, name: string) => invoke.mock.calls.filter(([command]) => command === name).length

describe("computer configuration jobs", () => {
  it("does not report an empty configuration as saved before computer state loads", async () => {
    const mock = bridge()
    const store = createProductionSource(mock.native)
    try {
      await expect(store.configureConfigurations({ schemaVersion: 1, computers: [] })).rejects.toThrow("configuration has not loaded")
      expect(count(mock.invoke, "change_computer_configuration")).toBe(0)
      expect(store.getSnapshot().source).toBeNull()
    } finally { store.dispose() }
  })

  it("does not report an empty configuration as saved after the initial state read fails", async () => {
    const mock = bridge(command => { if (command === "read_application_state") throw new Error("Read unavailable") })
    const store = createProductionSource(mock.native)
    try {
      await store.initialize()
      await expect(store.configureConfigurations({ schemaVersion: 1, computers: [] })).rejects.toThrow("configuration has not loaded")
      expect(count(mock.invoke, "change_computer_configuration")).toBe(0)
    } finally { store.dispose() }
  })

  it("resolves a loaded empty configuration without submitting native changes", async () => {
    const mock = bridge(command => command === "read_application_state" ? { ...source, computers: [] } : undefined)
    const store = createProductionSource(mock.native)
    try {
      await store.initialize()
      expect(await store.configureConfigurations({ schemaVersion: 1, computers: [] })).toMatchObject({ computers: [] })
      expect(count(mock.invoke, "change_computer_configuration")).toBe(0)
    } finally { store.dispose() }
  })

  it("runs an identical retry again once the earlier one has finished (H-12)", async () => {
    const mock = bridge(command => command === "retry_computer_configuration" ? structuredClone(source) : undefined)
    const store = createProductionSource(mock.native)
    try {
      await store.initialize()
      const request = { schemaVersion: 1 as const, computers: store.getSnapshot().source!.computers.map(({ configuration }) => configuration) }
      const first = store.configureConfigurations(request, { kind: "retry" })
      // A repeat while the first is in flight joins it.
      expect(store.configureConfigurations(request, { kind: "retry" })).toBe(first)
      await first
      expect(count(mock.invoke, "retry_computer_configuration")).toBe(1)
      await store.configureConfigurations(request, { kind: "retry" })
      expect(count(mock.invoke, "retry_computer_configuration")).toBe(2)
    } finally { store.dispose() }
  })

  it("keeps an explicit Disable access when setup saves GitHub settings (H-39)", async () => {
    const dev = source.computers[0]
    const disabled = { ...source, computers: [dev], github: { ...source.github, state: "connected" as const, account: "octo", policyRevision: 3, accessEnabled: false } }
    const saved = { ...disabled.github, policyRevision: 4, computerOperations: [{ computer: dev.configuration.name, status: "succeeded", message: "Applied" }] }
    const mock = bridge(command => {
      if (command === "read_application_state") return structuredClone(disabled)
      if (command === "configure_computer_identities") return null
      if (command === "save_github_configuration") return structuredClone(saved)
    })
    const store = createProductionSource(mock.native)
    try {
      await store.initialize()
      await store.submitSetupStep("github", {
        computerConfiguration: { schemaVersion: 1, computers: [dev.configuration] },
        applications: source.preferences,
        github: { connectionState: "connected", computers: [{ computer: dev.configuration.name, repositories: [], identity: { name: "Test", email: "test@example.invalid", apply: true } }] },
      })
      // Setup never sends an access switch (the backend ignores one); it saves against the revision it saw.
      const save = mock.invoke.mock.calls.find(([command]) => command === "save_github_configuration")
      expect(save?.[1]).toEqual({ configuration: expect.objectContaining({ baseRevision: 3 }) })
      expect(save?.[1]?.configuration).not.toHaveProperty("accessEnabled")
    } finally { store.dispose() }
  })
})

const pushTarget = { repository: "octo/repo", branch: "main", commit: "0123456789abcdef0123456789abcdef01234567" }
const office = { id: "office", name: "Office Mac", address: "user@office" }
const studio = { id: "studio", name: "Studio", address: "user@studio" }
const remoteTarget = (deviceId: string) => `silo-remote:${deviceId}:${source.computers[0].configuration.id}`
function remoteSource(patch: Partial<(typeof source)["computers"][number]> = {}) {
  const remote = structuredClone(source)
  remote.computers = [{ ...remote.computers[0], ...patch }]
  return remote
}
function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>(done => { resolve = done })
  return { promise, resolve }
}

describe("remote device refresh", () => {
  it("refreshes each device independently and keeps polling while one is slow (H-05)", async () => {
    vi.useFakeTimers()
    let studioState: "running" | "stopped" = "running"
    const mock = bridge((command, args) => {
      if (command === "device_list") return [office, studio]
      if (command === "device_snapshot") return args?.deviceId === "office" ? new Promise(() => {}) : remoteSource({ state: studioState })
    })
    const store = createProductionSource(mock.native)
    const row = (deviceId: string) => store.getSnapshot().source?.computers.find(computer => computer.configuration.id === remoteTarget(deviceId))
    try {
      let initialized = false
      const started = store.initialize().then(() => { initialized = true })
      await vi.advanceTimersByTimeAsync(0)
      expect(row("studio")?.state).toBe("running")
      studioState = "stopped"
      // Polling runs before the first loads finish, and the slow device does not hold it back.
      await vi.advanceTimersByTimeAsync(10_000)
      expect(initialized).toBe(false)
      expect(row("studio")?.state).toBe("stopped")
      await vi.advanceTimersByTimeAsync(5_000)
      await started
      expect(store.getSnapshot().source?.devices?.find(device => device.id === "office")?.error).toContain("not responding")
      await vi.advanceTimersByTimeAsync(20_000)
      // The slow device keeps one read in flight instead of piling up SSH requests.
      expect(mock.invoke.mock.calls.filter(([command, args]) => command === "device_snapshot" && args?.deviceId === "office")).toHaveLength(1)
    } finally {
      store.dispose()
      vi.useRealTimers()
    }
  })

  it("marks a slow device's rows stale and clears that once it answers (H-05)", async () => {
    vi.useFakeTimers()
    const answer = deferred<unknown>()
    let reads = 0
    const mock = bridge(command => {
      if (command === "device_list") return [office]
      if (command === "device_snapshot") return ++reads === 1 ? remoteSource() : answer.promise
    })
    const store = createProductionSource(mock.native)
    const row = () => store.getSnapshot().source?.computers.find(computer => computer.configuration.id === remoteTarget("office"))
    try {
      await store.initialize()
      expect(row()?.freshness).toBe("fresh")
      const refreshing = store.applicationActions.refreshRepositories!()
      await vi.advanceTimersByTimeAsync(15_000)
      await refreshing
      expect(row()).toMatchObject({ freshness: "stale", stateDetail: "Updating…" })
      answer.resolve(remoteSource({ stateDetail: "Answered" }))
      await vi.advanceTimersByTimeAsync(0)
      expect(row()).toMatchObject({ freshness: "fresh", stateDetail: "Answered" })
      expect(store.getSnapshot().source?.devices?.[0].error).toBeUndefined()
    } finally {
      store.dispose()
      vi.useRealTimers()
    }
  })

  it("skips network reads for an offline device (H-05)", async () => {
    const mock = bridge(command => {
      if (command === "device_list") return [office]
      if (command === "device_snapshot") throw new Error("Connection timed out")
    })
    const store = createProductionSource(mock.native)
    try {
      await store.initialize()
      expect(store.getSnapshot().source?.devices?.[0].connected).toBe(false)
      mock.invoke.mockClear()
      await store.applicationActions.refreshNetwork!()
      expect(count(mock.invoke, "read_network_state")).toBe(1)
      expect(count(mock.invoke, "remote_network_state")).toBe(0)
    } finally { store.dispose() }
  })

  it("accepts a repeated remote action while the follow-up refresh is still running (H-05)", async () => {
    let reads = 0
    const mock = bridge(command => {
      if (command === "device_list") return [office]
      if (command === "device_snapshot") return ++reads === 1 ? remoteSource() : new Promise(() => {})
      if (command === "remote_computer_action") return remoteSource()
    })
    const store = createProductionSource(mock.native)
    try {
      await store.initialize()
      store.applicationActions.stopComputer(remoteTarget("office"))
      await vi.waitFor(() => expect(store.getSnapshot().source?.computers.find(computer => computer.configuration.id === remoteTarget("office"))?.lifecycleAction).toBeUndefined())
      await vi.waitFor(() => expect(reads).toBe(2))
      store.applicationActions.stopComputer(remoteTarget("office"))
      expect(count(mock.invoke, "remote_computer_action")).toBe(2)
    } finally { store.dispose() }
  })

  it("does not let a read that started before a remote edit revert it (H-06)", async () => {
    vi.useFakeTimers()
    const stale = deferred<unknown>()
    let reads = 0
    let current = remoteSource({ purpose: "Before" })
    const mock = bridge(command => {
      if (command === "device_list") return [office]
      if (command === "device_snapshot") return ++reads === 2 ? stale.promise : structuredClone(current)
      if (command === "remote_upsert_computer") { current = remoteSource({ purpose: "Edited" }); return structuredClone(current) }
    })
    const store = createProductionSource(mock.native)
    const purpose = () => store.getSnapshot().source?.computers.find(computer => computer.configuration.id === remoteTarget("office"))?.purpose
    try {
      await store.initialize()
      void store.refresh()
      await vi.advanceTimersByTimeAsync(0)
      expect(reads).toBe(2)
      const configuration = { ...source.computers[0].configuration, id: remoteTarget("office") }
      await store.applicationActions.saveRemoteComputer!("office", configuration, configuration)
      expect(purpose()).toBe("Edited")
      const shown: Array<string | undefined> = []
      const unsubscribe = store.subscribe(() => shown.push(purpose()))
      stale.resolve(remoteSource({ purpose: "Before" }))
      // Drain the overlapping read and its follow-up before checking every published value.
      await vi.advanceTimersByTimeAsync(0)
      expect(reads).toBeGreaterThanOrEqual(3)
      unsubscribe()
      expect(shown).not.toContain("Before")
      expect(purpose()).toBe("Edited")
    } finally { store.dispose() }
  })

  it("does not bring back a device removed while the list was being read (H-06)", async () => {
    vi.useFakeTimers()
    const list = deferred<unknown>()
    let lists = 0
    let listedDevices = [office]
    const mock = bridge(command => {
      if (command === "device_list") return ++lists === 2 ? list.promise : structuredClone(listedDevices)
      if (command === "device_snapshot") return remoteSource()
      if (command === "remove_device") { listedDevices = []; return null }
    })
    const store = createProductionSource(mock.native)
    try {
      await store.initialize()
      const refreshing = store.refresh()
      await vi.advanceTimersByTimeAsync(0)
      expect(lists).toBe(2)
      await store.applicationActions.removeDevice!("office")
      list.resolve([office])
      await refreshing
      await vi.advanceTimersByTimeAsync(0)
      expect(lists).toBe(3)
      expect(store.getSnapshot().source?.devices).toEqual([])
      expect(store.getSnapshot().source?.computers.some(computer => computer.device)).toBe(false)
    } finally { store.dispose() }
  })

  it("lists a newly connected device when connecting resolves, even during a refresh (H-06)", async () => {
    const list = deferred<unknown>()
    let lists = 0
    let listedDevices: typeof office[] = []
    const mock = bridge(command => {
      if (command === "device_list") return ++lists === 2 ? list.promise : structuredClone(listedDevices)
      if (command === "device_snapshot") return remoteSource()
      if (command === "connect_device") { listedDevices = [office]; return office }
    })
    const store = createProductionSource(mock.native)
    try {
      await store.initialize()
      void store.refresh()
      await vi.waitFor(() => expect(lists).toBe(2))
      const connecting = store.applicationActions.connectDevice!("user@office")
      list.resolve([])
      await connecting
      expect(store.getSnapshot().source?.devices?.map(device => device.id)).toEqual(["office"])
      expect(store.getSnapshot().source?.computers.some(computer => computer.configuration.id === remoteTarget("office"))).toBe(true)
    } finally { store.dispose() }
  })

  it("lists a newly connected device after a superseded device-list read fails", async () => {
    const late = deferred<unknown>()
    let lists = 0
    let listedDevices: typeof office[] = []
    const mock = bridge(command => {
      if (command === "device_list") return ++lists === 2 ? late.promise.then(() => { throw new Error("Old list unavailable") }) : structuredClone(listedDevices)
      if (command === "device_snapshot") return remoteSource()
      if (command === "connect_device") { listedDevices = [office]; return office }
    })
    const store = createProductionSource(mock.native)
    try {
      await store.initialize()
      const refresh = store.applicationActions.refreshRepositories!()
      await vi.waitFor(() => expect(lists).toBe(2))
      const connecting = store.applicationActions.connectDevice!("user@office")
      await vi.waitFor(() => expect(count(mock.invoke, "connections_status")).toBeGreaterThan(2))
      late.resolve(null)
      await connecting
      await refresh
      expect(store.getSnapshot().source?.devices?.map(device => device.id)).toEqual(["office"])
      expect(store.getSnapshot().source?.devicesError).toBeUndefined()
    } finally { store.dispose() }
  })

  it("reports a failed device list separately from Connections (H-22)", async () => {
    let failList = false
    const mock = bridge(command => {
      if (command === "device_list") { if (failList) throw new Error("listedDevices file unreadable"); return [office] }
      if (command === "device_snapshot") return remoteSource()
      if (command === "connections_status") return { enabled: true, deviceId: "local", name: "Laptop", address: "user@laptop" }
    })
    const store = createProductionSource(mock.native)
    try {
      await store.initialize()
      failList = true
      await store.refresh()
      await vi.waitFor(() => expect(store.getSnapshot().source?.devicesError).toContain("listedDevices file unreadable"))
      expect(store.getSnapshot().source?.connectionsError).toBeUndefined()
      expect(store.getSnapshot().source?.connections?.enabled).toBe(true)
      expect(store.getSnapshot().source?.devices?.map(device => device.id)).toEqual(["office"])
      failList = false
      await store.refresh()
      await vi.waitFor(() => expect(store.getSnapshot().source?.devicesError).toBeUndefined())
    } finally { store.dispose() }
  })

  it("chains one repository refresh for concurrent requests behind a plain read (H-23)", async () => {
    const plain = deferred<unknown>()
    let reads = 0
    const mock = bridge(command => {
      if (command === "device_list") return [office]
      if (command === "device_snapshot") return ++reads === 2 ? plain.promise : remoteSource()
    })
    const store = createProductionSource(mock.native)
    try {
      await store.initialize()
      void store.refresh()
      await vi.waitFor(() => expect(reads).toBe(2))
      const first = store.applicationActions.refreshRepositories!()
      const second = store.applicationActions.refreshRepositories!()
      plain.resolve(remoteSource())
      await Promise.all([first, second])
      const repositoryReads = mock.invoke.mock.calls.filter(([command, args]) => command === "device_snapshot" && args?.refreshRepositories === true)
      expect(repositoryReads).toHaveLength(1)
      expect(reads).toBe(3)
    } finally { store.dispose() }
  })
})

describe("repository push status", () => {
  const pushOf = (store: ReturnType<typeof createProductionSource>, computer = "dev") => store.getSnapshot().source?.repositoryPushOperations.find(operation => operation.computer === computer && operation.repositoryPath === "/workspace/repo")

  it("backs off unanswered status checks and ends in a dismissible unknown result (H-08)", async () => {
    vi.useFakeTimers()
    const mock = bridge(command => {
      if (command === "start_repository_push" || command === "repository_push_status") throw new Error("connection lost")
      if (command === "dismiss_repository_push") throw new Error("still unreachable")
    })
    const store = createProductionSource(mock.native)
    const checks = () => count(mock.invoke, "start_repository_push") + count(mock.invoke, "repository_push_status")
    try {
      await store.initialize()
      store.applicationActions.pushRepository("dev", "/workspace/repo", pushTarget)
      await vi.advanceTimersByTimeAsync(0)
      expect(pushOf(store)).toMatchObject({ status: "pushing", message: expect.stringContaining("connection lost") })
      await vi.advanceTimersByTimeAsync(14_000)
      // 0 s, then 4 s and 8 s later, rather than every 2 s.
      expect(checks()).toBe(3)
      await vi.advanceTimersByTimeAsync(10 * 60_000)
      expect(checks()).toBe(8)
      expect(pushOf(store)).toMatchObject({ status: "unknown", message: expect.stringContaining("Check the branch on GitHub") })
      // The result survives a refresh whose host reports nothing for it.
      await store.refresh()
      expect(pushOf(store)?.status).toBe("unknown")
      await vi.advanceTimersByTimeAsync(10 * 60_000)
      expect(checks()).toBe(8)
      store.applicationActions.dismissRepositoryPush!("dev", "/workspace/repo")
      expect(pushOf(store)).toBeUndefined()
      await vi.advanceTimersByTimeAsync(0)
      await store.refresh()
      expect(pushOf(store)).toBeUndefined()
    } finally {
      store.dispose()
      vi.useRealTimers()
    }
  })

  it("stops polling and drops the pending push when its computer is deleted (H-08)", async () => {
    vi.useFakeTimers()
    let deleted = false
    const mock = bridge(command => {
      if (command === "read_application_state" && deleted) return { ...structuredClone(source), computers: source.computers.filter(computer => computer.configuration.name !== "dev") }
      if (command === "start_repository_push" || command === "repository_push_status") return { operationId: "push-1", status: "pushing" }
    })
    const store = createProductionSource(mock.native)
    try {
      await store.initialize()
      store.applicationActions.pushRepository("dev", "/workspace/repo", pushTarget)
      await vi.advanceTimersByTimeAsync(4_000)
      expect(pushOf(store)?.status).toBe("pushing")
      deleted = true
      await store.refresh()
      expect(pushOf(store)).toBeUndefined()
      const polls = count(mock.invoke, "repository_push_status")
      await vi.advanceTimersByTimeAsync(60_000)
      expect(count(mock.invoke, "repository_push_status")).toBe(polls)
    } finally {
      store.dispose()
      vi.useRealTimers()
    }
  })

  it("stops polling a remote push when its device is removed (H-08)", async () => {
    vi.useFakeTimers()
    const mock = bridge(command => {
      if (command === "device_list") return [office]
      if (command === "device_snapshot") return remoteSource()
      if (command === "start_repository_push" || command === "repository_push_status") throw new Error("host unreachable")
    })
    const store = createProductionSource(mock.native)
    try {
      await store.initialize()
      store.applicationActions.pushRepository(remoteTarget("office"), "/workspace/repo", pushTarget)
      await vi.advanceTimersByTimeAsync(0)
      expect(pushOf(store, remoteTarget("office"))?.status).toBe("pushing")
      await store.applicationActions.removeDevice!("office")
      expect(pushOf(store, remoteTarget("office"))).toBeUndefined()
      const polls = count(mock.invoke, "repository_push_status") + count(mock.invoke, "start_repository_push")
      await vi.advanceTimersByTimeAsync(10 * 60_000)
      expect(count(mock.invoke, "repository_push_status") + count(mock.invoke, "start_repository_push")).toBe(polls)
    } finally {
      store.dispose()
      vi.useRealTimers()
    }
  })
})

describe("derived view", () => {
  it("hides a reported failure while its retry waits, across refreshes (H-30)", async () => {
    const failed = structuredClone(source)
    failed.computers[0] = { ...failed.computers[0], state: "stopped", lifecycleFailure: "Start failed: not enough memory" }
    const action = deferred<unknown>()
    const mock = bridge(command => {
      if (command === "read_application_state") return structuredClone(failed)
      if (command === "computer_action") return action.promise
    })
    const store = createProductionSource(mock.native)
    const dev = () => store.getSnapshot().source?.computers.find(computer => computer.configuration.name === "dev")
    try {
      await store.initialize()
      expect(dev()?.lifecycleFailure).toContain("not enough memory")
      store.applicationActions.startComputer("dev")
      expect(dev()).toMatchObject({ lifecycleAction: "start", lifecycleFailure: undefined })
      // The queued action has not begun, so the runtime still reports the old failure.
      await store.refresh()
      expect(dev()).toMatchObject({ lifecycleAction: "start", lifecycleFailure: undefined })
      action.resolve(structuredClone(source))
      await vi.waitFor(() => expect(dev()?.lifecycleAction).toBeUndefined())
    } finally { store.dispose() }
  })

  it("derives remote rows from their device without writing them back (H-30)", async () => {
    const mock = bridge(command => {
      if (command === "device_list") return [office]
      if (command === "device_snapshot") return remoteSource()
    })
    const store = createProductionSource(mock.native)
    try {
      await store.initialize()
      for (let index = 0; index < 3; index++) await store.refresh()
      const remote = store.getSnapshot().source?.computers.filter(computer => computer.device) ?? []
      expect(remote.map(computer => computer.configuration.id)).toEqual([remoteTarget("office")])
      expect(store.getSnapshot().source?.activities.filter(activity => activity.id.startsWith("silo-remote-activity:")).length).toBe(source.activities.length)
    } finally { store.dispose() }
  })
})

describe("mutation responses", () => {
  it("keeps GitHub, repositories and pushes that a mutation response omits (H-36)", async () => {
    const full = structuredClone(source)
    full.github = { ...full.github, state: "connected", account: "octo", policyRevision: 5, repositoryCatalog: ["acme/silo"], repositoryCatalogStatus: { status: "available" } }
    full.repositoryPushOperations = [{ computer: "dev", repositoryPath: "acme/silo", commitCount: 1, status: "failed", message: "rejected" }]
    // The shape `read_application_state_with` returns for a mutation today.
    const placeholder = { ...structuredClone(full), github: { state: "disconnected" }, repositoryPushOperations: [], computers: full.computers.map(computer => ({ ...computer, repositories: [] })) }
    let reads = 0
    const mock = bridge(command => {
      if (command === "read_application_state") return ++reads === 1 ? structuredClone(full) : new Promise(() => {})
      if (command === "computer_action") return structuredClone(placeholder)
    })
    const store = createProductionSource(mock.native)
    try {
      await store.initialize()
      store.applicationActions.stopComputer("dev")
      await vi.waitFor(() => expect(count(mock.invoke, "read_application_state")).toBe(2))
      const view = store.getSnapshot().source!
      expect(view.github).toMatchObject({ state: "connected", account: "octo", policyRevision: 5 })
      expect(view.computers.find(computer => computer.configuration.name === "dev")?.repositories).toEqual(full.computers[0].repositories)
      expect(view.repositoryPushOperations).toEqual(full.repositoryPushOperations)
    } finally { store.dispose() }
  })

  it("keeps a remote device's enrichment across a remote edit response (H-36)", async () => {
    const remote = remoteSource()
    remote.github = { ...remote.github, state: "connected", account: "octo", policyRevision: 2 }
    const edited = { ...structuredClone(remote), github: { state: "disconnected" }, computers: remote.computers.map(computer => ({ ...computer, purpose: "Edited", repositories: [] })) }
    let reads = 0
    const mock = bridge(command => {
      if (command === "device_list") return [office]
      if (command === "device_snapshot") return ++reads === 1 ? structuredClone(remote) : new Promise(() => {})
      if (command === "remote_upsert_computer") return structuredClone(edited)
    })
    const store = createProductionSource(mock.native)
    try {
      await store.initialize()
      const configuration = { ...source.computers[0].configuration, id: remoteTarget("office") }
      await store.applicationActions.saveRemoteComputer!("office", configuration, configuration)
      const row = store.getSnapshot().source?.computers.find(computer => computer.configuration.id === remoteTarget("office"))
      expect(row).toMatchObject({ purpose: "Edited", repositories: remote.computers[0].repositories })
    } finally { store.dispose() }
  })
})

describe("overlapping lifecycle responses", () => {
  it.each([false, true])("preserves both completions and newer same-computer intent (remote=%s)", async remote => {
    const initial = structuredClone(source)
    initial.computers = initial.computers.slice(0, 2).map(row => ({ ...row, state: "running" }))
    const [a, b] = initial.computers
    const target = (row: typeof a) => remote ? `silo-remote:office:${row.configuration.id}` : row.configuration.name
    const older = deferred<unknown>()
    const newer = deferred<unknown>()
    const restarted = deferred<unknown>()
    let reads = 0
    const mock = bridge((command, args) => {
      if (command === "device_list") return remote ? [office] : []
      if (command === (remote ? "device_snapshot" : "read_application_state")) return ++reads === 1 ? initial : new Promise(() => {})
      if (command === (remote ? "remote_computer_action" : "computer_action")) {
        if (args?.action === "start") return restarted.promise
        return (args?.name === a.configuration.name ? older : newer).promise
      }
    })
    const store = createProductionSource(mock.native)
    const row = (computer: typeof a) => store.getSnapshot().source?.computers.find(item => item.configuration.id === (remote ? target(computer) : computer.configuration.id))
    const result = (aState: "running" | "stopped", bState: "running" | "stopped") => ({ ...initial, computers: [{ ...a, state: aState }, { ...b, state: bState }] })
    try {
      await store.initialize()
      store.applicationActions.stopComputer(target(a))
      store.applicationActions.stopComputer(target(b))
      newer.resolve(result("running", "stopped"))
      await vi.waitFor(() => { expect(row(b)?.state).toBe("stopped"); expect(row(b)?.lifecycleAction).toBeUndefined() })
      older.resolve(result("stopped", "running"))
      await vi.waitFor(() => { expect(row(a)?.state).toBe("stopped"); expect(row(a)?.lifecycleAction).toBeUndefined() })
      expect(row(b)?.state).toBe("stopped")
      store.applicationActions.startComputer(target(b))
      restarted.resolve(result("running", "running"))
      await vi.waitFor(() => { expect(row(b)?.state).toBe("running"); expect(row(b)?.lifecycleAction).toBeUndefined() })
      expect(row(a)?.state).toBe("stopped")
    } finally { store.dispose() }
  })
})

describe("Connections response ordering", () => {
  it.each([
    { when: "before", fails: false }, { when: "before", fails: true },
    { when: "during", fails: false }, { when: "during", fails: true },
  ])("ignores an older status reply started $when a toggle (failure: $fails)", async ({ when, fails }) => {
    const status = { enabled: false, deviceId: "local", name: "Laptop", address: "user@laptop" }
    const late = deferred<unknown>()
    const toggle = deferred<unknown>()
    let delayed = false
    const mock = bridge(command => {
      if (command === "connections_status") return delayed ? late.promise.then(value => { if (fails) throw new Error("Old status unavailable"); return value }) : status
      if (command === "set_connections_enabled") return toggle.promise
    })
    const store = createProductionSource(mock.native)
    try {
      await store.initialize()
      delayed = true
      const changing = when === "during" ? store.applicationActions.setConnectionsEnabled!(true) : undefined
      const refresh = store.applicationActions.refreshRepositories!()
      await vi.waitFor(() => expect(count(mock.invoke, "connections_status")).toBeGreaterThan(1))
      const saving = changing ?? store.applicationActions.setConnectionsEnabled!(true)
      toggle.resolve({ ...status, enabled: true })
      await saving
      expect(store.getSnapshot().source?.connections?.enabled).toBe(true)
      late.resolve(status)
      await refresh
      expect(store.getSnapshot().source?.connections?.enabled).toBe(true)
      expect(store.getSnapshot().source?.connectionsError).toBeUndefined()
    } finally { store.dispose() }
  })
})

describe("remote configuration mutation response ordering", () => {
  it.each(["edit", "delete", "add"] as const)("preserves a sibling's newer lifecycle result after a late remote %s", async kind => {
    const initial = structuredClone(source)
    initial.computers = initial.computers.slice(0, 2).map(row => ({ ...row, state: "running" }))
    const [a, b] = initial.computers
    const late = deferred<unknown>()
    let reads = 0
    const newer = { ...initial, computers: [a, { ...b, state: "stopped" }] }
    const mock = bridge(command => {
      if (command === "device_list") return [office]
      if (command === "device_snapshot") return ++reads === 1 ? initial : new Promise(() => {})
      if (command === "remote_computer_action") return newer
      if (command === "remote_upsert_computer" || command === "remote_delete_computer") return late.promise
    })
    const store = createProductionSource(mock.native)
    const target = (id: string) => `silo-remote:office:${id}`
    const row = (id: string) => store.getSnapshot().source?.computers.find(item => item.configuration.id === target(id))
    try {
      await store.initialize()
      const configuration = { ...a.configuration, id: target(kind === "add" ? "new-remote" : a.configuration.id), name: kind === "add" ? "new-remote" : a.configuration.name }
      const mutation = kind === "delete" ? store.applicationActions.deleteRemoteComputer!("office", configuration)
        : store.applicationActions.saveRemoteComputer!("office", configuration, kind === "add" ? undefined : configuration)
      store.applicationActions.stopComputer(target(b.configuration.id))
      await vi.waitFor(() => {
        expect(row(b.configuration.id)?.state).toBe("stopped")
        expect(row(b.configuration.id)?.lifecycleAction).toBeUndefined()
      })
      const older = structuredClone(initial)
      if (kind === "delete") older.computers = [b]
      else if (kind === "edit") older.computers[0].purpose = "Edited"
      else older.computers.push({ ...a, configuration: { ...a.configuration, id: "new-remote", name: "new-remote" }, state: "stopped" })
      late.resolve(older)
      await mutation
      expect(row(b.configuration.id)?.state).toBe("stopped")
      if (kind === "delete") expect(row(a.configuration.id)).toBeUndefined()
      else if (kind === "edit") expect(row(a.configuration.id)?.purpose).toBe("Edited")
      else expect(row("new-remote")).toMatchObject({ state: "stopped", configuration: { name: "new-remote" } })
    } finally { store.dispose() }
  })
})

describe("SSH save response ordering", () => {
  it.each([false, true])("preserves independent sibling SSH saves when responses finish in reverse order (remote=%s)", async remote => {
    const initial = structuredClone(source)
    initial.computers = initial.computers.slice(0, 2)
    const target = (computer: typeof initial.computers[number]) => remote ? `silo-remote:office:${computer.configuration.id}` : computer.configuration.name
    const rows = initial.computers.map(computer => ({ computer: target(computer), enabled: true, port: 2222, bindAddress: "127.0.0.1", keys: [], state: "listening", message: null, fingerprint: null, deviceName: "Laptop", addresses: [] }))
    const older = deferred<unknown>()
    const newer = deferred<unknown>()
    const mock = bridge((command, args) => {
      if (command === "device_list") return remote ? [office] : []
      if (command === "device_snapshot") return initial
      if (command === "read_ssh_access_state") return { computers: remote ? [] : rows }
      if (command === "remote_ssh_access_state") return { computers: rows }
      if (command === "save_ssh_access" || command === "remote_save_ssh_access") return args?.port === 2223 ? older.promise : newer.promise
    })
    const store = createProductionSource(mock.native)
    const row = (computer: string) => store.getSnapshot().source?.sshAccess?.computers.find(item => item.computer === computer)
    try {
      await store.initialize()
      await store.applicationActions.refreshSshAccess!()
      const request = { enabled: true, bindAddress: "127.0.0.1", keys: [] }
      const first = store.applicationActions.saveSshAccess!({ ...request, computer: rows[0].computer, port: 2223 })
      const second = store.applicationActions.saveSshAccess!({ ...request, computer: rows[1].computer, port: 2224 })
      newer.resolve({ computers: [rows[0], { ...rows[1], port: 2224 }] })
      await second
      older.resolve({ computers: [{ ...rows[0], port: 2223 }, rows[1]] })
      await first
      expect(row(rows[0].computer)?.port).toBe(2223)
      expect(row(rows[1].computer)?.port).toBe(2224)
    } finally { store.dispose() }
  })
})

describe("checkpoint response ordering", () => {
  it.each(["capture", "fork"] as const)("preserves a newer sibling lifecycle result after a late %s response", async kind => {
    const initial = structuredClone(source)
    initial.computers = initial.computers.slice(0, 2).map(row => ({ ...row, state: "running" }))
    const [a, b] = initial.computers
    const pending = deferred<unknown>()
    let reads = 0
    const newer = { ...initial, computers: [{ ...a }, { ...b, state: "stopped" }] }
    const mock = bridge(command => {
      if (command === "read_application_state") return ++reads === 1 ? initial : new Promise(() => {})
      if (command === "computer_action") return newer
      if (command === "create_checkpoint" || command === "fork_checkpoint") return pending.promise
    })
    const store = createProductionSource(mock.native)
    const row = (id: string) => store.getSnapshot().source?.computers.find(item => item.configuration.id === id)
    try {
      await store.initialize()
      const checkpoint = { id: "captured", name: "Saved", createdAt: "2026-10-02T12:00:00Z", scope: "full" as const, reason: "manual" as const }
      const action = kind === "capture" ? store.applicationActions.createCheckpoint!(a.configuration.name, "Saved")
        : store.applicationActions.forkCheckpoint!(a.configuration.name, "point-1", "new-fork")
      store.applicationActions.stopComputer(b.configuration.name)
      await vi.waitFor(() => {
        expect(row(b.configuration.id)?.state).toBe("stopped")
        expect(row(b.configuration.id)?.lifecycleAction).toBeUndefined()
      })
      const older = structuredClone(initial)
      older.computers[0].checkpoints = [checkpoint]
      if (kind === "fork") older.computers.push({ ...a, configuration: { ...a.configuration, id: "fork-id", name: "new-fork" }, state: "stopped" })
      pending.resolve(older)
      await action
      expect(row(b.configuration.id)?.state).toBe("stopped")
      expect(row(a.configuration.id)?.checkpoints).toEqual([checkpoint])
      if (kind === "fork") expect(row("fork-id")).toMatchObject({ state: "stopped", configuration: { name: "new-fork" } })
    } finally { store.dispose() }
  })
})

describe("overlapping state reads", () => {
  const withState = (state: "running" | "stopped", detail: string) => ({ ...structuredClone(source), computers: source.computers.map((computer, index) => index === 0 ? { ...computer, state, stateDetail: detail } : computer) })

  it("shows an earlier read's result when a later read only reports UPDATING (H-37)", async () => {
    const earlier = deferred<unknown>()
    let reads = 0
    const mock = bridge(command => {
      if (command !== "read_application_state") return undefined
      reads++
      if (reads === 2) return earlier.promise
      if (reads === 3) throw { code: "update_in_progress", message: "Computer settings are changing." }
      return withState("running", "Running")
    })
    const store = createProductionSource(mock.native)
    const dev = () => store.getSnapshot().source?.computers[0]
    try {
      await store.initialize()
      const first = store.refresh()
      await vi.waitFor(() => expect(reads).toBe(2))
      await store.refresh()
      earlier.resolve(withState("stopped", "Stopped just now"))
      await first
      expect(dev()).toMatchObject({ state: "stopped", stateDetail: "Stopped just now" })
    } finally { store.dispose() }
  })

  it("lets an earlier fresh row finish after a later settling row without rolling back siblings (H-37)", async () => {
    const earlier = deferred<unknown>()
    let reads = 0
    const mock = bridge(command => {
      if (command !== "read_application_state") return undefined
      if (++reads === 2) return earlier.promise
      const result = withState("running", reads === 3 ? "Old runtime reading" : "Initial")
      if (reads === 3) {
        result.computers[0].settling = true
        result.computers[1].stateDetail = "Newest sibling"
      }
      return result
    })
    const store = createProductionSource(mock.native)
    try {
      await store.initialize()
      const first = store.refresh()
      await vi.waitFor(() => expect(reads).toBe(2))
      await store.refresh()
      expect(store.getSnapshot().source!.computers[0]).toMatchObject({ stateDetail: "Initial", settling: true })
      earlier.resolve(withState("stopped", "Fresh completed read"))
      await first
      expect(store.getSnapshot().source!.computers[0]).toMatchObject({ state: "stopped", stateDetail: "Fresh completed read" })
      expect(store.getSnapshot().source!.computers[1].stateDetail).toBe("Newest sibling")
    } finally { store.dispose() }
  })

  it("propagates local stale status, host capacity, and the published website host", async () => {
    const local = withState("running", "Last known status")
    local.computers[0].freshness = "stale"
    const capacity = { logicalCpus: 8, physicalMemoryBytes: 16 * 1024 ** 3, maxMemoryGib: 16 }
    const mock = bridge(command => {
      if (command === "read_application_state") return { ...local, deviceCapacity: capacity }
      if (command === "read_network_state") return { computers: [{ computer: local.computers[0].configuration.name, host: "dev.localhost", error: null, ports: [{ port: 3000, hostPort: 43000, scheme: "http", state: "reachable", configured: true }] }] }
    })
    const store = createProductionSource(mock.native)
    try {
      await store.initialize()
      await store.applicationActions.refreshNetwork!()
      expect(store.getSnapshot().source!.deviceCapacity).toEqual(capacity)
      expect(store.getSnapshot().source!.computers[0]).toMatchObject({ freshness: "stale", ports: [{ host: "dev.localhost", hostPort: 43000 }] })
    } finally { store.dispose() }
  })

  it("never lets an older read replace a newer read's result (H-37)", async () => {
    const earlier = deferred<unknown>()
    let reads = 0
    const mock = bridge(command => {
      if (command !== "read_application_state") return undefined
      reads++
      if (reads === 2) return earlier.promise
      return withState(reads === 3 ? "stopped" : "running", reads === 3 ? "Newest" : "Initial")
    })
    const store = createProductionSource(mock.native)
    try {
      await store.initialize()
      const first = store.refresh()
      await vi.waitFor(() => expect(reads).toBe(2))
      await store.refresh()
      expect(store.getSnapshot().source?.computers[0].stateDetail).toBe("Newest")
      earlier.resolve(withState("running", "Older"))
      await first
      expect(store.getSnapshot().source?.computers[0].stateDetail).toBe("Newest")
    } finally { store.dispose() }
  })

  it("drops a read that started before an action published its result (H-37)", async () => {
    const earlier = deferred<unknown>()
    let reads = 0
    const mock = bridge(command => {
      if (command === "read_application_state") { reads++; return reads === 2 ? earlier.promise : reads === 3 ? new Promise(() => {}) : withState("running", "Running") }
      if (command === "computer_action") return withState("stopped", "Stopped by action")
    })
    const store = createProductionSource(mock.native)
    try {
      await store.initialize()
      const before = store.refresh()
      await vi.waitFor(() => expect(reads).toBe(2))
      store.applicationActions.stopComputer("dev")
      await vi.waitFor(() => expect(store.getSnapshot().source?.computers[0].stateDetail).toBe("Stopped by action"))
      earlier.resolve(withState("running", "Before the action"))
      await before
      expect(store.getSnapshot().source?.computers[0].stateDetail).toBe("Stopped by action")
    } finally { store.dispose() }
  })
})

describe("export and import state", () => {
  it("keeps a running export through a transient read failure instead of inventing a failure (H-35)", async () => {
    const archive = { name: "dev.silo-backup", archivePath: "/tmp/dev.silo-backup", completedLabel: "Not completed", size: "Unknown", destination: "/tmp", computers: ["dev"] }
    const running: BackupState = { ...backup, operation: { operation: "backup", archive, runningNames: ["dev"], kind: "running", progress: 40, phases: [{ title: "Exporting dev", detail: "", tone: "running" }] } }
    let failing = false
    const mock = bridge(command => {
      if (command === "read_backup_state") { if (failing) throw new Error("bridge timed out"); return structuredClone(running) }
    })
    const store = createProductionSource(mock.native)
    try {
      await store.initialize()
      expect(store.getSnapshot().backup.operation).toMatchObject({ kind: "running", progress: 40 })
      failing = true
      await store.refresh()
      expect(store.getSnapshot().backup.operation).toMatchObject({ kind: "running", progress: 40 })
      expect(store.getSnapshot().backup.availabilityMessage).toContain("bridge timed out")
      failing = false
      await store.refresh()
      expect(store.getSnapshot().backup).toMatchObject({ availability: "available", operation: { kind: "running" } })
    } finally { store.dispose() }
  })
})

describe("result and job identity", () => {
  const archive = { name: "dev.silo-backup", archivePath: "/tmp/dev.silo-backup", completedLabel: "Today", size: "1 GiB", destination: "/tmp", computers: ["dev"] }
  const result = { operation: "backup" as const, archive, runningNames: [], kind: "result" as const, outcome: "success" as const, title: "Export complete", message: "Exported dev." }

  it("shows a result again when the runtime refuses to dismiss it (H-33, E-49)", async () => {
    const mock = bridge(command => {
      if (command === "read_backup_state") return { ...backup, operationId: "op-1", operation: result }
      if (command === "dismiss_backup_operation") return false
    })
    const store = createProductionSource(mock.native)
    try {
      await store.initialize()
      store.backupActions.dismissOperation()
      expect(store.getSnapshot().backup.operation).toBeNull()
      await vi.waitFor(() => expect(store.getSnapshot().backup.operation).toMatchObject({ kind: "result", title: "Export complete" }))
      expect(mock.invoke).toHaveBeenCalledWith("dismiss_backup_operation", { expectedOperation: result, expectedOperationId: "op-1" })
    } finally { store.dispose() }
  })

  it("keeps the runtime's unseen marker on the result it belongs to and drops it once the result is dismissed here", async () => {
    const interrupted = { ...result, operation: "restore" as const, outcome: "failed" as const, title: "Import interrupted before the upgrade", message: "Silo closed before this import finished." }
    const mock = bridge(command => {
      if (command === "read_backup_state") return { ...backup, operationId: "op-1", operation: interrupted, resultUnseen: true }
      if (command === "dismiss_backup_operation") return true
    })
    const store = createProductionSource(mock.native)
    try {
      await store.initialize()
      expect(store.getSnapshot().backup).toMatchObject({ operationId: "op-1", resultUnseen: true, operation: { kind: "result", title: "Import interrupted before the upgrade" } })
      store.backupActions.dismissOperation()
      // The runtime is told what was shown, and the dismissed result is no longer reported unseen.
      expect(mock.invoke).toHaveBeenCalledWith("dismiss_backup_operation", { expectedOperation: interrupted, expectedOperationId: "op-1" })
      expect(store.getSnapshot().backup.operation).toBeNull()
      expect(store.getSnapshot().backup.resultUnseen).toBeUndefined()
    } finally { store.dispose() }
  })

  it("does not report an ordinary result as unseen", async () => {
    const mock = bridge(command => { if (command === "read_backup_state") return { ...backup, operationId: "op-1", operation: result } })
    const store = createProductionSource(mock.native)
    try {
      await store.initialize()
      expect(store.getSnapshot().backup.operation).toMatchObject({ kind: "result" })
      expect(store.getSnapshot().backup.resultUnseen).toBeUndefined()
    } finally { store.dispose() }
  })

  it("identifies a dismissed result by its operation id, not its serialized payload (H-33)", async () => {
    let raced = false
    const mock = bridge(command => {
      // A read that raced the dismissal returns the same operation's result, re-rendered.
      if (command === "read_backup_state") return { ...backup, operationId: "op-1", operation: raced ? { ...result, detail: "Archive verified." } : result }
      if (command === "dismiss_backup_operation") { raced = true; return true }
    })
    const store = createProductionSource(mock.native)
    try {
      await store.initialize()
      store.backupActions.dismissOperation()
      await vi.waitFor(() => expect(raced).toBe(true))
      await store.refresh()
      expect(store.getSnapshot().backup.operation).toBeNull()
    } finally { store.dispose() }
  })

  it("dismisses the previous result in the runtime when a new export starts (E-49)", async () => {
    const mock = bridge(command => {
      if (command === "read_backup_state") return { ...backup, operationId: "op-1", operation: result }
      if (command === "dismiss_backup_operation") return true
      if (command === "start_backup") return new Promise(() => {})
    })
    const store = createProductionSource(mock.native)
    try {
      await store.initialize()
      store.backupActions.startBackup("/tmp", ["dev"])
      expect(store.getSnapshot().backup.operation).toMatchObject({ kind: "running" })
      expect(mock.invoke).toHaveBeenCalledWith("dismiss_backup_operation", { expectedOperation: result, expectedOperationId: "op-1" })
    } finally { store.dispose() }
  })

  it("joins an identical in-flight configuration request whatever its property order (H-33)", async () => {
    const pending = deferred<unknown>()
    const mock = bridge(command => command === "retry_computer_configuration" ? pending.promise : undefined)
    const store = createProductionSource(mock.native)
    try {
      await store.initialize()
      const configurations = store.getSnapshot().source!.computers.map(({ configuration }) => configuration)
      const first = store.configureConfigurations({ schemaVersion: 1, computers: configurations }, { kind: "retry", computer: "dev" })
      const reordered = configurations.map(configuration => Object.fromEntries(Object.entries(configuration).reverse()) as typeof configuration)
      const second = store.configureConfigurations({ computers: reordered, schemaVersion: 1 }, { computer: "dev", kind: "retry" })
      expect(second).toBe(first)
      pending.resolve(structuredClone(source))
      await first
      expect(count(mock.invoke, "retry_computer_configuration")).toBe(1)
    } finally { store.dispose() }
  })
})

describe("cancelled lifecycle actions", () => {
  const lifecycle = (id: string, title: string, occurredAt: string, extra: Record<string, unknown> = {}) => ({ id, category: "computer" as const, title, detail: "", occurredAt, time: occurredAt, tone: "neutral" as const, status: "completed" as const, computer: "dev", ...extra })

  it("shows a cancellation the runtime recorded as neutral after a reload (H-34)", async () => {
    let activities = [lifecycle("lifecycle-1-2", "Start cancelled", "2026-09-30T10:00:00.000Z", { cancelled: true, detail: "The action was cancelled." })]
    const mock = bridge(command => command === "read_application_state" ? { ...structuredClone(source), activities } : undefined)
    const store = createProductionSource(mock.native)
    const dev = () => store.getSnapshot().source?.computers.find(computer => computer.configuration.name === "dev")
    try {
      await store.initialize()
      expect(dev()).toMatchObject({ lifecycleFailureCancelled: true, lifecycleFailureAction: "start" })
      // A later start supersedes it.
      activities = [lifecycle("lifecycle-1-3", "Computer started", "2026-09-30T10:05:00.000Z", { tone: "success" }), ...activities]
      await store.refresh()
      expect(dev()?.lifecycleFailure).toBeUndefined()
      expect(dev()?.lifecycleFailureCancelled).toBeUndefined()
    } finally { store.dispose() }
  })

  it("keeps an older device's unclassified cancellation text as a failure (H-34)", async () => {
    const mock = bridge(command => {
      if (command === "device_list") return [office]
      if (command === "device_snapshot") return remoteSource({ state: "stopped", lifecycleFailure: "Stop failed: stop dev was cancelled." })
    })
    const store = createProductionSource(mock.native)
    try {
      await store.initialize()
      const remote = store.getSnapshot().source?.computers.find(computer => computer.configuration.id === remoteTarget("office"))
      expect(remote?.lifecycleFailure).toBe("Stop failed: stop dev was cancelled.")
      expect(remote?.lifecycleFailureCancelled).toBeUndefined()
    } finally { store.dispose() }
  })

  it("keeps a real reported failure red (H-34)", async () => {
    const failed = structuredClone(source)
    failed.computers[0] = { ...failed.computers[0], lifecycleFailure: "Start failed: not enough memory" }
    const mock = bridge(command => command === "read_application_state" ? structuredClone(failed) : undefined)
    const store = createProductionSource(mock.native)
    try {
      await store.initialize()
      const dev = store.getSnapshot().source?.computers[0]
      expect(dev?.lifecycleFailure).toBe("Start failed: not enough memory")
      expect(dev?.lifecycleFailureCancelled).toBeUndefined()
    } finally { store.dispose() }
  })
})

describe("native state validation", () => {
  it("keeps a newer device available when it reports values this version does not know (H-17)", async () => {
    const newer = remoteSource()
    const known = newer.computers[0]
    const payload = {
      ...structuredClone(newer),
      github: { ...newer.github, state: "suspended" },
      computers: [
        { ...structuredClone(known), state: "hibernating", stateDetail: "Hibernating since 10:00" },
      ],
      activities: [{ id: "broken" }, { id: "a-1", category: "insights", title: "Checked", detail: "", occurredAt: "2026-09-30T10:00:00.000Z", time: "", tone: "info", status: "done", computer: null }],
      repositoryPushOperations: [{ computer: known.configuration.name, repositoryPath: "acme/silo", commitCount: 1, status: "queued" }],
    }
    const mock = bridge(command => {
      if (command === "device_list") return [office]
      if (command === "device_snapshot") return payload
    })
    const store = createProductionSource(mock.native)
    try {
      await store.initialize()
      const view = store.getSnapshot().source!
      expect(view.devices?.[0]).toMatchObject({ connected: true })
      expect(view.devices?.[0].error).toBeUndefined()
      const rows = view.computers.filter(computer => computer.device)
      expect(rows).toHaveLength(1)
      expect(rows[0]).toMatchObject({ state: "stopped", freshness: "stale", stateDetail: "Hibernating since 10:00", attention: { level: "warning" } })
      const activity = view.activities.find(item => item.id.endsWith(encodeURIComponent("a-1")))
      expect(activity).toMatchObject({ category: "system", tone: "neutral", status: "completed", computer: undefined })
      expect(view.activities.some(item => item.id.endsWith("broken"))).toBe(false)
      expect(view.repositoryPushOperations.some(push => push.computer === remoteTarget("office"))).toBe(false)
    } finally { store.dispose() }
  })

  it("drops unreadable enrichment entries without rejecting local state (H-17)", () => {
    const local = structuredClone(source) as unknown as Record<string, unknown>
    const computers = local.computers as Array<Record<string, unknown>>
    computers[0].repositories = [{ path: "acme/silo", branch: "main" }, { path: "acme/ok", branch: "main", ahead: 0, behind: 0, dirty: false }]
    local.activities = [{ id: "no-title" }, ...(local.activities as unknown[])]
    const parsed = parseApplicationSource(local)
    expect(parsed.computers[0].repositories).toEqual([{ path: "acme/ok", branch: "main", ahead: 0, behind: 0, dirty: false }])
    expect(parsed.activities).toHaveLength(source.activities.length)
  })

  it("accepts native state without legacy preference and backup placeholders (D-20)", () => {
    const local = structuredClone(source) as unknown as Record<string, unknown>
    delete local.preferences
    delete local.backup
    const parsed = parseApplicationSource(local)
    expect(parsed.preferences.terminal).toBe("Terminal")
    expect(parsed.preferences.startupComputerIds).toBeUndefined()
    expect(parsed.backup).toEqual({ lastArchive: "", completedLabel: "", compressedSize: "", destination: "" })
  })

  it("retains separate setup and lifecycle diagnostics (D-39)", () => {
    const local = structuredClone(source)
    const row = { ...local.computers[0], lifecycleFailure: "Start failed. Retry.", lifecycleFailureDiagnostic: "Exit code 13" }
    const activity = { ...local.activities[0], diagnostic: "Exit code 13", partial: true }
    const parsed = parseApplicationSource({ ...local, computers: [row], activities: [activity] })
    expect(parsed.computers[0]).toMatchObject({ lifecycleFailureDiagnostic: "Exit code 13" })
    expect(parsed.activities[0]).toMatchObject({ diagnostic: "Exit code 13", partial: true })
  })

  it("rejects local state whose shown fields are malformed (H-17)", () => {
    expect(() => parseApplicationSource({ ...structuredClone(source), runtimeRepair: { status: "needed" } })).toThrow()
    expect(() => parseApplicationSource({ ...structuredClone(source), computers: [{ ...structuredClone(source.computers[0]), configuration: { id: "x", name: "dev" } }] })).toThrow()
  })
})

describe("React binding", () => {
  it("does not rerender subscribers during ten unchanged polling ticks", async () => {
    vi.useFakeTimers()
    const mock = bridge()
    const store = createProductionSource(mock.native)
    try {
      await store.initialize()
      await vi.advanceTimersByTimeAsync(0)
      let renders = 0
      const { result } = renderHook(() => { renders++; return useProductionSource(store) })
      const first = result.current
      const notified = vi.fn()
      const unsubscribe = store.subscribe(notified)
      const reads = count(mock.invoke, "read_application_state")
      for (let tick = 0; tick < 10; tick++) {
        await act(() => vi.advanceTimersByTimeAsync(10_000))
      }
      unsubscribe()
      expect(count(mock.invoke, "read_application_state") - reads).toBe(10)
      expect({ renders, notifications: notified.mock.calls.length }).toEqual({ renders: 1, notifications: 0 })
      expect(result.current).toBe(first)
    } finally { store.dispose(); vi.useRealTimers() }
  })

  it("returns the same value across renders until the snapshot changes (H-19)", async () => {
    let changed = false
    const mock = bridge(command => command === "read_application_state" && changed
      ? { ...structuredClone(source), github: { state: "disconnected" } }
      : undefined)
    const store = createProductionSource(mock.native)
    try {
      await store.initialize()
      const { result, rerender } = renderHook(() => useProductionSource(store))
      const first = result.current
      rerender()
      expect(result.current).toBe(first)
      changed = true
      await act(() => store.refresh())
      expect(result.current).not.toBe(first)
      expect(result.current.backup.actions).toBe(store.backupActions)
    } finally { store.dispose() }
  })
})

describe("GitHub state from full reads", () => {
  it("shows the unavailable state a failed GitHub read reports without a policy revision (H-20)", async () => {
    let failed = false
    const connected = { ...source, github: { ...source.github, state: "connected" as const, account: "octo", policyRevision: 4, repositoryCatalog: ["acme/silo"], repositoryCatalogStatus: { status: "available" as const } } }
    const fallback = { ...source, github: { state: "disconnected", accessEnabled: false, repositoryCatalog: [], repositoryCatalogStatus: { status: "unavailable", message: "GitHub settings could not be read.", canRetry: true }, computerOperations: [] } }
    const mock = bridge(command => command === "read_application_state" ? structuredClone(failed ? fallback : connected) : undefined)
    const store = createProductionSource(mock.native)
    try {
      await store.initialize()
      expect(store.getSnapshot().source?.github).toMatchObject({ state: "connected", policyRevision: 4 })
      failed = true
      await store.refresh()
      expect(store.getSnapshot().source?.github.repositoryCatalogStatus).toEqual({ status: "unavailable", message: "GitHub settings could not be read.", canRetry: true })
      failed = false
      await store.refresh()
      expect(store.getSnapshot().source?.github).toMatchObject({ state: "connected", policyRevision: 4, repositoryCatalogStatus: { status: "available" } })
    } finally { store.dispose() }
  })

  it("still ignores an older policy revision than the one shown (H-20)", async () => {
    let revision = 4
    const mock = bridge(command => command === "read_application_state" ? { ...structuredClone(source), github: { ...source.github, policyRevision: revision, accessEnabled: revision === 4 } } : undefined)
    const store = createProductionSource(mock.native)
    try {
      await store.initialize()
      revision = 3
      await store.refresh()
      expect(store.getSnapshot().source?.github).toMatchObject({ policyRevision: 4, accessEnabled: true })
    } finally { store.dispose() }
  })
})

describe("status actions", () => {
  it("reports a failed Quit request instead of dropping it (H-25)", async () => {
    toasts.showOperationFailure.mockClear()
    const mock = bridge(command => { if (command === "quit_app") throw new Error("settings could not be saved") })
    const store = createProductionSource(mock.native)
    try {
      await store.initialize()
      store.statusActions.quit()
      await vi.waitFor(() => expect(toasts.showOperationFailure).toHaveBeenCalledWith("quit", "Could not quit Silo", { description: "settings could not be saved" }))
    } finally { store.dispose() }
  })
})
