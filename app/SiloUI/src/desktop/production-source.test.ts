import { readFileSync } from "node:fs"
import { dirname, resolve } from "node:path"
import { fileURLToPath } from "node:url"
import { afterEach, describe, expect, it, vi } from "vitest"

import { applicationSourceForScenario } from "@/fixtures/application-scenarios"
import type { BackupState } from "@/features/application/model/backup-source"
import { createProductionSource, isUpdateInProgress, parseApplicationSource, parseBackupState, type ProductionBridge } from "./production-source"
import { siloProgressEventSchema } from "@/contracts/silo"

import { assertNativeBridgeMocksHandled, nativeBridgeMock, type NativeCommandHandlers } from "@/test/native-bridge-mock"

afterEach(assertNativeBridgeMocksHandled)

const pushTarget = { repository: "owner/repo", branch: "main", commit: "a".repeat(40) }

const toasts = vi.hoisted(() => ({ showOperationFailure: vi.fn() }))
vi.mock("@/lib/operation-toast", async (importOriginal) => ({ ...await importOriginal<typeof import("@/lib/operation-toast")>(), showOperationFailure: toasts.showOperationFailure }))

const source = applicationSourceForScenario("running")
const backup: BackupState = {
  snapshotId: "one",
  availability: "available",
  requiredSpaceGB: 2,
  availableSpaceGB: 40,
  archives: [{ name: "dev.silo-backup", archivePath: "/tmp/dev.silo-backup", completedLabel: "Today", size: "1 GiB", destination: "/tmp", computers: ["dev"] }],
  operation: null,
}

function initializationHandlers() {
  return {
    read_setup_activity: () => [],
    read_network_state: () => ({ computers: [] }),
    remote_network_state: () => ({ computers: [] }),
    device_list: () => [],
    connections_status: () => ({ enabled: false, deviceId: "local", name: "Laptop", address: "developer@laptop" }),
    read_operation_queue: () => ({ running: [], waiting: [] }),
  }
}

function native(overrides: Partial<ProductionBridge> = {}, handlers: NativeCommandHandlers = {}) {
  let event: (() => void) | null = null
  const invoke = nativeBridgeMock({
    ...initializationHandlers(),
    read_application_state: () => structuredClone(source),
    read_backup_state: () => structuredClone(backup),
    computer_action: () => structuredClone(source),
    ...handlers,
  })
  const listen = vi.fn(async (_name: string, handler: () => void) => { event = handler; return () => { event = null } })
  return { bridge: { invoke, listen, ...overrides } as ProductionBridge, invoke, emit: () => event?.() }
}

describe("production application bridge", () => {
  it("validates pending secret revocation names without exposing them as configured secrets", () => {
    const response = structuredClone(source)
    response.computers[0].pendingSecretRevocations = ["REMOVED_TOKEN"]
    expect(parseApplicationSource(response).computers[0].pendingSecretRevocations).toEqual(["REMOVED_TOKEN"])
    expect(parseApplicationSource(response).secrets.some(secret => secret.name === "REMOVED_TOKEN")).toBe(false)
    expect(() => parseApplicationSource({ ...response, computers: [{ ...response.computers[0], pendingSecretRevocations: [123] }] })).toThrow()
  })

  it("publishes remote pending revocation and clears it after Restart on the owning device", async () => {
    const remote = structuredClone(source)
    remote.computers = [{ ...remote.computers[0], pendingSecretRevocations: ["REMOVED_TOKEN"], attention: { level: "warning", message: "May still have access to REMOVED_TOKEN until it restarts." } }]
    const computerId = remote.computers[0].configuration.id
    const target = `silo-remote:office:${computerId}`
    let restarted = false
    const settled = () => ({ ...remote, computers: remote.computers.map(computer => ({ ...computer, pendingSecretRevocations: undefined, attention: undefined })) })
    const mock = native({}, {
      device_list: () => [{ id: "office", name: "Office Mac", address: "user@office" }],
      device_snapshot: () => restarted ? settled() : remote,
      remote_computer_action: () => { restarted = true; return settled() },
    })
    const store = createProductionSource(mock.bridge)
    const row = () => store.getSnapshot().source?.computers.find(computer => computer.configuration.id === target)
    try {
      await store.initialize()
      await vi.waitFor(() => expect(row()?.pendingSecretRevocations).toEqual(["REMOVED_TOKEN"]))
      expect(row()?.attention?.message).toContain("REMOVED_TOKEN")
      store.applicationActions.restartComputer(target)
      await vi.waitFor(() => expect(mock.invoke).toHaveBeenCalledWith("remote_computer_action", { deviceId: "office", computerId, action: "restart", name: "dev" }))
      await vi.waitFor(() => expect(row()?.pendingSecretRevocations).toBeUndefined())
    } finally { store.dispose() }
  })

  it.each([
    { code: "update_in_progress", message: "Please wait for configuration." },
    new Error("Connection timed out"),
  ])("retains pending revocation in a stale remote snapshot until its owner confirms it cleared (%s)", async cause => {
    const remote = structuredClone(source)
    const message = "May still have access to REMOVED_TOKEN until it restarts."
    remote.computers = [{ ...remote.computers[0], pendingSecretRevocations: ["REMOVED_TOKEN"], attention: { level: "warning", message } }]
    const target = `silo-remote:office:${remote.computers[0].configuration.id}`
    let unavailable = false
    let revoked = false
    const mock = native({}, {
      device_list: () => [{ id: "office", name: "Office Mac", address: "user@office" }],
      device_snapshot: () => {
        if (unavailable) throw cause
        return revoked ? { ...remote, computers: remote.computers.map(computer => ({ ...computer, pendingSecretRevocations: undefined, attention: undefined })) } : remote
      },
    })
    const store = createProductionSource(mock.bridge)
    const row = () => store.getSnapshot().source?.computers.find(computer => computer.configuration.id === target)
    try {
      await store.initialize()
      expect(row()?.pendingSecretRevocations).toEqual(["REMOVED_TOKEN"])
      unavailable = true
      await store.refresh()
      await vi.waitFor(() => expect(row()).toMatchObject({ freshness: "stale", pendingSecretRevocations: ["REMOVED_TOKEN"], attention: { level: "warning", message } }))
      unavailable = false
      revoked = true
      await store.refresh()
      await vi.waitFor(() => expect(row()?.freshness).toBe("fresh"))
      expect(row()?.pendingSecretRevocations).toBeUndefined()
      expect(row()?.attention).toBeUndefined()
    } finally { store.dispose() }
  })

  it("normalizes native checkpoint epoch milliseconds at the application boundary", () => {
    const createdAt = Date.UTC(2026, 8, 25, 12, 34, 56)
    const response = structuredClone(source) as unknown as Record<string, unknown>
    const computers = response.computers as Array<Record<string, unknown>>
    computers[0].checkpoints = [{
      id: "point-1", name: "Native timestamp", createdAt, scope: "full", reason: "manual",
    }]

    const parsed = parseApplicationSource(response)
    expect(parsed.computers[0].checkpoints?.[0].createdAt).toBe(new Date(createdAt).toISOString())
  })

  it("sends checkpoint commands with the computer ID and publishes the returned checkpoint history", async () => {
    const mock = native()
    const updated = structuredClone(source)
    updated.computers[0].checkpoints = [{ id: "point-1", name: "Before refactor", createdAt: "2026-09-25T10:00:00Z", scope: "full", reason: "manual" }]
    let changed = false
    const invoke = vi.fn(async (command: string, args?: Record<string, unknown>) => {
      if (["create_checkpoint", "fork_checkpoint", "restore_checkpoint"].includes(command)) { changed = true; return updated }
      if (command === "read_application_state" && changed) return updated
      return mock.invoke(command, args)
    })
    const store = createProductionSource({ ...mock.bridge, invoke } as ProductionBridge)
    try {
      await store.initialize()
      await store.applicationActions.createCheckpoint!("dev", "Before refactor")
      expect(invoke).toHaveBeenCalledWith("create_checkpoint", { computerId: source.computers[0].configuration.id, name: "Before refactor" })
      expect(store.getSnapshot().source?.computers[0].checkpoints?.[0].id).toBe("point-1")
      await store.applicationActions.forkCheckpoint!("dev", "point-1", "experiment")
      expect(invoke).toHaveBeenCalledWith("fork_checkpoint", { computerId: source.computers[0].configuration.id, checkpointId: "point-1", newName: "experiment" })
      await store.applicationActions.restoreCheckpoint!("dev", "point-1")
      expect(invoke).toHaveBeenCalledWith("restore_checkpoint", { computerId: source.computers[0].configuration.id, checkpointId: "point-1" })
    } finally { store.dispose() }
  })

  it("keeps a local checkpoint pending across refreshes, blocks duplicates, and clears failed operation fields before retry", async () => {
    const mock = native()
    let complete: ((value: unknown) => void) | undefined
    let failNext = true
    const failedSource = structuredClone(source)
    failedSource.computers[0].checkpointOperation = { kind: "capture", status: "failed", stage: "Checkpoint failed", error: "Another computer operation is still running." }
    const runningSource = structuredClone(source)
    runningSource.computers[0].checkpointOperation = { kind: "capture", status: "running", stage: "Capturing computer state" }
    const invoke = vi.fn(async (command: string, args?: Record<string, unknown>) => {
      if (command === "create_checkpoint") {
        if (failNext) { failNext = false; throw new Error("Another computer operation is still running.") }
        return new Promise(resolve => { complete = resolve })
      }
      if (command === "read_application_state" && !failNext) return complete ? runningSource : failedSource
      return mock.invoke(command, args)
    })
    const store = createProductionSource({ ...mock.bridge, invoke } as ProductionBridge)
    try {
      await store.initialize()
      await expect(store.applicationActions.createCheckpoint!("dev", "Before refactor")).rejects.toThrow("Another computer operation is still running.")
      await vi.waitFor(() => expect(store.getSnapshot().source?.computers[0].checkpointOperation).toMatchObject({ status: "failed" }))
      const retry = store.applicationActions.createCheckpoint!("dev", "Before refactor")
      expect(store.getSnapshot().source?.computers[0].checkpointOperation).toMatchObject({ kind: "capture", status: "running", stage: "Creating checkpoint…" })
      await store.refresh()
      expect(store.getSnapshot().source?.computers[0].checkpointOperation).toMatchObject({ kind: "capture", status: "running", stage: "Capturing computer state" })
      await expect(store.applicationActions.createCheckpoint!("dev", "Duplicate")).rejects.toThrow("A checkpoint operation is already running")
      expect(invoke.mock.calls.filter(([command]) => command === "create_checkpoint")).toHaveLength(2)
      const updated = structuredClone(source)
      updated.computers[0].checkpoints = [{ id: "point-1", name: "Before refactor", createdAt: "2026-09-25T10:00:00Z", scope: "full", reason: "manual" }]
      complete!(updated)
      await retry
      expect(store.getSnapshot().source?.computers[0].checkpoints?.[0].id).toBe("point-1")
      expect(store.getSnapshot().source?.computers[0].checkpointOperation ?? null).toBeNull()
      await store.refresh()
      expect(store.getSnapshot().source?.computers[0].checkpointOperation).toMatchObject({ status: "running", stage: "Capturing computer state" })
      runningSource.computers[0].checkpointOperation = { kind: "capture", status: "failed", stage: "Verification failed", error: "Checkpoint could not be verified." }
      await store.refresh()
      expect(store.getSnapshot().source?.computers[0].checkpointOperation).toMatchObject({ status: "failed", error: "Checkpoint could not be verified." })
    } finally { store.dispose() }
  })

  it("keeps remote checkpoint operations visible and guards duplicate requests", async () => {
    const mock = native()
    let complete: ((value: unknown) => void) | undefined
    const invoke = vi.fn(async (command: string, args?: Record<string, unknown>) => {
      if (command === "device_list") return [{ id: "office", name: "Office Mac", address: "user@office" }]
      if (command === "device_snapshot") return structuredClone(source)
      if (command === "remote_checkpoint_action") return new Promise(resolve => { complete = resolve })
      return mock.invoke(command, args)
    })
    const store = createProductionSource({ ...mock.bridge, invoke } as ProductionBridge)
    const target = "silo-remote:office:" + source.computers[0].configuration.id
    try {
      await store.initialize()
      const action = store.applicationActions.createCheckpoint!(target, "Remote point")
      expect(store.getSnapshot().source?.computers.find(computer => computer.configuration.id === target)?.checkpointOperation).toMatchObject({ kind: "capture", status: "running" })
      await store.refresh()
      expect(store.getSnapshot().source?.computers.find(computer => computer.configuration.id === target)?.checkpointOperation).toMatchObject({ kind: "capture", status: "running" })
      await expect(store.applicationActions.createCheckpoint!(target, "Duplicate")).rejects.toThrow("A checkpoint operation is already running")
      expect(invoke.mock.calls.filter(([command]) => command === "remote_checkpoint_action")).toHaveLength(1)
      complete!(undefined)
      await action
      expect(store.getSnapshot().source?.computers.find(computer => computer.configuration.id === target)?.checkpointOperation).toBeUndefined()
    } finally { store.dispose() }
  })

  it("retains the last remote computer snapshot as stale while its owner refreshes and preserves lifecycle state", async () => {
    const mock = native()
    let snapshotReads = 0
    let finishAction!: () => void
    const invoke = vi.fn(async (command: string, args?: Record<string, unknown>) => {
      if (command === "device_list") return [{ id: "office", name: "Office Mac", address: "user@office" }]
      if (command === "device_snapshot") {
        snapshotReads++
        if (snapshotReads === 2) throw { code: "update_in_progress", message: "Please wait for configuration." }
        return structuredClone(source)
      }
      if (command === "remote_computer_action") return new Promise(resolve => { finishAction = () => resolve(structuredClone(source)) })
      return mock.invoke(command, args)
    })
    const store = createProductionSource({ ...mock.bridge, invoke } as ProductionBridge)
    const target = `silo-remote:office:${source.computers[0].configuration.id}`
    try {
      await store.initialize()
      expect(snapshotReads).toBe(1)
      expect(store.getSnapshot().source?.computers.find(computer => computer.configuration.id === target)).toMatchObject({
        state: source.computers[0].state,
        freshness: "fresh",
        device: { connected: true },
      })

      store.applicationActions.startComputer!(target)
      expect(store.getSnapshot().source?.computers.find(computer => computer.configuration.id === target)?.lifecycleAction).toBe("start")
      await store.refresh()
      await vi.waitFor(() => expect(store.getSnapshot().source?.computers.find(computer => computer.configuration.id === target)?.device?.busy).toBe(true))
      const refreshing = store.getSnapshot().source?.computers.find(computer => computer.configuration.id === target)
      expect(refreshing).toMatchObject({
        state: source.computers[0].state,
        stateDetail: "Updating…",
        freshness: "stale",
        device: { connected: true, busy: true },
        lifecycleAction: "start",
      })

      await store.refresh()
      await vi.waitFor(() => expect(store.getSnapshot().source?.computers.find(computer => computer.configuration.id === target)?.freshness).toBe("fresh"))
      expect(snapshotReads).toBe(3)
      const refreshed = store.getSnapshot().source?.computers.find(computer => computer.configuration.id === target)
      expect(refreshed).toMatchObject({
        state: source.computers[0].state,
        stateDetail: source.computers[0].stateDetail,
        freshness: "fresh",
        device: { connected: true },
        lifecycleAction: "start",
      })
      expect(refreshed?.device).not.toHaveProperty("busy")

      finishAction()
      await vi.waitFor(() => expect(store.getSnapshot().source?.computers.find(computer => computer.configuration.id === target)?.lifecycleAction).toBeUndefined())
    } finally { store.dispose() }
  })

  it("records a failed remote lifecycle action on the computer without marking its device offline", async () => {
    const mock = native()
    const invoke = vi.fn(async (command: string, args?: Record<string, unknown>) => {
      if (command === "device_list") return [{ id: "office", name: "Office Mac", address: "user@office" }]
      if (command === "device_snapshot") return structuredClone(source)
      if (command === "remote_computer_action") throw new Error("insufficient memory")
      return mock.invoke(command, args)
    })
    const store = createProductionSource({ ...mock.bridge, invoke } as ProductionBridge)
    const target = `silo-remote:office:${source.computers[0].configuration.id}`
    try {
      await store.initialize()
      store.applicationActions.startComputer!(target)
      await vi.waitFor(() => expect(store.getSnapshot().source?.computers.find(computer => computer.configuration.id === target)?.lifecycleFailure).toContain("insufficient memory"))
      const row = store.getSnapshot().source?.computers.find(computer => computer.configuration.id === target)
      expect(row).toMatchObject({ lifecycleFailureAction: "start", device: { connected: true } })
    } finally { store.dispose() }
  })

  it("routes checkpoint actions through the owning remote device", async () => {
    const mock = native({}, { remote_checkpoint_action: () => undefined })
    const store = createProductionSource(mock.bridge)
    await store.applicationActions.createCheckpoint!("silo-remote:11111111-1111-4111-8111-111111111111:22222222-2222-4222-8222-222222222222", "point")
    await store.applicationActions.forkCheckpoint!("silo-remote:11111111-1111-4111-8111-111111111111:22222222-2222-4222-8222-222222222222", "point-id", "branch")
    await store.applicationActions.restoreCheckpoint!("silo-remote:11111111-1111-4111-8111-111111111111:22222222-2222-4222-8222-222222222222", "point-id")
    expect(mock.invoke).toHaveBeenCalledWith("remote_checkpoint_action", { deviceId: "11111111-1111-4111-8111-111111111111", computerId: "22222222-2222-4222-8222-222222222222", action: "create", name: "point" })
    expect(mock.invoke).toHaveBeenCalledWith("remote_checkpoint_action", { deviceId: "11111111-1111-4111-8111-111111111111", computerId: "22222222-2222-4222-8222-222222222222", action: "fork", checkpointId: "point-id", newName: "branch" })
    expect(mock.invoke).toHaveBeenCalledWith("remote_checkpoint_action", { deviceId: "11111111-1111-4111-8111-111111111111", computerId: "22222222-2222-4222-8222-222222222222", action: "restore", checkpointId: "point-id" })
    expect(mock.invoke.mock.calls.some(([command]) => ["create_checkpoint", "fork_checkpoint", "restore_checkpoint"].includes(command as string))).toBe(false)
    store.dispose()
  })

  it("deletes a local checkpoint by computer ID, reads checkpoint usage, and refuses remote deletes", async () => {
    const mock = native()
    const usage = { totalBytes: 4096, checkpoints: [{ id: "point-1", sizeBytes: 4096, usedBy: ["experiment"], deleteBlocker: "Used by experiment." }] }
    const invoke = vi.fn(async (command: string, args?: Record<string, unknown>) => {
      if (command === "delete_checkpoint") return structuredClone(source)
      if (command === "read_checkpoint_usage") return usage
      return mock.invoke(command, args)
    })
    const store = createProductionSource({ ...mock.bridge, invoke } as ProductionBridge)
    try {
      await store.initialize()
      await store.applicationActions.deleteCheckpoint!("dev", "point-1")
      expect(invoke).toHaveBeenCalledWith("delete_checkpoint", { computerId: source.computers[0].configuration.id, checkpointId: "point-1" })
      expect(await store.applicationActions.readCheckpointUsage!(source.computers[0].configuration.id)).toEqual(usage)
      await expect(store.applicationActions.deleteCheckpoint!("silo-remote:11111111-1111-4111-8111-111111111111:22222222-2222-4222-8222-222222222222", "point-1")).rejects.toThrow("on its own device")
      expect(invoke.mock.calls.some(([command, args]) => command === "remote_checkpoint_action" && (args as Record<string, unknown>)?.action === "delete")).toBe(false)
    } finally { store.dispose() }
  })

  it("keeps native checkpoint deletion progress and drops malformed unfinished Restore data", () => {
    const response = structuredClone(source) as unknown as Record<string, unknown>
    const computers = response.computers as Array<Record<string, unknown>>
    const operation = { kind: "delete", status: "running", stage: "Deleting checkpoint…" }
    computers[0].checkpointOperation = operation
    computers[0].unfinishedRestore = { checkpointId: "point-1", phase: "unknown" }
    computers[0].settling = true
    const computer = parseApplicationSource(response).computers[0]
    expect(computer.checkpointOperation).toEqual(operation)
    expect(computer.unfinishedRestore).toBeNull()
    expect(computer.settling).toBe(true)
  })

  it("keeps an unfinished Restore in the computer view and abandons it by computer ID", async () => {
    const response = structuredClone(source) as unknown as Record<string, unknown>
    const computers = response.computers as Array<Record<string, unknown>>
    computers[0].unfinishedRestore = { checkpointId: "point-1", checkpointName: "Before refactor", phase: "capturing" }
    expect(parseApplicationSource(response).computers[0].unfinishedRestore).toEqual({ checkpointId: "point-1", checkpointName: "Before refactor", phase: "capturing" })

    const mock = native()
    const invoke = vi.fn(async (command: string, args?: Record<string, unknown>) => command === "abandon_restore" ? structuredClone(source) : mock.invoke(command, args))
    const store = createProductionSource({ ...mock.bridge, invoke } as ProductionBridge)
    try {
      await store.initialize()
      await store.applicationActions.abandonRestore!("dev")
      expect(invoke).toHaveBeenCalledWith("abandon_restore", { computerId: source.computers[0].configuration.id })
    } finally { store.dispose() }
  })

  it("recognizes update deferral by code and rejects legacy message text", () => {
    expect(isUpdateInProgress({ code: "update_in_progress", message: "Please wait." })).toBe(true)
    expect(isUpdateInProgress("remote request failed: SILO_SANDBOX_UPDATE_IN_PROGRESS")).toBe(false)
    expect(isUpdateInProgress(new Error("runtime unavailable"))).toBe(false)
  })

  it("re-reads the operation queue once when it changes during a read", async () => {
    const mock = native()
    const handlers = new Map<string, () => void>()
    let finishRead: ((value: unknown) => void) | undefined
    let queueReads = 0
    const invoke = vi.fn(async (command: string, args?: Record<string, unknown>) => {
      if (command === "read_operation_queue") {
        queueReads++
        if (queueReads === 2) return new Promise(resolve => { finishRead = resolve })
        return { running: [], waiting: [] }
      }
      return mock.invoke(command, args)
    })
    const listen = vi.fn(async (name: string, handler: () => void) => { handlers.set(name, handler); return () => { handlers.delete(name) } })
    const store = createProductionSource({ invoke, listen } as unknown as ProductionBridge)
    try {
      await store.initialize()
      const reads = queueReads
      handlers.get("silo://operation-queue-changed")?.()
      handlers.get("silo://operation-queue-changed")?.()
      handlers.get("silo://operation-queue-changed")?.()
      await vi.waitFor(() => expect(finishRead).toBeDefined())
      finishRead!({ running: [], waiting: [] })
      await vi.waitFor(() => expect(queueReads).toBe(reads + 2))
      await new Promise(resolve => setTimeout(resolve, 0))
      expect(queueReads).toBe(reads + 2)
    } finally { store.dispose() }
  })

  it("polls remote devices once per visible tick, pauses while hidden, and coalesces event bursts", async () => {
    vi.useFakeTimers()
    const mock = native()
    const handlers = new Map<string, () => void>()
    const invoke = vi.fn(async (command: string, args?: Record<string, unknown>) => command === "device_list" ? [] : mock.invoke(command, args))
    const listen = vi.fn(async (name: string, handler: () => void) => { handlers.set(name, handler); return () => { handlers.delete(name) } })
    const store = createProductionSource({ invoke, listen } as unknown as ProductionBridge)
    const count = (name: string) => invoke.mock.calls.filter(([command]) => command === name).length
    try {
      await store.initialize()
      await vi.advanceTimersByTimeAsync(0)
      let listedDevices = count("device_list")
      await vi.advanceTimersByTimeAsync(10_000)
      expect(count("device_list")).toBe(listedDevices + 1)
      listedDevices = count("device_list")
      const reads = count("read_application_state")
      const visibility = vi.spyOn(document, "visibilityState", "get").mockReturnValue("hidden")
      await vi.advanceTimersByTimeAsync(30_000)
      expect(count("device_list")).toBe(listedDevices)
      expect(count("read_application_state")).toBe(reads)
      visibility.mockRestore()
      document.dispatchEvent(new Event("visibilitychange"))
      await vi.advanceTimersByTimeAsync(0)
      expect(count("read_application_state")).toBe(reads + 1)
      expect(count("device_list")).toBe(listedDevices + 1)
      const beforeBurst = count("read_application_state")
      for (let index = 0; index < 5; index++) handlers.get("silo://application-state-changed")?.()
      await vi.advanceTimersByTimeAsync(0)
      expect(count("read_application_state")).toBe(beforeBurst + 2)
    } finally {
      store.dispose()
      vi.restoreAllMocks()
      vi.useRealTimers()
    }
  })

  it("refreshes repository rows while visible without overlapping slow reads and stops on disposal", async () => {
    vi.useFakeTimers()
    const mock = native()
    let complete: ((value: unknown) => void) | undefined
    let reads = 0
    const invoke = vi.fn(async (command: string, args?: Record<string, unknown>) => {
      if (command === "read_application_state") {
        reads++
        if (reads === 1) return structuredClone(source)
        return new Promise(resolve => { complete = resolve })
      }
      return mock.invoke(command, args)
    })
    const store = createProductionSource({ ...mock.bridge, invoke } as ProductionBridge)
    try {
      await store.initialize()
      await vi.advanceTimersByTimeAsync(10_000)
      expect(reads).toBe(2)
      await vi.advanceTimersByTimeAsync(30_000)
      expect(reads).toBe(2)
      const changed = structuredClone(source)
      changed.computers[0].repositories = [{ path: "new-repository", branch: "main", ahead: 0, behind: 0, dirty: false }]
      complete!(changed)
      await vi.advanceTimersByTimeAsync(0)
      expect(store.getSnapshot().source?.computers[0].repositories).toEqual(changed.computers[0].repositories)
      const visibility = vi.spyOn(document, "visibilityState", "get").mockReturnValue("hidden")
      await vi.advanceTimersByTimeAsync(10_000)
      expect(reads).toBe(2)
      visibility.mockRestore()
      await vi.advanceTimersByTimeAsync(10_000)
      expect(reads).toBe(3)
      complete!(changed)
      await vi.advanceTimersByTimeAsync(0)
      store.dispose()
      await vi.advanceTimersByTimeAsync(10_000)
      expect(reads).toBe(3)
    } finally {
      store.dispose()
      vi.restoreAllMocks()
      vi.useRealTimers()
    }
  })

  it("bypasses local and remote repository caches for an explicit refresh", async () => {
    const mock = native()
    const invoke = vi.fn(async (command: string, args?: Record<string, unknown>) => {
      if (command === "device_list") return [{ id: "office", name: "Office Mac", address: "user@office" }]
      if (command === "device_snapshot") return structuredClone(source)
      return mock.invoke(command, args)
    })
    const store = createProductionSource({ ...mock.bridge, invoke } as ProductionBridge)
    try {
      await store.initialize()
      await store.applicationActions.refreshRepositories!()
      expect(invoke).toHaveBeenCalledWith("read_application_state", { refreshRepositories: true })
      expect(invoke).toHaveBeenCalledWith("device_snapshot", { deviceId: "office", refreshRepositories: true })
    } finally { store.dispose() }
  })

  it.each(["dev", "silo-remote:00000000-0000-4000-8000-000000000010:00000000-0000-4000-8000-000000000011"])("opens the desktop for the exact selected target %s", async computer => {
    const mock = native({}, { open_desktop: () => undefined })
    const store = createProductionSource(mock.bridge)
    await store.applicationActions.openDesktop!(computer)
    expect(mock.invoke).toHaveBeenCalledWith("open_desktop", { computer })
    expect(mock.invoke.mock.calls.some(([command]) => command === "computer_action")).toBe(false)
    store.dispose()
  })

  it("reports desktop opening failure without changing the computer state", async () => {
    const mock = native()
    const invoke = vi.fn(async (command: string, args?: Record<string, unknown>) => {
      if (command === "open_desktop") throw new Error("Owning device unavailable")
      return mock.invoke(command, args)
    })
    const store = createProductionSource({ ...mock.bridge, invoke } as ProductionBridge)
    await store.initialize()
    const before = store.getSnapshot().source?.computers
    toasts.showOperationFailure.mockClear()
    await store.applicationActions.openDesktop!("dev")
    expect(toasts.showOperationFailure).toHaveBeenCalledWith("open-desktop:dev", "Could not open the Linux desktop", { description: expect.stringContaining("Owning device unavailable") })
    expect(store.getSnapshot().error).toBeNull()
    expect(store.getSnapshot().source?.computers).toEqual(before)
    store.dispose()
  })

  it("prepares connection commands and key exports on the selected owner", async () => {
    const mock = native()
    const invoke = vi.fn(async (command: string, args?: Record<string, unknown>) => command === "ssh_connection" ? (args?.download ? null : "ssh -i '/private/key' root@127.0.0.1") : mock.invoke(command, args))
    const store = createProductionSource({ ...mock.bridge, invoke } as ProductionBridge)
    expect(await store.applicationActions.sshConnection!("dev", false)).toContain("ssh -i")
    expect(invoke).toHaveBeenLastCalledWith("ssh_connection", { computer: "dev", download: false })
    const deviceId = "00000000-0000-4000-8000-000000000010"
    const computerId = "00000000-0000-4000-8000-000000000011"
    expect(await store.applicationActions.sshConnection!(`silo-remote:${deviceId}:${computerId}`, true)).toBeNull()
    expect(invoke).toHaveBeenLastCalledWith("ssh_connection", { deviceId, computerId, download: true })
    store.dispose()
  })

  it("refreshes SSH state, persists explicit exposure, and preserves rows on refresh failure", async () => {
    const mock = native()
    let failed = false
    const row = { computer: "dev", enabled: true, port: 2222, bindAddress: "127.0.0.1", keys: [], state: "waiting", message: null, fingerprint: null, deviceName: "Ada Mac", addresses: ["192.168.1.42"] }
    const invoke = vi.fn(async (command: string, args?: Record<string, unknown>) => {
      if (command === "read_ssh_access_state") { if (failed) throw new Error("private runtime details"); return { computers: [row] } }
      if (command === "save_ssh_access") return { computers: [{ ...row, ...args }] }
      return mock.invoke(command, args)
    })
    const store = createProductionSource({ ...mock.bridge, invoke } as ProductionBridge)
    await store.initialize()
    await store.applicationActions.refreshSshAccess!()
    expect(store.getSnapshot().source?.sshAccess?.computers[0]).toEqual(row)
    const request = { computer: "dev", enabled: true, port: 2223, bindAddress: "192.168.1.42", keys: [] }
    await store.applicationActions.saveSshAccess!(request)
    expect(invoke).toHaveBeenCalledWith("save_ssh_access", request)
    await store.refresh()
    expect(store.getSnapshot().source?.sshAccess?.computers[0].bindAddress).toBe("192.168.1.42")
    failed = true
    await store.applicationActions.refreshSshAccess!()
    expect(store.getSnapshot().source?.sshAccessError).toBe("Could not check SSH access.")
    expect(store.getSnapshot().source?.sshAccess?.computers[0].port).toBe(2223)
    store.dispose()
  })

  it("keeps personal-token connection independent from OAuth and never publishes its value", async () => {
    const mock = native()
    const base = await mock.invoke("read_application_state") as { github: Record<string, unknown> }
    const github = { ...base.github, state: "connected", personalToken: { state: "connected", saved: true, account: "token-user" } }
    const invoke = vi.fn(async (command: string, args?: Record<string, unknown>) => command === "save_github_personal_token" ? github : mock.invoke(command, args))
    const store = createProductionSource({ ...mock.bridge, invoke } as ProductionBridge)
    await store.initialize()
    await store.applicationActions.saveGitHubPersonalToken!("github_pat_synthetic")
    expect(invoke).toHaveBeenCalledWith("save_github_personal_token", { token: "github_pat_synthetic" })
    expect(store.getSnapshot().source?.github.state).toBe("connected")
    expect(store.getSnapshot().source?.github.personalToken?.account).toBe("token-user")
    expect(JSON.stringify(store.getSnapshot())).not.toContain("github_pat_synthetic")
    store.dispose()
  })

  it("passes status destinations and dismisses completed push results natively", async () => {
    const mock = native({}, { open_main: () => undefined, dismiss_repository_push: () => undefined })
    const store = createProductionSource(mock.bridge)
    await store.initialize()
    store.statusActions.openSilo({ computer: "dev", computerSection: "logs" })
    expect(mock.invoke).toHaveBeenCalledWith("open_main", { route: { computer: "dev", computerSection: "logs" } })
    store.statusActions.dismissRepositoryPush("dev", "/workspace/repo")
    await vi.waitFor(() => expect(mock.invoke).toHaveBeenCalledWith("dismiss_repository_push", { computer: "dev", repositoryPath: "/workspace/repo" }))
    store.dispose()
  })
  it("keeps network mappings across application refresh and shares only reachable sites", async () => {
    const mock = native({}, { open_network_port: () => undefined, remote_open_network_port: () => undefined })
    const state = { computers: [{ computer: "dev", error: null, host: "dev-1a2b3c4d.localhost", ports: [{port:3000,hostPort:43000,scheme:"http",state:"reachable",configured:true}] }] }
    const invoke = vi.fn(async (command: string, args?: Record<string, unknown>) => command === "read_network_state" || command === "save_network_port" ? state : mock.invoke(command,args))
    const store = createProductionSource({...mock.bridge,invoke} as ProductionBridge)
    await store.initialize()
    await store.applicationActions.refreshNetwork?.()
    expect(store.getSnapshot().source?.computers[0].ports).toEqual([{port:3000,hostPort:43000,scheme:"http",configured:true,listening:true,host:"dev-1a2b3c4d.localhost"}])
    await store.refresh()
    expect(store.getSnapshot().source?.network).toEqual(state)
    await store.applicationActions.saveNetworkPort?.({computer:"dev",port:3000,hostPort:null,scheme:"http"})
    expect(invoke).toHaveBeenCalledWith("save_network_port",{computer:"dev",port:3000,hostPort:null,scheme:"http"})
    store.statusActions.openSite("dev",3000)
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith("open_network_port",{computer:"dev",port:3000}))
    store.statusActions.openSite("silo-remote:office:vm-1",3000)
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith("remote_open_network_port",{deviceId:"office",computerId:"vm-1",port:3000}))
    store.dispose()
  })

  it("keeps cached rows after failed network checks but revokes reachable status", async () => {
    const mock = native()
    let failed = false
    const state = { computers: [{ computer:"dev",error:null,ports:[{port:3000,hostPort:43000,scheme:"http",state:"reachable",configured:true}] }] }
    const invoke = vi.fn(async (command: string, args?: Record<string, unknown>) => {
      if(command === "read_network_state") { if(failed) throw new Error("raw runtime output"); return state }
      return mock.invoke(command,args)
    })
    const store = createProductionSource({...mock.bridge,invoke} as ProductionBridge)
    await store.initialize(); await store.applicationActions.refreshNetwork?.()
    failed = true
    await store.applicationActions.refreshNetwork?.()
    expect(store.getSnapshot().source?.network?.computers.find(row => row.computer === "dev")).toEqual({ ...state.computers[0], error: "Could not check network services." })
    expect(store.getSnapshot().source?.networkError).toBe("Could not check network services.")
    expect(store.getSnapshot().source?.computers[0].ports[0].listening).toBe(false)
    store.dispose()
  })

  it("passes the selected folder path to the native editor action", async () => {
    const mock = native()
    const store = createProductionSource(mock.bridge)
    await store.applicationActions.openEditor("dev", "/workspace/projects/my folder")
    expect(mock.invoke).toHaveBeenCalledWith("computer_action", { action: "open-editor", name: "dev", path: "/workspace/projects/my folder" })
    store.dispose()
  })

  it("loads directory pages through the native bridge and validates their shape", async () => {
    const mock = native()
    const page = { snapshotId: "snapshot", entries: [{ name: "a b.txt", path: "/workspace/a b.txt", kind: "file" }], nextOffset: null }
    const invoke = vi.fn(async (command: string, args?: Record<string, unknown>) =>
      command === "list_computer_directory" ? page : mock.invoke(command, args))
    const store = createProductionSource({ ...mock.bridge, invoke } as ProductionBridge)
    expect(await store.applicationActions.listComputerDirectory?.("dev", "/workspace", 200, "snapshot")).toEqual(page)
    expect(await store.statusActions.listComputerDirectory?.("dev", "/workspace", 200, "snapshot")).toEqual(page)
    expect(invoke).toHaveBeenCalledWith("list_computer_directory", { computer: "dev", path: "/workspace", offset: 200, snapshotId: "snapshot" })
    page.entries[0].path = "/outside"
    await expect(store.applicationActions.listComputerDirectory?.("dev", "/workspace", 0)).rejects.toThrow()
    expect(invoke).not.toHaveBeenCalledWith("computer_action", expect.anything())
    store.dispose()
  })

  it("saves secret values only in the native request and publishes value-free metadata", async () => {
    const mock = native()
    const request = { operation: "add" as const, name: "API_TOKEN", value: "private-test-value", computers: ["dev"], allowedDomains: ["api.example.test"] }
    const saved = { id: "api-token", name: request.name, computers: request.computers, allowedDomains: request.allowedDomains, state: "restart-required" as const, pendingComputers: ["dev"], value: request.value }
    const invoke = vi.fn(async (name: string, args?: Record<string, unknown>) => {
      if (name === "save_secret" || name === "retry_secret") return [saved]
      if (name === "remove_secret") return []
      if (name === "read_application_state") return { ...source, secrets: [saved] }
      return mock.invoke(name, args)
    })
    const store = createProductionSource({ ...mock.bridge, invoke } as ProductionBridge)
    await store.initialize()
    await store.applicationActions.saveSecret(request)
    expect(invoke).toHaveBeenCalledWith("save_secret", { request })
    expect(store.getSnapshot().source?.secrets[0]).toEqual(expect.objectContaining({ pendingComputers: ["dev"] }))
    expect(JSON.stringify(store.getSnapshot())).not.toContain(request.value)
    await store.applicationActions.retrySecret?.("api-token")
    expect(invoke).toHaveBeenCalledWith("retry_secret", { id: "api-token" })
    await store.applicationActions.removeSecret("api-token")
    expect(invoke).toHaveBeenCalledWith("remove_secret", { id: "api-token" })
    store.dispose()
  })

  it("propagates rejected secret saves so the editor can preserve the draft", async () => {
    const mock = native()
    const invoke = vi.fn(async (name: string, args?: Record<string, unknown>) => {
      if (name === "save_secret") throw new Error("Credential storage is locked.")
      return mock.invoke(name, args)
    })
    const store = createProductionSource({ ...mock.bridge, invoke } as ProductionBridge)
    await store.initialize()
    await expect(store.applicationActions.saveSecret({ operation: "add", name: "API_TOKEN", value: "private-test-value", computers: ["dev"], allowedDomains: ["api.example.test"] })).rejects.toThrow("Credential storage is locked.")
    expect(store.getSnapshot().source?.secrets).toEqual(source.secrets)
    store.dispose()
  })

  it("dismisses a crash through the native owner and waits for its confirmed state", async () => {
    let finish!: (value: unknown) => void
    const command = new Promise((resolve) => { finish = resolve })
    const mock = native()
    const crashed = { ...source, computers: source.computers.map(w => ({ ...w, state: "failed", canDismissError: true })) }
    const invoke = vi.fn(async (name: string, args?: Record<string, unknown>) => name === "computer_action" ? command : name === "read_application_state" ? crashed : mock.invoke(name, args))
    const store = createProductionSource({ ...mock.bridge, invoke } as ProductionBridge)
    await store.initialize()
    store.applicationActions.dismissComputerError("dev")
    expect(invoke).toHaveBeenCalledWith("computer_action", { action: "dismiss-error", name: "dev" })
    expect(store.getSnapshot().source?.computers[0].state).toBe("failed")
    expect(store.getSnapshot().source?.computers[0].lifecycleAction).toBe("dismiss-error")
    store.applicationActions.startComputer("dev")
    expect(invoke.mock.calls.filter(([name]) => name === "computer_action")).toHaveLength(1)
    finish(crashed)
    await vi.waitFor(() => expect(store.getSnapshot().source?.computers[0].lifecycleAction).toBeUndefined())
    store.dispose()
  })

  it("records a cancelled lifecycle action as neutral and clears it when retried", async () => {
    let reject!: (error: unknown) => void
    const command = new Promise((_, r) => { reject = r })
    const mock = native()
    let attempts = 0
    const invoke = vi.fn(async (name: string, args?: Record<string, unknown>) => {
      if (name !== "computer_action") return mock.invoke(name, args)
      attempts += 1
      return attempts === 1 ? command : new Promise(() => {})
    })
    const store = createProductionSource({ ...mock.bridge, invoke } as ProductionBridge)
    await store.initialize()
    store.applicationActions.stopComputer("dev")
    expect(store.getSnapshot().source?.computers[0].lifecycleAction).toBe("stop")
    reject({ code: "cancelled", message: "Stopped at your request." })
    await vi.waitFor(() => expect(store.getSnapshot().source?.computers[0].lifecycleFailureCancelled).toBe(true))
    const cancelled = store.getSnapshot().source?.computers[0]
    expect(cancelled?.lifecycleFailureAction).toBe("stop")
    expect(cancelled?.lifecycleAction).toBeUndefined()
    // Retrying clears the cancelled banner the instant the new action is submitted.
    store.applicationActions.stopComputer("dev")
    const retried = store.getSnapshot().source?.computers[0]
    expect(retried?.lifecycleFailure).toBeUndefined()
    expect(retried?.lifecycleFailureCancelled).toBeUndefined()
    expect(retried?.lifecycleAction).toBe("stop")
    store.dispose()
  })

  it("shows restart immediately, keeps it through refresh, and blocks conflicting actions", async () => {
    let finish!: (value: unknown) => void
    const command = new Promise((resolve) => { finish = resolve })
    const mock = native()
    const invoke = vi.fn(async (name: string, args?: Record<string, unknown>) => name === "computer_action" ? command : mock.invoke(name, args))
    const store = createProductionSource({ ...mock.bridge, invoke } as ProductionBridge)
    await store.initialize()
    store.applicationActions.restartComputer("dev")
    expect(store.getSnapshot().source?.computers[0].lifecycleAction).toBe("restart")
    await store.refresh()
    expect(store.getSnapshot().source?.computers[0].lifecycleAction).toBe("restart")
    store.applicationActions.restartComputer("dev")
    store.applicationActions.stopComputer("dev")
    expect(invoke.mock.calls.filter(([name]) => name === "computer_action")).toHaveLength(1)
    finish(source)
    await vi.waitFor(() => expect(store.getSnapshot().source?.computers[0].lifecycleAction).toBeUndefined())
    store.dispose()
  })

  it("does not clear restart feedback when an earlier terminal action finishes", async () => {
    let finishTerminal!: (value: unknown) => void
    let finishRestart!: (value: unknown) => void
    const terminal = new Promise((resolve) => { finishTerminal = resolve })
    const restart = new Promise((resolve) => { finishRestart = resolve })
    const mock = native()
    const invoke = vi.fn(async (name: string, args?: Record<string, unknown>) => name === "computer_action" ? args?.action === "restart" ? restart : terminal : mock.invoke(name, args))
    const store = createProductionSource({ ...mock.bridge, invoke } as ProductionBridge)
    await store.initialize()
    store.applicationActions.openTerminal("dev")
    store.applicationActions.restartComputer("dev")
    finishTerminal(source)
    await vi.waitFor(() => expect(mock.invoke.mock.calls.filter(([name]) => name === "read_application_state").length).toBeGreaterThan(1))
    expect(store.getSnapshot().source?.computers[0].lifecycleAction).toBe("restart")
    finishRestart(source)
    await vi.waitFor(() => expect(store.getSnapshot().source?.computers[0].lifecycleAction).toBeUndefined())
    store.dispose()
  })

  it("publishes saved computer configuration before requesting live state", async () => {
    const configurations = source.computers.map(({ configuration }) => structuredClone(configuration))
    const invoke = nativeBridgeMock({ read_computer_configuration: () => ({ schemaVersion: 1, computers: configurations }) })
    const mock = native({ invoke: invoke as ProductionBridge["invoke"] })
    const store = createProductionSource(mock.bridge)
    const changed = vi.fn()
    store.subscribe(changed)

    await store.loadConfiguration()

    expect(invoke).toHaveBeenCalledExactlyOnceWith("read_computer_configuration")
    expect(store.getSnapshot()).toMatchObject({ savedConfigurations: configurations, source: null, loading: true, error: null })
    expect(changed).toHaveBeenCalledOnce()
    store.dispose()
  })

  it("accepts an empty saved configuration for a fresh install", async () => {
    const mock = native({ invoke: nativeBridgeMock({ read_computer_configuration: () => ({ schemaVersion: 1, computers: [] }) }) as ProductionBridge["invoke"] })
    const store = createProductionSource(mock.bridge)

    await store.loadConfiguration()

    expect(store.getSnapshot().savedConfigurations).toEqual([])
    expect(store.getSnapshot().source).toBeNull()
    store.dispose()
  })

  it.each([
    { schemaVersion: 2, computers: [] },
    { schemaVersion: 1, computers: [{ name: "invalid" }] },
    { schemaVersion: 1, computers: "unreadable" },
    null,
  ])("publishes no computer rows for malformed saved configuration, without failing startup: %j", async (configuration) => {
    const mock = native({ invoke: nativeBridgeMock({ read_computer_configuration: () => configuration }) as ProductionBridge["invoke"] })
    vi.spyOn(console, "error").mockImplementation(() => {})
    const store = createProductionSource(mock.bridge)

    // The list only feeds loading rows (H-10): an unreadable one shows none.
    await expect(store.loadConfiguration()).resolves.toBeUndefined()

    expect(store.getSnapshot().savedConfigurations).toEqual([])
    expect(store.getSnapshot().source).toBeNull()
    store.dispose()
  })

  it("shows an authoritative host push failure without success", async () => {
    const mock = native()
    const original = mock.invoke.getMockImplementation()!
    let failed = false
    const failure = { computer: "dev", repositoryPath: "/workspace/repo", status: "failed" as const, commitCount: 0, message: "Repository authorization was removed" }
    mock.invoke.mockImplementation((command, args) => {
      if (command === "start_repository_push") { failed = true; return Promise.resolve(failure) }
      if (command === "read_application_state" && failed) return Promise.resolve({ ...structuredClone(source), repositoryPushOperations: [failure] })
      return original(command, args)
    })
    const store = createProductionSource(mock.bridge)
    await store.initialize()
    store.applicationActions.pushRepository("dev", "/workspace/repo", pushTarget)
    await vi.waitFor(() => expect(store.getSnapshot().source?.repositoryPushOperations).toContainEqual({ computer: "dev", repositoryPath: "/workspace/repo", commitCount: 0, status: "failed", message: "Repository authorization was removed" }))
    expect(mock.invoke).toHaveBeenCalledWith("start_repository_push", { computer: "dev", repositoryPath: "/workspace/repo", operationId: expect.any(String), target: pushTarget })
    store.dispose()
  })

  it("blocks an unknown push until the user acknowledges checking GitHub", async () => {
    const mock = native()
    const original = mock.invoke.getMockImplementation()!
    const unknown = { computer: "dev", repositoryPath: "/workspace/repo", status: "unknown" as const, commitCount: 0, message: "Check this branch on GitHub before retrying." }
    let acknowledged = false
    mock.invoke.mockImplementation((command, args) => {
      if (command === "read_application_state") return Promise.resolve({ ...structuredClone(source), repositoryPushOperations: acknowledged ? [] : [unknown] })
      if (command === "dismiss_repository_push") { acknowledged = true; return Promise.resolve() }
      if (command === "start_repository_push") return Promise.resolve({ status: "failed", commitCount: 0, message: "Test completed" })
      return original(command, args)
    })
    const store = createProductionSource(mock.bridge)
    try {
      await store.initialize()
      store.applicationActions.pushRepository("dev", "/workspace/repo", pushTarget)
      expect(mock.invoke.mock.calls.some(([command]) => command === "start_repository_push")).toBe(false)
      store.applicationActions.dismissRepositoryPush!("dev", "/workspace/repo")
      await vi.waitFor(() => expect(store.getSnapshot().source?.repositoryPushOperations).toEqual([]))
      expect(mock.invoke.mock.calls.some(([command]) => command === "start_repository_push")).toBe(false)
      store.applicationActions.pushRepository("dev", "/workspace/repo", pushTarget)
      await vi.waitFor(() => expect(mock.invoke).toHaveBeenCalledWith("start_repository_push", { computer: "dev", repositoryPath: "/workspace/repo", operationId: expect.any(String), target: pushTarget }))
    } finally { store.dispose() }
  })

  it("reports a failed account connection once without inventing computer failures", async () => {
    const mock = native({}, { read_github_state: () => structuredClone(source.github) })
    const original = mock.invoke.getMockImplementation()!
    mock.invoke.mockImplementation((command, args) => command === "connect_github" ? Promise.reject(new Error("GitHub is not configured in this build")) : original(command, args))
    const store = createProductionSource(mock.bridge)
    await store.initialize()
    const before = store.getSnapshot().source!.github
    store.applicationActions.connectGitHub!()
    await vi.waitFor(() => expect(store.getSnapshot().source?.github.repositoryCatalogStatus).toEqual({ status: "unavailable", message: "GitHub operation failed: GitHub is not configured in this build", canRetry: false }))
    expect(store.getSnapshot().source?.github.computerOperations).toEqual(before.computerOperations)
    expect(store.getSnapshot().source?.github.account).toEqual(before.account)
    store.dispose()
  })

  it("shows native OAuth progress while browser login is pending, then connected state", async () => {
    const events = new Map<string, () => void>()
    let liveState = { ...source, github: { ...source.github, state: "disconnected" as const, account: undefined } } as typeof source
    let finishLogin!: (value: unknown) => void
    const invoke = nativeBridgeMock({
      ...initializationHandlers(),
      read_application_state: () => structuredClone(liveState),
      read_backup_state: () => structuredClone(backup),
      connect_github: () => new Promise((resolve) => { finishLogin = resolve }),
    })
    const store = createProductionSource({ invoke, listen: async (event, handler) => { events.set(event, handler); return () => events.delete(event) } } as ProductionBridge)
    await store.initialize()
    store.applicationActions.connectGitHub!()
    liveState = { ...liveState, github: { ...liveState.github, state: "connecting" } }
    events.get("silo://application-state-changed")!()
    await vi.waitFor(() => expect(store.getSnapshot().source?.github.state).toBe("connecting"))
    finishLogin({ ...liveState.github, state: "connected", account: "test-account" })
    await vi.waitFor(() => expect(store.getSnapshot().source?.github.state).toBe("connected"))
    expect(store.getSnapshot().source?.github.account).toBe("test-account")
    store.dispose()
  })

  it.each(["resolve", "reject"])("cancels authorization without a failure or stale %s changing account state", async (completion) => {
    const mock = native()
    const original = mock.invoke.getMockImplementation()!
    let finishLogin!: (value: unknown) => void
    let failLogin!: (cause: Error) => void
    const disconnected = { ...source.github, state: "disconnected", account: null }
    mock.invoke.mockImplementation((command, args) => {
      if (command === "connect_github") return new Promise((resolve, reject) => { finishLogin = resolve; failLogin = reject })
      if (command === "cancel_github_connection") return Promise.resolve(disconnected)
      return original(command, args)
    })
    const store = createProductionSource(mock.bridge)
    await store.initialize()
    store.applicationActions.connectGitHub!()
    store.applicationActions.cancelGitHubConnection!()
    await vi.waitFor(() => expect(store.getSnapshot().source?.github.state).toBe("disconnected"))
    if (completion === "resolve") finishLogin({ ...source.github, state: "connected", account: "late-account" })
    else failLogin(new Error("GitHub authorization was cancelled"))
    await new Promise(resolve => setTimeout(resolve, 0))
    expect(store.getSnapshot().source?.github.state).toBe("disconnected")
    expect(store.getSnapshot().source?.github.account).toBeUndefined()
    expect(store.getSnapshot().error).toBeNull()
    expect((store.getSnapshot().setupActivity ?? []).some(event => event.message.includes("did not complete") || event.message === "GitHub account connected.")).toBe(false)
    store.dispose()
  })

  it("loads newly authorized repositories once when returning from GitHub", async () => {
    const mock = native({}, { manage_github_repositories: () => undefined })
    const updatedGitHub = { ...source.github, state: "connected", repositoryCatalog: ["acme/new-repository"] }
    const invoke = vi.fn(async (command: string, args?: Record<string, unknown>) => {
      if (command === "refresh_github_repositories") return updatedGitHub
      return mock.invoke(command, args)
    })
    const store = createProductionSource({ ...mock.bridge, invoke } as ProductionBridge)
    await store.initialize()
    try {
      window.dispatchEvent(new Event("focus"))
      await new Promise(resolve => setTimeout(resolve, 0))
      expect(invoke.mock.calls.filter(([command]) => command === "refresh_github_repositories")).toHaveLength(0)
      store.applicationActions.manageGitHubRepositories!()
      expect(invoke).toHaveBeenCalledWith("manage_github_repositories")
      window.dispatchEvent(new Event("focus"))
      await vi.waitFor(() => expect(store.getSnapshot().source?.github.repositoryCatalog).toEqual(["acme/new-repository"]))
      window.dispatchEvent(new Event("focus"))
      await new Promise(resolve => setTimeout(resolve, 0))
      expect(invoke.mock.calls.filter(([command]) => command === "refresh_github_repositories")).toHaveLength(1)
    } finally { store.dispose() }
  })

  it("reopens the browser without replacing an active connection attempt", async () => {
    const mock = native()
    const original = mock.invoke.getMockImplementation()!
    let finishLogin!: (value: unknown) => void
    mock.invoke.mockImplementation((command, args) => {
      if (command === "connect_github") return new Promise(resolve => { finishLogin = resolve })
      if (command === "reopen_github_authorization") return Promise.resolve(null)
      return original(command, args)
    })
    const store = createProductionSource(mock.bridge)
    await store.initialize()
    store.applicationActions.connectGitHub!()
    store.applicationActions.reopenGitHubAuthorization!()
    expect(mock.invoke).toHaveBeenCalledWith("reopen_github_authorization")
    finishLogin({ ...source.github, state: "connected", account: "test-account" })
    await vi.waitFor(() => expect(store.getSnapshot().source?.github.account).toBe("test-account"))
    expect(store.getSnapshot().error).toBeNull()
    store.dispose()
  })

  it("restores native disconnected state after cancelled browser login", async () => {
    const mock = native()
    const original = mock.invoke.getMockImplementation()!
    mock.invoke.mockImplementation((command, args) => {
      if (command === "read_application_state") return Promise.resolve({ ...source, github: { ...source.github, state: "connecting" } })
      if (command === "connect_github") return Promise.reject(new Error("GitHub authorization was cancelled"))
      if (command === "read_github_state") return Promise.resolve({ ...source.github, state: "disconnected", account: null })
      return original(command, args)
    })
    const store = createProductionSource(mock.bridge)
    await store.initialize()
    store.applicationActions.connectGitHub!()
    await vi.waitFor(() => expect(store.getSnapshot().source?.github.state).toBe("disconnected"))
    expect(store.getSnapshot().source?.github.account).toBeUndefined()
    expect(store.getSnapshot().source?.github.repositoryCatalogStatus).toMatchObject({ status: "unavailable", canRetry: false })
    store.dispose()
  })

  it("returns native save failures without making the available repository catalog unavailable", async () => {
    const mock = native()
    const original = mock.invoke.getMockImplementation()!
    mock.invoke.mockImplementation((command, args) => command === "save_github_configuration" ? Promise.reject(new Error("Invalid Git identity settings.")) : original(command, args))
    const store = createProductionSource(mock.bridge)
    await store.initialize()
    const catalog = store.getSnapshot().source?.github.repositoryCatalogStatus
    await expect(store.applicationActions.saveGitHubConfiguration!({ deviceIdentity: null, computers: [] })).rejects.toThrow("Invalid Git identity settings.")
    expect(store.getSnapshot().source?.github.repositoryCatalogStatus).toEqual(catalog)
    expect(store.getSnapshot().error).toContain("Invalid Git identity settings.")
    store.dispose()
  })

  it("ignores an older settings response after a newer save completes", async () => {
    const mock = native()
    const original = mock.invoke.getMockImplementation()!
    const pending: Array<(value: unknown) => void> = []
    mock.invoke.mockImplementation((command, args) => command === "save_github_configuration" ? new Promise((resolve) => { pending.push(resolve) }) : original(command, args))
    const store = createProductionSource(mock.bridge)
    await store.initialize()
    const configuration = { baseRevision: 0, deviceIdentity: null, computers: [] }
    store.applicationActions.saveGitHubConfiguration!(configuration)
    store.applicationActions.saveGitHubConfiguration!({ ...configuration, baseRevision: 1 })
    pending[1]({ ...source.github, policyRevision: 2, accessEnabled: false })
    await vi.waitFor(() => expect(store.getSnapshot().source?.github.policyRevision).toBe(2))
    pending[0]({ ...source.github, policyRevision: 1, accessEnabled: true })
    await new Promise((resolve) => setTimeout(resolve, 0))
    expect(store.getSnapshot().source?.github.accessEnabled).toBe(false)
    expect(store.getSnapshot().source?.github.policyRevision).toBe(2)
    store.dispose()
  })

  it("keeps all-repository intent and waits for native acknowledgment before showing changed access", async () => {
    let resolveMutation!: (value: unknown) => void
    const request = { deviceIdentity: null, computers: [{ computer: "dev", repositoryMode: "all" as const, allRepositoriesAllowChanges: false, repositories: [], identity: { name: "", email: "", apply: false } }] }
    const mock = native()
    const original = mock.invoke.getMockImplementation()!
    mock.invoke.mockImplementation((command, args) => command === "save_github_configuration" ? new Promise((resolve) => { resolveMutation = resolve }) : original(command, args))
    const store = createProductionSource(mock.bridge)
    await store.initialize()
    const before = store.getSnapshot().source?.github
    store.applicationActions.saveGitHubConfiguration!(request)
    expect(mock.invoke).toHaveBeenCalledWith("save_github_configuration", { configuration: request })
    expect(store.getSnapshot().source?.github).toBe(before)
    resolveMutation({ ...before, ...request, computerOperations: [{ computer: "dev", status: "failed", message: "Runtime did not acknowledge access", canRetry: true }] })
    await vi.waitFor(() => expect(store.getSnapshot().source?.github.computerOperations?.[0].status).toBe("failed"))
    expect(store.getSnapshot().source?.github.computers?.[0].repositoryMode).toBe("all")
    store.dispose()
  })

  it("updates asynchronous computer acknowledgment from a native state event", async () => {
    const events = new Map<string, () => void>()
    let github = { ...source.github, policyRevision: 11, computerOperations: [{ computer: "dev", status: "applying" as const, message: "Applying access" }] } as typeof source.github
    const store = createProductionSource({
      invoke: nativeBridgeMock({
        ...initializationHandlers(),
        read_application_state: () => ({ ...source, github }),
        read_backup_state: () => backup,
        save_github_configuration: () => github,
      }),
      listen: async (event, handler) => { events.set(event, handler); return () => events.delete(event) },
    } as ProductionBridge)
    await store.initialize()
    store.applicationActions.saveGitHubConfiguration!({ deviceIdentity: null, computers: [] })
    await new Promise((resolve) => setTimeout(resolve, 0))
    expect(store.getSnapshot().source?.github.computerOperations?.[0].status).toBe("applying")
    github = { ...github, computerOperations: [{ computer: "dev", status: "succeeded", message: "Verified access" }] }
    events.get("silo://application-state-changed")!()
    await vi.waitFor(() => expect(store.getSnapshot().source?.github.computerOperations?.[0].status).toBe("succeeded"))
    store.dispose()
  })

  it("accepts the exact native GitHub states without granting implicit all-repository writes", () => {
    const states = JSON.parse(readFileSync(resolve(dirname(fileURLToPath(import.meta.url)), "../test/contracts/github-state.json"), "utf8")) as Array<typeof source.github>
    for (const github of states) {
      const parsed = parseApplicationSource({ ...source, github }).github
      expect(parsed).toEqual(github)
      expect(parsed.computers?.[0]).toMatchObject({ repositoryMode: "all", allRepositoriesAllowChanges: false, repositories: [] })
      expect(parsed.policyRevision).toBe(7)
    }
  })

  it("accepts activity serialized by the native journal", () => {
    const events = JSON.parse(readFileSync(resolve(dirname(fileURLToPath(import.meta.url)), "../test/contracts/setup-activity.json"), "utf8")) as unknown[]
    expect(events.map((event) => siloProgressEventSchema.parse(event))).toEqual(events)
  })
  it("accepts the exact export and import state serialized by the Rust bridge", () => {
    const state = JSON.parse(readFileSync(resolve(dirname(fileURLToPath(import.meta.url)), "../test/contracts/backup-state.json"), "utf8"))
    expect(parseBackupState(state)).toEqual(state)
  })

  it("accepts running and failed-result variants serialized by Rust", () => {
    const operations = JSON.parse(readFileSync(resolve(dirname(fileURLToPath(import.meta.url)), "../test/contracts/backup-operations.json"), "utf8")) as unknown[]
    for (const operation of operations) expect(parseBackupState({ ...backup, operation }).operation).toEqual(operation)
  })

  it("accepts the unseen marker the Rust bridge serializes only when a result was not shown yet", () => {
    const result = { operation: "restore", archive: backup.archives[0], runningNames: [], targetName: "copy", kind: "result", outcome: "failed", title: "Import interrupted before the upgrade", message: "Silo closed before this import finished.", detail: "No computer was added. Import the file again." }
    expect(parseBackupState({ ...backup, operationId: "op-1", operation: result, resultUnseen: true }).resultUnseen).toBe(true)
    // Omitted when false, like every other result.
    expect(parseBackupState({ ...backup, operationId: "op-1", operation: result }).resultUnseen).toBeUndefined()
    expect(() => parseBackupState({ ...backup, resultUnseen: "yes" })).toThrow()
  })

  it("rejects malformed authoritative state instead of substituting preview data", () => {
    expect(() => parseApplicationSource({ computers: [] })).toThrow()
    expect(() => parseBackupState({ availability: "available", archives: [] })).toThrow()
  })

  it("does not hide a failed live-update subscription behind an initial snapshot", async () => {
    const unsubscribe = vi.fn()
    const listen = vi.fn().mockResolvedValueOnce(unsubscribe).mockRejectedValueOnce(new Error("event channel closed")).mockResolvedValue(unsubscribe)
    const mock = native({ listen })
    const store = createProductionSource(mock.bridge)
    await expect(store.initialize()).rejects.toThrow("Silo could not subscribe to application updates: event channel closed")
    expect(unsubscribe).toHaveBeenCalledTimes(listen.mock.calls.length - 1)
    expect(store.getSnapshot().source).toBeNull()
    store.dispose()
  })

  it("loads both native snapshots and refreshes after a computer action", async () => {
    const mock = native()
    const store = createProductionSource(mock.bridge)
    await store.initialize()
    expect(store.getSnapshot().source?.computers[0].configuration.name).toBe("dev")
    expect(store.getSnapshot().backup.archives[0].archivePath).toBe("/tmp/dev.silo-backup")
    store.applicationActions.stopComputer("dev")
    await vi.waitFor(() => expect(mock.invoke).toHaveBeenCalledWith("computer_action", { action: "stop", name: "dev" }))
    await vi.waitFor(() => expect(mock.invoke.mock.calls.filter(([command]) => command === "read_application_state")).toHaveLength(2))
    store.dispose()
  })

  it("keeps detected identity across computer results that omit it, then accepts a fresh missing identity", async () => {
    const deviceIdentity = { name: "Host Author", email: "host@example.test" }
    let currentIdentity: typeof deviceIdentity | undefined = deviceIdentity
    const computerResult = structuredClone(source)
    delete computerResult.github.deviceIdentity
    const mock = native({ invoke: nativeBridgeMock({
      ...initializationHandlers(),
      read_application_state: () => ({ ...structuredClone(source), github: { ...source.github, deviceIdentity: currentIdentity } }),
      read_backup_state: () => structuredClone(backup),
      read_setup_activity: () => [],
      change_computer_configuration: () => computerResult,
    }) as ProductionBridge["invoke"] })
    const store = createProductionSource(mock.bridge)
    await store.initialize()
    const committed = source.computers.filter(({ device }) => !device).map(({ configuration }) => configuration)
    const first = committed[0]
    const edited = { ...first, cpus: first.cpus === 1 ? 2 : 1 }
    await store.configureConfigurations({ schemaVersion: 1, computers: [edited, ...committed.slice(1)] })
    expect(store.getSnapshot().source?.github.deviceIdentity).toEqual(deviceIdentity)
    currentIdentity = undefined
    await store.refresh()
    expect(store.getSnapshot().source?.github.deviceIdentity).toBeUndefined()
    store.dispose()
  })

  it("keeps captured logs during configuration and reloads them after it finishes", async () => {
    const initial = structuredClone(source)
    const oldLog = { line: "computer booted", occurredAt: "2026-09-10T09:00:00Z" }
    const newLog = { line: "Computer stopped", occurredAt: "2026-09-10T09:01:00Z" }
    initial.computers[0].logs = [oldLog]
    const mutation = structuredClone(initial)
    mutation.computers[0].logs = []
    let reads = 0
    const invoke = nativeBridgeMock({
      ...initializationHandlers(),
      read_application_state: async () => {
        const result = structuredClone(initial)
        if (++reads > 1) result.computers[0].logs = [oldLog, newLog]
        return result
      },
      read_backup_state: () => structuredClone(backup),
      read_setup_activity: () => [],
      change_computer_configuration: () => mutation,
    })
    const store = createProductionSource(native({ invoke: invoke as ProductionBridge["invoke"] }).bridge)
    await store.initialize()
    const observed: number[] = []
    const unsubscribe = store.subscribe(() => observed.push(store.getSnapshot().source?.computers[0].logs.length ?? -1))
    const committed = initial.computers.filter(({ device }) => !device).map(({ configuration }) => configuration)
    const first = committed[0]
    const edited = { ...first, cpus: first.cpus === 1 ? 2 : 1 }
    await store.configureConfigurations({ schemaVersion: 1, computers: [edited, ...committed.slice(1)] })
    expect(observed).not.toContain(0)
    expect(store.getSnapshot().source?.computers[0].logs).toEqual([oldLog, newLog])
    unsubscribe()
    store.dispose()
  })

  it("retains an unscoped preflight error and lets dismissal unlock the committed configuration", async () => {
    const invoke = nativeBridgeMock({
      ...initializationHandlers(),
      read_application_state: () => structuredClone(source),
      read_backup_state: () => structuredClone(backup),
      read_setup_activity: () => [],
      change_computer_configuration: async () => { throw new Error("Stop computer 'dev' before removing it.") },
    })
    const store = createProductionSource(native({ invoke: invoke as ProductionBridge["invoke"] }).bridge)
    await store.initialize()
    const committedComputers = store.getSnapshot().source?.computers.map(({ configuration }) => configuration)
    await expect(store.configureConfigurations({ schemaVersion: 1, computers: [] })).rejects.toThrow("Stop computer")
    expect(store.getSnapshot().source?.computerConfigurationOperation).toMatchObject({ status: "failed", error: { computer: null, message: "Stop computer 'dev' before removing it." } })
    store.applicationActions.dismissComputerConfigurationError()
    await store.refresh()
    expect(store.getSnapshot().source?.computerConfigurationOperation).toBeNull()
    expect(store.getSnapshot().source?.computers.map(({ configuration }) => configuration)).toEqual(committedComputers)
    store.dispose()
  })

  it("retries verification only for the requested computer without resending the list", async () => {
    let attempts = 0
    const invoke = nativeBridgeMock({
      ...initializationHandlers(),
      read_application_state: () => structuredClone(source),
      read_backup_state: () => structuredClone(backup),
      read_setup_activity: () => [],
      change_computer_configuration: async () => {
        if (++attempts === 1) throw new Error("Verification failed")
        return structuredClone(source)
      },
      retry_computer_configuration: () => structuredClone(source),
    })
    const store = createProductionSource(native({ invoke: invoke as ProductionBridge["invoke"] }).bridge)
    await store.initialize()
    const committed = store.getSnapshot().source!.computers.filter((computer) => !computer.device).map(({ configuration }) => configuration)
    const original = committed[0]
    const edited = { ...original, cpus: original.cpus === 1 ? 2 : 1 }
    await expect(store.configureConfigurations({ schemaVersion: 1, computers: [edited, ...committed.slice(1)] })).rejects.toThrow("Verification failed")
    store.applicationActions.retryComputerConfiguration("dev")
    // The retry resumes the recorded attempt by computer; it does not resend a list.
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith("retry_computer_configuration", {
      requestId: expect.any(String), retryComputer: "dev",
    }))
    await vi.waitFor(() => expect(store.getSnapshot().source?.computerConfigurationOperation).toBeNull())
    store.dispose()
  })

  it("routes an edit to a targeted change carrying the committed configuration as expected", async () => {
    const invoke = nativeBridgeMock({
      ...initializationHandlers(),
      read_application_state: () => structuredClone(source),
      read_backup_state: () => structuredClone(backup),
      read_setup_activity: () => [],
      change_computer_configuration: () => structuredClone(source),
    })
    const store = createProductionSource(native({ invoke: invoke as ProductionBridge["invoke"] }).bridge)
    await store.initialize()
    const committed = store.getSnapshot().source!.computers.filter((computer) => !computer.device).map(({ configuration }) => configuration)
    const original = committed[0]
    const edited = { ...original, cpus: original.cpus === 1 ? 2 : 1 }
    store.applicationActions.saveComputerConfiguration({ schemaVersion: 1, computers: [edited, ...committed.slice(1)] })
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith("change_computer_configuration", {
      change: { kind: "upsert", configuration: edited, expected: original },
      requestId: expect.any(String),
    }))
    expect(invoke).not.toHaveBeenCalledWith("retry_computer_configuration", expect.anything())
    store.dispose()
  })

  it("carries the editing baseline as expected even when the committed snapshot has moved on", async () => {
    const invoke = nativeBridgeMock({
      ...initializationHandlers(),
      read_application_state: () => structuredClone(source),
      read_backup_state: () => structuredClone(backup),
      read_setup_activity: () => [],
      change_computer_configuration: () => structuredClone(source),
    })
    const store = createProductionSource(native({ invoke: invoke as ProductionBridge["invoke"] }).bridge)
    await store.initialize()
    const committed = store.getSnapshot().source!.computers.filter((computer) => !computer.device).map(({ configuration }) => configuration)
    const original = committed[0]
    // The user opened the editor while maxCPUs was 99; the live committed snapshot never
    // held that value. The save must send the baseline, not the current committed config.
    const baselineEntry = { ...original, maxCPUs: 99 }
    const baseline = [baselineEntry, ...committed.slice(1)]
    const edited = { ...original, maxCPUs: 7 }
    store.applicationActions.saveComputerConfiguration({ schemaVersion: 1, computers: [edited, ...committed.slice(1)] }, baseline)
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith("change_computer_configuration", {
      change: { kind: "upsert", configuration: edited, expected: baselineEntry },
      requestId: expect.any(String),
    }))
    store.dispose()
  })

  it("reports unreadable activity without replacing it with success or raw diagnostics", async () => {
    const mock = native({ invoke: nativeBridgeMock({
      ...initializationHandlers(),
      read_setup_activity: async () => { throw new Error("private path and token") },
      read_application_state: () => structuredClone(source),
      read_backup_state: () => structuredClone(backup),
    }) as ProductionBridge["invoke"] })
    const store = createProductionSource(mock.bridge)
    await store.initialize()
    expect(store.getSnapshot().setupActivityError).toBe("Saved setup activity could not be loaded. Retry by reopening Silo.")
    expect(store.getSnapshot().setupEvents).toEqual([])
    expect(store.getSnapshot().source).not.toBeNull()
    store.dispose()
  })

  it("coalesces duplicate in-flight computer actions", async () => {
    let finish: (() => void) | undefined
    const mock = native()
    mock.invoke.mockImplementation(nativeBridgeMock({
      ...initializationHandlers(),
      read_application_state: () => structuredClone(source),
      read_backup_state: () => structuredClone(backup),
      computer_action: async () => {
        await new Promise<void>((resolve) => { finish = resolve })
        return structuredClone(source)
      },
    }))
    const store = createProductionSource(mock.bridge)
    await store.initialize()
    store.applicationActions.stopComputer("dev")
    store.applicationActions.stopComputer("dev")
    await vi.waitFor(() => expect(mock.invoke.mock.calls.filter(([command]) => command === "computer_action")).toHaveLength(1))
    finish?.()
    store.dispose()
  })

  it("keeps a computer action failure through refresh and clears it after a successful retry", async () => {
    const mock = native()
    let refuse = true
    mock.invoke.mockImplementation(nativeBridgeMock({
      ...initializationHandlers(),
      read_application_state: () => structuredClone(source),
      read_backup_state: () => structuredClone(backup),
      computer_action: async () => {
        if (refuse) throw new Error("runtime refused stop")
        return structuredClone(source)
      },
    }))
    const store = createProductionSource(mock.bridge)
    await store.initialize()
    store.applicationActions.stopComputer("dev")
    await vi.waitFor(() => expect(store.getSnapshot().source?.computers.find(({ configuration }) => configuration.name === "dev")?.lifecycleFailure).toBe("Stop failed: runtime refused stop"))
    expect(store.getSnapshot().source?.computers.find(({ configuration }) => configuration.name === "dev")?.state).toBe("running")
    expect(store.getSnapshot().source?.computerOperationsUnavailable).toBeUndefined()
    expect(store.getSnapshot().source?.computers.find(({ configuration }) => configuration.name === "dev")?.freshness).toBe("stale")
    expect(store.getSnapshot().source?.computers.find(({ configuration }) => configuration.name === "dev")?.lifecycleAction).toBeUndefined()
    await store.refresh()
    expect(store.getSnapshot().source?.computers.find(({ configuration }) => configuration.name === "dev")?.lifecycleFailure).toContain("runtime refused stop")
    expect(store.getSnapshot().source?.computers.find(({ configuration }) => configuration.name === "dev")?.freshness).toBe("fresh")
    expect(store.getSnapshot().source?.computers.filter(({ configuration }) => configuration.name !== "dev").every(computer => !computer.lifecycleFailure && computer.freshness === "fresh")).toBe(true)
    refuse = false
    store.applicationActions.stopComputer("dev")
    await vi.waitFor(() => expect(store.getSnapshot().source?.computers.find(({ configuration }) => configuration.name === "dev")?.lifecycleFailure).toBeUndefined())
    store.dispose()
  })

  it("keeps the loaded application, marked stale, when an authoritative refresh fails", async () => {
    const mock = native()
    const store = createProductionSource(mock.bridge)
    await store.initialize()
    expect(store.getSnapshot().source?.computers[0].configuration.name).toBe("dev")
    mock.invoke.mockImplementation(nativeBridgeMock({
      ...initializationHandlers(),
      read_application_state: async () => { throw new Error("runtime state unavailable") },
      read_backup_state: () => structuredClone(backup),
    }))
    await store.refresh()
    const message = "Silo could not read application state: runtime state unavailable"
    expect(store.getSnapshot().source?.computerOperationsUnavailable).toBe(message)
    expect(store.getSnapshot().source?.computers.every(({ freshness, attention }) => freshness === "stale" && attention?.message === message)).toBe(true)
    expect(store.getSnapshot().error).toBe(message)
    store.dispose()
  })

  it("keeps the mounted application and restore progress while configuration is updating", async () => {
    const mock = native()
    const store = createProductionSource(mock.bridge)
    await store.initialize()
    const updating = { ...backup, operation: { kind: "running", operation: "restore", archive: backup.archives[0], runningNames: [], targetName: "copy", progress: 0, indeterminate: true, phases: [{ title: "Creating restored computer", detail: "", tone: "running" }] } }
    mock.invoke.mockImplementation(nativeBridgeMock({
      ...initializationHandlers(),
      read_application_state: async () => { throw { code: "update_in_progress", message: "Please wait for configuration." } },
      read_backup_state: () => updating,
    }))
    await store.refresh()
    expect(store.getSnapshot().source?.computers[0].configuration.name).toBe("dev")
    expect(store.getSnapshot().error).toBeNull()
    expect(store.getSnapshot().backup.operation).toMatchObject({ kind: "running", phases: [{ title: "Creating restored computer" }] })
    // A later authoritative snapshot remains responsible for reporting success.
    mock.invoke.mockImplementation(nativeBridgeMock({
      ...initializationHandlers(),
      read_application_state: () => structuredClone(source),
      read_backup_state: () => ({ ...backup, operation: { kind: "result", operation: "restore", archive: backup.archives[0], runningNames: [], outcome: "success", title: "Restored", message: "Computer restored successfully." } }),
    }))
    await store.refresh()
    expect(store.getSnapshot().source).not.toBeNull()
    expect(store.getSnapshot().backup.operation).toMatchObject({ kind: "result", outcome: "success" })
    store.dispose()
  })

  it("keeps the requested restore running and reports malformed native operation state (H-35)", async () => {
    let broken = false
    const mock = native()
    mock.invoke.mockImplementation(nativeBridgeMock({
      ...initializationHandlers(),
      read_application_state: () => structuredClone(source),
      start_restore: async () => { broken = true; return undefined },
      read_backup_state: () => broken ? { ...backup, operation: { kind: "running", running_names: [] } } : structuredClone(backup),
    }))
    const store = createProductionSource(mock.bridge)
    await store.initialize()
    store.backupActions.startRestore(backup.archives[0], "restored", "dev")
    await vi.waitFor(() => expect(store.getSnapshot().backup.availabilityMessage).toContain("invalid export and import state"))
    // A malformed read is not evidence that the restore failed.
    expect(store.getSnapshot().backup.operation).toMatchObject({ kind: "running", operation: "restore", targetName: "restored" })
    store.dispose()
  })

  it("shows restore immediately, ignores stale results and dismisses results locally", async () => {
    const completed = { operation: "backup" as const, archive: backup.archives[0], runningNames: [], kind: "result" as const, outcome: "success" as const, title: "Export complete", message: "Export completed successfully." }
    let release: (() => void) | undefined
    let current = { ...structuredClone(backup), operation: completed, operationId: "first-operation" } as BackupState
    const mock = native({ invoke: nativeBridgeMock({
      ...initializationHandlers(),
      read_application_state: () => structuredClone(source),
      read_backup_state: () => structuredClone(current),
      start_restore: async () => { await new Promise<void>(resolve => { release = resolve }) },
      dismiss_backup_operation: async () => {
        if (current.operation?.kind !== "result") return false
        current = { ...current, operation: null }
        return true
      },
    }) as ProductionBridge["invoke"] })
    const store = createProductionSource(mock.bridge)
    await store.initialize()
    store.backupActions.dismissOperation()
    expect(store.getSnapshot().backup.operation).toBeNull()
    await store.refresh()
    expect(store.getSnapshot().backup.operation).toBeNull()
    store.backupActions.startRestore(backup.archives[0], "restored", "dev")
    expect(store.getSnapshot().backup.operation).toMatchObject({ kind: "running", operation: "restore", indeterminate: true })
    await store.refresh()
    expect(store.getSnapshot().backup.operation?.kind).toBe("running")
    store.backupActions.dismissOperation()
    expect(store.getSnapshot().backup.operation?.kind).toBe("running")
    expect(mock.bridge.invoke).toHaveBeenCalledWith("dismiss_backup_operation", { expectedOperation: completed, expectedOperationId: "first-operation" })
    expect(vi.mocked(mock.bridge.invoke).mock.calls.filter(([command]) => command === "dismiss_backup_operation")).toHaveLength(1)
    current = { ...current, operationId: "second-operation", operation: { ...completed, operation: "restore", targetName: "restored", title: "Restore complete" } }
    await store.refresh()
    expect(store.getSnapshot().backup.operation).toMatchObject({ kind: "result", operation: "restore" })
    store.backupActions.dismissOperation()
    release?.()
    await new Promise(resolve => setTimeout(resolve, 0))
    await store.refresh()
    expect(store.getSnapshot().backup.operation).toBeNull()
    store.dispose()
  })

  it("keeps a submission failure visible across refreshes until dismissed", async () => {
    const completed = { operation: "backup" as const, archive: backup.archives[0], runningNames: [], kind: "result" as const, outcome: "success" as const, title: "Export complete", message: "Export completed successfully." }
    const mock = native({ invoke: nativeBridgeMock({
      ...initializationHandlers(),
      read_application_state: () => structuredClone(source),
      read_backup_state: () => ({ ...backup, operation: completed }),
      start_restore: async () => { throw new Error("Computer name already exists") },
      dismiss_backup_operation: () => false,
    }) as ProductionBridge["invoke"] })
    const store = createProductionSource(mock.bridge)
    await store.initialize()
    store.backupActions.startRestore(backup.archives[0], "dev", "dev")
    await vi.waitFor(() => expect(store.getSnapshot().backup.operation).toMatchObject({ kind: "result", outcome: "failed" }))
    await store.refresh()
    expect(store.getSnapshot().backup.operation).toMatchObject({ kind: "result", outcome: "failed", message: "Computer name already exists" })
    store.backupActions.dismissOperation()
    await store.refresh()
    expect(store.getSnapshot().backup.operation).toBeNull()
    store.dispose()
  })

  it("announces archive selection before waiting for validation", async () => {
    let release: (() => void) | undefined
    const mock = native({ invoke: nativeBridgeMock({
      ...initializationHandlers(),
      read_application_state: () => structuredClone(source),
      read_backup_state: () => structuredClone(backup),
      choose_backup_archive: () => "/tmp/dev.silo-backup",
      inspect_backup_archive: async () => {
        await new Promise<void>(resolve => { release = resolve })
        return { archive: backup.archives[0], valid: true }
      },
    }) as ProductionBridge["invoke"] })
    const store = createProductionSource(mock.bridge)
    await store.initialize()
    const selected = vi.fn()
    const inspection = store.backupActions.chooseArchive(selected)
    await vi.waitFor(() => expect(selected).toHaveBeenCalledWith("/tmp/dev.silo-backup"))
    release?.()
    await inspection
    store.dispose()
  })

  it("coalesces duplicate in-flight backup starts", async () => {
    let finish: (() => void) | undefined
    const mock = native()
    mock.invoke.mockImplementation(nativeBridgeMock({
      ...initializationHandlers(),
      read_application_state: () => structuredClone(source),
      read_backup_state: () => structuredClone(backup),
      start_backup: async () => { await new Promise<void>((resolve) => { finish = resolve }) },
    }))
    const store = createProductionSource(mock.bridge)
    await store.initialize()
    store.backupActions.startBackup("/Volumes/Backups", ["dev"])
    store.backupActions.startBackup("/Volumes/Backups", ["dev"])
    await vi.waitFor(() => expect(mock.invoke.mock.calls.filter(([command]) => command === "start_backup")).toHaveLength(1))
    finish?.()
    store.dispose()
  })

  it("uses native archive paths and destination pickers", async () => {
    const mock = native()
    mock.invoke.mockImplementation(nativeBridgeMock({
      ...initializationHandlers(),
      read_application_state: () => structuredClone(source),
      read_backup_state: () => structuredClone(backup),
      choose_backup_destination: () => "/Volumes/Backups",
      choose_backup_archive: () => "/Volumes/Backups/dev.silo-backup",
      inspect_backup_archive: () => ({ archive: backup.archives[0], valid: true }),
      start_restore: () => undefined,
    }))
    const store = createProductionSource(mock.bridge)
    await store.initialize()
    expect(await store.backupActions.chooseDestination()).toBe("/Volumes/Backups")
    await store.backupActions.chooseArchive()
    expect(mock.invoke).toHaveBeenCalledWith("inspect_backup_archive", { archivePath: "/Volumes/Backups/dev.silo-backup", requestId: expect.any(String) })
    store.backupActions.startRestore(backup.archives[0], "dev-restored")
    await vi.waitFor(() => expect(mock.invoke).toHaveBeenCalledWith("start_restore", { archivePath: "/tmp/dev.silo-backup", newName: "dev-restored" }))
    store.dispose()
  })
})

describe("remote SSH access", () => {
  const computerId = source.computers[0].configuration.id
  const target = `silo-remote:office:${encodeURIComponent(computerId)}`
  const request = { computer: target, enabled: true, port: 2222, bindAddress: "127.0.0.1", keys: [] }
  const row = { ...request, state: "listening", message: null, fingerprint: "SHA256:fixture", deviceName: "Office Mac", addresses: ["192.168.1.42"] }
  function fixture() {
    const mock = native()
    let failOffice: unknown
    let failLocal = false
    let officeRead: Promise<unknown> | undefined
    let officeSave: Promise<unknown> | undefined
    const invoke = vi.fn(async (command: string, args?: Record<string, unknown>): Promise<unknown> => {
      if (command === "device_list") return [{ id: "office", name: "Office Mac", address: "user@office" }, { id: "lab", name: "Lab Mac", address: "user@lab" }]
      if (command === "device_snapshot") return { ...source, computers: [source.computers[0]] }
      if (command === "read_ssh_access_state") { if (failLocal) throw new Error("Local failed"); return { computers: [{ ...row, computer: "dev", deviceName: "Laptop" }] } }
      if (command === "remote_ssh_access_state") {
        if (args?.deviceId === "office") { if (failOffice) throw failOffice; if (officeRead) return officeRead }
        return { computers: [{ ...row, computer: `silo-remote:${args?.deviceId}:${encodeURIComponent(computerId)}`, deviceName: args?.deviceId === "office" ? "Office Mac" : "Lab Mac" }] }
      }
      if (command === "remote_save_ssh_access") { if (officeSave) return officeSave; const { deviceId, computerId: id, ...settings } = args!; return { computers: [{ ...row, ...settings, computer: `silo-remote:${deviceId}:${encodeURIComponent(String(id))}` }] } }
      if (command === "save_ssh_access") return { computers: [{ ...row, ...args, deviceName: "Laptop" }] }
      return mock.invoke(command, args)
    })
    const store = createProductionSource({ ...mock.bridge, invoke } as ProductionBridge)
    return { store, invoke, failOffice: (message: unknown = new Error("private remote details")) => { failOffice = message }, failLocal: () => { failLocal = true }, delayOffice: (promise: Promise<unknown>) => { officeRead = promise }, delaySave: (promise: Promise<unknown>) => { officeSave = promise } }
  }
  it("routes same-name remote computers by immutable owner and computer IDs and retains other owners", async () => {
    const { store, invoke } = fixture()
    try {
      await store.initialize(); await store.applicationActions.refreshSshAccess!()
      expect(store.getSnapshot().source?.sshAccess?.computers.map(item => item.computer)).toEqual(["dev", target, `silo-remote:lab:${encodeURIComponent(computerId)}`])
      await store.applicationActions.saveSshAccess!({ ...request, keys: ["ssh-ed25519 synthetic-public-key"] })
      expect(invoke).toHaveBeenCalledWith("remote_save_ssh_access", { deviceId: "office", computerId, enabled: true, port: 2222, bindAddress: "127.0.0.1", keys: ["ssh-ed25519 synthetic-public-key"] })
      expect(invoke).not.toHaveBeenCalledWith("save_ssh_access", expect.anything())
      expect(store.getSnapshot().source?.sshAccess?.computers).toHaveLength(3)
      await store.applicationActions.saveSshAccess!({ ...request, computer: "dev", enabled: false })
      expect(store.getSnapshot().source?.sshAccess?.computers).toHaveLength(3)
      expect(store.getSnapshot().source?.sshAccess?.computers.find(item => item.computer === target)?.keys).toEqual(["ssh-ed25519 synthetic-public-key"])
    } finally { store.dispose() }
  })
  it("retains cached keys while a failed owner becomes unavailable and healthy owners remain writable", async () => {
    const { store, failOffice, failLocal } = fixture()
    try {
      await store.initialize(); await store.applicationActions.refreshSshAccess!()
      await store.applicationActions.saveSshAccess!({ ...request, keys: ["ssh-ed25519 retained-key"] })
      failOffice(); await store.applicationActions.refreshSshAccess!()
      const rows = store.getSnapshot().source?.sshAccess?.computers
      expect(rows?.find(item => item.computer === target)).toMatchObject({ enabled: true, keys: ["ssh-ed25519 retained-key"], unavailable: expect.stringContaining("Office Mac") })
      expect(rows?.find(item => item.computer === "dev")?.unavailable).toBeUndefined()
      expect(rows?.find(item => item.computer.startsWith("silo-remote:lab:"))?.unavailable).toBeUndefined()
      await expect(store.applicationActions.saveSshAccess!({ ...request, keys: [] })).rejects.toThrow("Refresh SSH status")
      await store.applicationActions.saveSshAccess!({ ...request, computer: "dev", enabled: false })
      failLocal(); await store.applicationActions.refreshSshAccess!()
      expect(store.getSnapshot().source?.sshAccess?.computers.find(item => item.computer === "dev")?.unavailable).toBeDefined()
      expect(store.getSnapshot().source?.sshAccess?.computers.find(item => item.computer.startsWith("silo-remote:lab:"))?.unavailable).toBeUndefined()
    } finally { store.dispose() }
  })
  it("explains that an older owner must update Silo instead of reconnecting", async () => {
    const { store, failOffice } = fixture()
    try {
      await store.initialize(); await store.applicationActions.refreshSshAccess!()
      failOffice({ code: "unsupported_remote_operation", message: "Update the owner." })
      await store.applicationActions.refreshSshAccess!()
      const row = store.getSnapshot().source?.sshAccess?.computers.find(item => item.computer === target)
      expect(row?.unavailable).toBe("Update Silo on Office Mac to manage SSH access. That version does not support remote SSH management.")
      expect(row?.unavailable).not.toContain("Reconnect")
      await expect(store.applicationActions.saveSshAccess!(request)).rejects.toThrow("Refresh SSH status")
      expect(store.getSnapshot().source?.sshAccess?.computers.find(item => item.computer === "dev")?.unavailable).toBeUndefined()
    } finally { store.dispose() }
  })
  it("rejects old read results even when polling starts during a pending revocation", async () => {
    const { store, delayOffice, delaySave } = fixture()
    let resolveRead!: (value: unknown) => void
    let resolveSave!: (value: unknown) => void
    try {
      await store.initialize(); await store.applicationActions.refreshSshAccess!()
      delaySave(new Promise(resolve => { resolveSave = resolve }))
      const saving = store.applicationActions.saveSshAccess!({ ...request, keys: [] })
      delayOffice(new Promise(resolve => { resolveRead = resolve }))
      const refreshing = store.applicationActions.refreshSshAccess!()
      resolveSave({ computers: [{ ...row, keys: [] }] })
      await saving
      resolveRead({ computers: [{ ...row, keys: ["ssh-ed25519 revoked-key"] }] })
      await refreshing
      expect(store.getSnapshot().source?.sshAccess?.computers.find(item => item.computer === target)?.keys).toEqual([])
    } finally { store.dispose() }
  })
  it("does not let a delayed remote refresh restore a revoked key", async () => {
    const { store, delayOffice, invoke } = fixture()
    let resolveRead!: (value: unknown) => void
    try {
      await store.initialize(); await store.applicationActions.refreshSshAccess!()
      delayOffice(new Promise(resolve => { resolveRead = resolve }))
      const refresh = store.applicationActions.refreshSshAccess!()
      await vi.waitFor(() => expect(invoke.mock.calls.filter(call => call[0] === "remote_ssh_access_state" && call[1]?.deviceId === "office")).toHaveLength(2))
      await store.applicationActions.saveSshAccess!({ ...request, keys: [] })
      resolveRead({ computers: [{ ...row, keys: ["ssh-ed25519 revoked-key"] }] })
      await refresh
      expect(store.getSnapshot().source?.sshAccess?.computers.find(item => item.computer === target)?.keys).toEqual([])
    } finally { store.dispose() }
  })
})

describe("retained log bridge", () => {
  it("passes opaque device and computer identities and rejects malformed pages", async () => {
    const mock = native()
    const invoke = nativeBridgeMock({ query_computer_logs: () => ({ entries: "not a log page" }) })
    const store = createProductionSource({ ...mock.bridge, invoke } as unknown as ProductionBridge)
    const request = { computerId: "computer-id", deviceId: "office-id", query: "old failure", limit: 200 }
    await expect(store.applicationActions.queryLogs!(request)).rejects.toThrow()
    expect(invoke).toHaveBeenCalledWith("query_computer_logs", { request })
    store.dispose()
  })
  it("distinguishes a canceled native export from a saved export and validates the reply", async () => {
    const mock = native()
    const invoke = nativeBridgeMock({ export_computer_logs: vi.fn().mockResolvedValueOnce(false).mockResolvedValueOnce(true).mockResolvedValueOnce("yes") })
    const store = createProductionSource({ ...mock.bridge, invoke } as ProductionBridge)
    const requests = [{ computerId: "computer-id", query: "failure" }]
    expect(await store.applicationActions.exportLogs!(requests)).toBe(false)
    expect(await store.applicationActions.exportLogs!(requests)).toBe(true)
    await expect(store.applicationActions.exportLogs!(requests)).rejects.toThrow()
    expect(invoke).toHaveBeenCalledWith("export_computer_logs", { requests })
    store.dispose()
  })
})

describe("operation queue bridge", () => {
  function queueBridge() {
    const handlers = new Map<string, () => void>()
    let queue: unknown = { running: [], waiting: [] }
    const invoke = nativeBridgeMock({
      ...initializationHandlers(),
      read_application_state: () => structuredClone(source),
      read_backup_state: () => structuredClone(backup),
      read_operation_queue: () => structuredClone(queue),
    })
    const listen = vi.fn(async (name: string, handler: () => void) => { handlers.set(name, handler); return () => handlers.delete(name) })
    return {
      bridge: { invoke, listen } as ProductionBridge,
      setQueue: (next: unknown) => { queue = next },
      emit: () => handlers.get("silo://operation-queue-changed")?.(),
    }
  }

  it("reads the queue on start and refetches when the change event fires", async () => {
    const mock = queueBridge()
    const store = createProductionSource(mock.bridge)
    try {
      await store.initialize()
      expect(mock.bridge.invoke).toHaveBeenCalledWith("read_operation_queue")
      expect(store.getSnapshot().source?.operationQueue).toEqual({ running: [], waiting: [] })

      mock.setQueue({
        running: [{ id: 1, label: "Backing up computers", computerId: null, computerName: null, sinceMs: 1000, cancellable: true, expectedMs: null, blockedByHidden: false }],
        waiting: [{ id: 2, label: "Restarting dev", kind: "lifecycle", computerId: "id-dev", computerName: "dev", sinceMs: 2000, cancellable: true, expectedMs: null, blockedByHidden: false }],
      })
      mock.emit()
      await vi.waitFor(() => expect(store.getSnapshot().source?.operationQueue?.waiting[0].label).toBe("Restarting dev"))
      expect(store.getSnapshot().source?.operationQueue?.running[0].computerId).toBeNull()
    } finally { store.dispose() }
  })

  it("keeps the last queue when a read fails", async () => {
    const mock = queueBridge()
    const store = createProductionSource(mock.bridge)
    try {
      await store.initialize()
      mock.setQueue({ running: [{ id: 1, label: "Backing up", computerId: null, computerName: null, sinceMs: 1, cancellable: true, expectedMs: null, blockedByHidden: false }], waiting: [] })
      mock.emit()
      await vi.waitFor(() => expect(store.getSnapshot().source?.operationQueue?.running).toHaveLength(1))
      mock.setQueue("not a queue")
      mock.emit()
      await new Promise((resolve) => setTimeout(resolve, 0))
      expect(store.getSnapshot().source?.operationQueue?.running).toHaveLength(1)
    } finally { store.dispose() }
  })
})


it.each(["failure", "success"] as const)("ignores a previous computer's late lifecycle %s after recreation", async (outcome) => {
  let finish!: (value: unknown) => void
  let reject!: (cause: unknown) => void
  const command = new Promise((resolve, fail) => { finish = resolve; reject = fail })
  let current = structuredClone(source)
  const mock = native({}, { read_application_state: () => structuredClone(current), computer_action: () => command })
  const store = createProductionSource(mock.bridge)
  try {
    await store.initialize()
    store.applicationActions.startComputer("dev")
    current.computers[0].configuration.id = "00000000-0000-4000-8000-000000000099"
    await store.refresh()
    const publishedIds: string[] = []
    store.subscribe(() => { publishedIds.push(store.getSnapshot().source!.computers[0].configuration.id) })
    if (outcome === "success") finish(structuredClone(source))
    else reject(new Error("old computer failed"))
    await vi.waitFor(() => expect(store.getSnapshot().source?.computers[0].lifecycleAction).toBeUndefined())
    expect(store.getSnapshot().source?.computers[0].configuration.id).toBe(current.computers[0].configuration.id)
    expect(store.getSnapshot().source?.computers[0].lifecycleFailure).toBeUndefined()
    expect(publishedIds).not.toContain(source.computers[0].configuration.id)
  } finally { store.dispose() }
})

it("a recreated computer does not inherit pending lifecycle state or action locks", async () => {
  const commands: Array<(value: unknown) => void> = []
  let current = structuredClone(source)
  const mock = native({}, {
    read_application_state: () => structuredClone(current),
    computer_action: () => new Promise(resolve => { commands.push(resolve) }),
  })
  const store = createProductionSource(mock.bridge)
  try {
    await store.initialize()
    store.applicationActions.startComputer("dev")
    current.computers[0].configuration.id = "00000000-0000-4000-8000-000000000099"
    await store.refresh()
    expect(store.getSnapshot().source?.computers[0].lifecycleAction).toBeUndefined()
    store.applicationActions.startComputer("dev")
    expect(commands).toHaveLength(2)
    commands[0](structuredClone(source))
    await new Promise(resolve => setTimeout(resolve, 0))
    expect(store.getSnapshot().source?.computers[0].lifecycleAction).toBe("start")
    commands[1](current)
    await vi.waitFor(() => expect(store.getSnapshot().source?.computers[0].lifecycleAction).toBeUndefined())
  } finally { store.dispose() }
})
