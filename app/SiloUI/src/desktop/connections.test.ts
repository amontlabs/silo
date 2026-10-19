import { afterEach, describe, expect, it, vi } from "vitest"
import { applicationSourceForScenario } from "@/fixtures/application-scenarios"
import { remoteComputerTarget } from "@/features/application/model/connections"
import { createProductionSource, type ProductionBridge } from "./production-source"

import { assertNativeBridgeMocksHandled, nativeBridgeMock } from "@/test/native-bridge-mock"

afterEach(() => {
  assertNativeBridgeMocksHandled()
  vi.useRealTimers()
})

const pushTarget = { repository: "owner/repo", branch: "main", commit: "a".repeat(40) }

function initializationHandlers() {
  return {
    read_backup_state: () => ({ snapshotId: "remote-fixture", availability: "available", requiredSpaceGB: 0, availableSpaceGB: 40, archives: [], operation: null }),
    read_operation_queue: () => ({ running: [], waiting: [] }),
  }
}

describe("remote device ownership", () => {
  it("keeps same-name computers distinct and directs a remote lifecycle action to its owner", async () => {
    const local = applicationSourceForScenario("running")
    const remote = structuredClone(local)
    remote.computers = [remote.computers[0]]
    local.computers[0].logs = [{ line: "Local computer log", occurredAt: "now" }]
    remote.computers[0].logs = [{ line: "Remote computer log", occurredAt: "now" }]
    const device = { id: "office", name: "Office Mac", address: "developer@office" }
    const invoke = nativeBridgeMock({
      ...initializationHandlers(),
      read_application_state: () => local,
      device_list: () => [device],
      device_snapshot: () => remote,
      remote_computer_action: () => ({ ...remote, computers: remote.computers.map(computer => ({ ...computer, logs: [] })) }),
      connections_status: () => ({ enabled: false, deviceId: "local", name: "Laptop", address: "developer@laptop" }),
      read_setup_activity: () => [],
      read_network_state: () => ({ computers: [] }),
    })
    const store = createProductionSource({ invoke, listen: async () => () => {} } as ProductionBridge)
    try {
      await store.initialize()
      const target = remoteComputerTarget(device.id, remote.computers[0].configuration.id)
      const names = store.getSnapshot().source!.computers.filter(computer => computer.configuration.name === remote.computers[0].configuration.name)
      expect(names).toHaveLength(2)
      expect(names[0].configuration.id).not.toBe(names[1].configuration.id)
      const observedRemoteLogs: string[] = []
      const unsubscribe = store.subscribe(() => observedRemoteLogs.push(...(store.getSnapshot().source?.computers.find(computer => computer.device)?.logs.map(log => log.line) ?? [])))
      store.applicationActions.stopComputer(target)
      await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith("remote_computer_action", { deviceId: "office", computerId: remote.computers[0].configuration.id, action: "stop", name: remote.computers[0].configuration.name }))
      expect(invoke).not.toHaveBeenCalledWith("computer_action", expect.anything())
      await vi.waitFor(() => expect(store.getSnapshot().source?.computers).toHaveLength(local.computers.length + 1))
      expect(observedRemoteLogs).not.toContain("Local computer log")
      unsubscribe()
    } finally { store.dispose() }
  })

  it("strips UI-qualified identities from optimistic remote edit and deletion requests", async () => {
    const local = applicationSourceForScenario("running")
    const configuration = local.computers[0].configuration
    const invoke = nativeBridgeMock({
      remote_upsert_computer: () => local,
      remote_delete_computer: () => local,
      device_list: () => [],
      connections_status: () => ({ enabled: false, deviceId: "local", name: "Laptop", address: "developer@laptop" }),
    })
    const store = createProductionSource({ invoke, listen: async () => () => {} } as unknown as ProductionBridge)
    const displayed = { ...configuration, id: remoteComputerTarget("office", configuration.id) }
    try {
      await store.applicationActions.saveRemoteComputer!("office", displayed, displayed)
      expect(invoke).toHaveBeenCalledWith("remote_upsert_computer", { deviceId: "office", configuration, expected: configuration })
      await store.applicationActions.deleteRemoteComputer!("office", displayed)
      expect(invoke).toHaveBeenCalledWith("remote_delete_computer", { deviceId: "office", computerId: configuration.id, expected: configuration })
    } finally { store.dispose() }
  })
})

it("keeps local state fresh after remote lifecycle failure and launches editors on the controlling device", async () => {
  vi.useFakeTimers()
  const local = applicationSourceForScenario("running")
  const remote = structuredClone(local)
  remote.computers = [remote.computers[0]]
  const device = { id: "office", name: "Office Mac", address: "user@office" }
  const target = remoteComputerTarget(device.id, remote.computers[0].configuration.id)
  let disconnected = false
  const invoke = nativeBridgeMock({
      ...initializationHandlers(),
      read_application_state: () => local,
      device_list: () => [device],
      device_snapshot: async () => { if (disconnected) throw new Error("Connection lost"); return remote },
      remote_computer_action: async () => { disconnected = true; throw new Error("Connection lost") },
      computer_action: () => local,
      connections_status: () => ({ enabled: false, deviceId: "local", name: "Laptop", address: "user@laptop" }),
      read_network_state: () => ({ computers: [] }),
      read_setup_activity: () => [],
    })
  const store = createProductionSource({ invoke, listen: async () => () => {} } as ProductionBridge)
  try {
    await store.initialize()
    store.applicationActions.openEditor(target, "/workspace/project")
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith("computer_action", { action: "open-editor", name: target, path: "/workspace/project" }))
    await vi.advanceTimersByTimeAsync(0)
    store.applicationActions.stopComputer(target)
    await vi.waitFor(() => expect(store.getSnapshot().source!.computers.find(computer => computer.device)?.freshness).toBe("stale"))
    expect(store.getSnapshot().source!.computers.filter(computer => !computer.device).every(computer => computer.freshness === "fresh")).toBe(true)
    expect(store.getSnapshot().source!.computerOperationsUnavailable).toBeUndefined()
  } finally { store.dispose() }
})

it("uses qualified remote port mappings and isolates reachability after a failed local network refresh", async () => {
  const local = applicationSourceForScenario("running")
  const remote = structuredClone(local)
  remote.computers = [remote.computers[0]]
  const target = remoteComputerTarget("office", remote.computers[0].configuration.id)
  let networkFailure = false
  const invoke = nativeBridgeMock({
      ...initializationHandlers(),
      read_application_state: () => local,
      device_list: () => [{ id: "office", name: "Office Mac", address: "user@office" }],
      device_snapshot: () => remote,
      connections_status: () => ({ enabled: false, deviceId: "local", name: "Laptop", address: "user@laptop" }),
      read_network_state: async () => {
      if (networkFailure) throw new Error("Network check failed")
      return { computers: [{ computer: local.computers[0].configuration.name, error: null, ports: [{ port: 3000, hostPort: 3000, scheme: "http", configured: true, state: "reachable" }] }] }
    },
      remote_network_state: () => ({ computers: [{ computer: target, error: null, ports: [{ port: 3000, hostPort: 43000, scheme: "http", configured: true, state: "reachable" }] }] }),
      remote_save_network_port: () => ({ computers: [{ computer: target, error: null, ports: [{ port: 3000, hostPort: 43000, scheme: "http", configured: true, state: "reachable" }] }] }),
      remote_remove_network_port: () => ({ computers: [{ computer: target, error: null, ports: [{ port: 3000, hostPort: 43000, scheme: "http", configured: true, state: "reachable" }] }] }),
      read_setup_activity: () => [],
    })
  const store = createProductionSource({ invoke, listen: async () => () => {} } as ProductionBridge)
  try {
    await store.initialize()
    await store.applicationActions.refreshNetwork!()
    expect(store.getSnapshot().source!.computers.find(computer => computer.device)?.ports).toEqual([{ port: 3000, hostPort: 43000, scheme: "http", configured: true, listening: true }])
    expect(invoke).toHaveBeenCalledWith("remote_network_state", { deviceId: "office" })
    expect(store.getSnapshot().source!.computers.find(computer => !computer.device)?.ports[0].hostPort).toBe(3000)
    await store.applicationActions.saveNetworkPort!({ computer: target, port: 3000, hostPort: 43000, scheme: "http" })
    expect(invoke).toHaveBeenCalledWith("remote_save_network_port", { deviceId: "office", computerId: remote.computers[0].configuration.id, port: 3000, hostPort: 43000, scheme: "http" })
    expect(store.getSnapshot().source!.network!.computers.map(row => row.computer)).toEqual([local.computers[0].configuration.name, target])
    await store.applicationActions.removeNetworkPort!(target, 3000)
    expect(invoke).toHaveBeenCalledWith("remote_remove_network_port", { deviceId: "office", computerId: remote.computers[0].configuration.id, port: 3000 })
    networkFailure = true
    await store.applicationActions.refreshNetwork!()
    expect(store.getSnapshot().source!.computers.find(computer => computer.device)?.ports[0].listening).toBe(true)
  } finally { store.dispose() }
})

it("keeps a reachable device connected while its computer configuration is busy", async () => {
  const local = applicationSourceForScenario("running")
  let busy = false
  const invoke = nativeBridgeMock({
      ...initializationHandlers(),
      read_application_state: () => local,
      device_list: () => [{ id: "office", name: "Office Mac", address: "user@office" }],
      device_snapshot: async () => {
      if (busy) throw { code: "update_in_progress", message: "Please wait for configuration." }
      return local
    },
      connections_status: () => ({ enabled: false, deviceId: "local", name: "Laptop", address: "user@laptop" }),
      read_network_state: () => ({ computers: [] }),
      remote_network_state: () => ({ computers: [] }),
      read_setup_activity: () => [],
    })
  const store = createProductionSource({ invoke, listen: async () => () => {} } as ProductionBridge)
  try {
    await store.initialize()
    busy = true
    await store.refresh()
    await vi.waitFor(() => expect(store.getSnapshot().source!.devices![0].busy).toBe(true))
    const computer = store.getSnapshot().source!.computers.find(computer => computer.device)!
    expect(computer.device!.connected).toBe(true)
    expect(computer.freshness).toBe("stale")
    expect(computer.stateDetail).toBe("Updating…")
  } finally { store.dispose() }
})

it("preserves connected remote computers when later local state reads fail", async () => {
  const local = applicationSourceForScenario("running")
  let failLocal = false
  const invoke = nativeBridgeMock({
      ...initializationHandlers(),
      read_application_state: async () => { if (failLocal) throw new Error("Local runtime unavailable"); return local },
      device_list: () => [{ id: "office", name: "Office Mac", address: "user@office" }],
      device_snapshot: () => local,
      remote_computer_action: () => local,
      connections_status: () => ({ enabled: false, deviceId: "local", name: "Laptop", address: "user@laptop" }),
      read_network_state: () => ({ computers: [] }),
      remote_network_state: () => ({ computers: [] }),
      read_setup_activity: () => [],
    })
  const store = createProductionSource({ invoke, listen: async () => () => {} } as ProductionBridge)
  try {
    await store.initialize()
    failLocal = true
    await store.refresh()
    const source = store.getSnapshot().source!
    expect(source.computerOperationsUnavailable).toContain("Local runtime unavailable")
    expect(source.computers.find(computer => !computer.device)?.freshness).toBe("stale")
    const remote = source.computers.find(computer => computer.device)!
    expect(remote.freshness).toBe("fresh")
    store.applicationActions.stopComputer(remote.configuration.id)
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith("remote_computer_action", { deviceId: "office", computerId: local.computers[0].configuration.id, action: "stop", name: expect.anything() }))
  } finally { store.dispose() }
})

it("uses native local metadata to display verified remote computers when local runtime fails at startup", async () => {
  const remote = applicationSourceForScenario("running")
  const shell = { ...remote, computers: [], runtimeRepair: { status: "unavailable", reason: "Local runtime unavailable" } }
  const invoke = nativeBridgeMock({
      ...initializationHandlers(),
      read_application_state: async () => { throw new Error("Local runtime unavailable") },
      read_application_shell: () => shell,
      device_list: () => [{ id: "office", name: "Office Mac", address: "user@office" }],
      device_snapshot: () => remote,
      connections_status: () => ({ enabled: false, deviceId: "local", name: "Laptop", address: "user@laptop" }),
      read_network_state: () => ({ computers: [] }),
      remote_network_state: () => ({ computers: [] }),
      read_setup_activity: () => [],
    })
  const store = createProductionSource({ invoke, listen: async () => () => {} } as ProductionBridge)
  try {
    await store.initialize()
    await vi.waitFor(() => expect(store.getSnapshot().source?.computers).toHaveLength(remote.computers.length))
    expect(invoke).toHaveBeenCalledWith("read_application_shell", { error: "Silo could not read application state: Local runtime unavailable" })
    expect(store.getSnapshot().source!.computers.every(computer => computer.device?.id === "office" && computer.freshness === "fresh")).toBe(true)
    expect(store.getSnapshot().source!.runtimeRepair?.reason).toBe("Local runtime unavailable")
  } finally { store.dispose() }
})

it("merges remote repository results and activity idempotently without same-name collisions", async () => {
  const local = applicationSourceForScenario("running")
  const remote = structuredClone(local)
  remote.computers = [remote.computers[0]]
  const name = local.computers[0].configuration.name
  const target = remoteComputerTarget("office", remote.computers[0].configuration.id)
  local.repositoryPushOperations = [{ computer: name, repositoryPath: "/workspace/repo", commitCount: 1, status: "succeeded" }]
  remote.repositoryPushOperations = [{ computer: name, repositoryPath: "/workspace/repo", commitCount: 2, status: "failed", message: "Remote push failed" }]
  remote.activities = [{ ...local.activities[0], computer: name, title: "Remote start" }]
  remote.github.account = "remote-owner"
  remote.secrets = []
  const invoke = nativeBridgeMock({
      ...initializationHandlers(),
      read_application_state: () => local,
      device_list: () => [{ id: "office", name: "Office Mac", address: "user@office" }],
      device_snapshot: () => remote,
      start_repository_push: () => remote.repositoryPushOperations![0],
      connections_status: () => ({ enabled: false, deviceId: "local", name: "Laptop", address: "user@laptop" }),
      read_network_state: () => ({ computers: [] }),
      remote_network_state: () => ({ computers: [] }),
      read_setup_activity: () => [],
    })
  const store = createProductionSource({ invoke, listen: async () => () => {} } as ProductionBridge)
  try {
    await store.initialize()
    await store.applicationActions.refreshNetwork!()
    await store.applicationActions.refreshNetwork!()
    const source = store.getSnapshot().source!
    expect(source.repositoryPushOperations).toEqual([local.repositoryPushOperations[0], { ...remote.repositoryPushOperations[0], computer: target }])
    expect(source.activities).toHaveLength(local.activities.length + 1)
    expect(new Set(source.activities.map(activity => activity.id)).size).toBe(source.activities.length)
    expect(source.activities.find(activity => activity.title === "Remote start")).toMatchObject({ computer: target, detail: expect.stringContaining("Office Mac:") })
    expect(source.github).toMatchObject(local.github)
    expect(source.secrets).toEqual(local.secrets)
    store.applicationActions.pushRepository(target, "/workspace/repo", pushTarget)
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith("start_repository_push", { computer: target, repositoryPath: "/workspace/repo", operationId: expect.any(String), target: pushTarget }))
  } finally { store.dispose() }
})

it.each(["succeeded", "failed"] as const)("keeps a remote push loading across refreshes until its %s result arrives", async status => {
  const local = applicationSourceForScenario("running")
  const remote = structuredClone(local)
  remote.computers = [remote.computers[0]]
  const computer = remoteComputerTarget("office", remote.computers[0].configuration.id)
  const repositoryPath = remote.computers[0].repositories[0].path
  let finishPush!: (result: unknown) => void
  const pending = new Promise(resolve => { finishPush = resolve })
  let finishStaleRead: ((result: unknown) => void) | undefined
  let holdRemoteRead = false
  const invoke = nativeBridgeMock({
      ...initializationHandlers(),
      read_application_state: () => local,
      device_list: () => [{ id: "office", name: "Office Mac", address: "user@office" }],
      device_snapshot: () => holdRemoteRead ? new Promise(resolve => { finishStaleRead = resolve }) : remote,
      connections_status: () => ({ enabled: false, deviceId: "local", name: "Laptop", address: "user@laptop" }),
      read_network_state: () => ({ computers: [] }),
      remote_network_state: () => ({ computers: [] }),
      read_setup_activity: () => [],
      start_repository_push: () => pending,
    })
  const store = createProductionSource({ invoke, listen: async () => () => {} } as ProductionBridge)
  try {
    await store.initialize()
    store.applicationActions.pushRepository(computer, repositoryPath, pushTarget)
    const pushing = { computer, repositoryPath, commitCount: remote.computers[0].repositories[0].ahead, status: "pushing" }
    expect(store.getSnapshot().source!.repositoryPushOperations).toContainEqual(pushing)
    const remoteReads = invoke.mock.calls.filter(([command]) => command === "device_snapshot").length
    await store.refresh()
    await vi.waitFor(() => expect(invoke.mock.calls.filter(([command]) => command === "device_snapshot").length).toBeGreaterThan(remoteReads))
    await store.applicationActions.refreshNetwork!()
    expect(store.getSnapshot().source!.repositoryPushOperations).toContainEqual(pushing)
    store.applicationActions.pushRepository(computer, repositoryPath, pushTarget)
    expect(invoke.mock.calls.filter(([command]) => command === "start_repository_push")).toHaveLength(1)
    holdRemoteRead = true
    await store.refresh()
    await vi.waitFor(() => expect(finishStaleRead).toBeDefined())
    const staleRemote = structuredClone(remote)
    const result = { computer: remote.computers[0].configuration.name, repositoryPath, commitCount: 2, ...(status === "failed" ? { status, message: "Missing LFS object" } : { status }) }
    remote.repositoryPushOperations = [result]
    finishPush(result)
    await vi.waitFor(() => expect(store.getSnapshot().source!.repositoryPushOperations).toContainEqual({ ...result, computer }))
    holdRemoteRead = false
    finishStaleRead!(staleRemote)
    await store.applicationActions.refreshNetwork!()
    expect(store.getSnapshot().source!.repositoryPushOperations).toContainEqual({ ...result, computer })
  } finally { store.dispose() }
})

it.each([true, false])("reconciles a lost start reply without another push (host accepted: %s)", async accepted => {
  vi.useFakeTimers()
  const local = applicationSourceForScenario("running")
  const remote = structuredClone(local)
  remote.computers = [remote.computers[0]]
  const computer = remoteComputerTarget("office", remote.computers[0].configuration.id)
  const repositoryPath = remote.computers[0].repositories[0].path
  let attempts = 0
  const success = { computer: remote.computers[0].configuration.name, repositoryPath, status: "succeeded" as const, commitCount: 2 }
  const invoke = nativeBridgeMock({
      ...initializationHandlers(),
      read_application_state: () => local,
      device_list: () => [{ id: "office", name: "Office Mac", address: "user@office" }],
      device_snapshot: () => remote,
      connections_status: () => ({ enabled: false, deviceId: "local", name: "Laptop", address: "user@laptop" }),
      read_network_state: () => ({ computers: [] }),
      remote_network_state: () => ({ computers: [] }),
      read_setup_activity: () => [],
      start_repository_push: async () => {
      if (++attempts === 1) throw new Error("SSH connection closed")
      remote.repositoryPushOperations = [success]
      return success
    },
      repository_push_status: async () => {
      if (accepted) remote.repositoryPushOperations = [success]
      return accepted ? success : null
    },
    })
  const store = createProductionSource({ invoke, listen: async () => () => {} } as ProductionBridge)
  try {
    await store.initialize()
    store.applicationActions.pushRepository(computer, repositoryPath, pushTarget)
    await vi.waitFor(() => expect(store.getSnapshot().source!.repositoryPushOperations).toContainEqual(expect.objectContaining({ status: "pushing", message: expect.stringContaining("Waiting for push status") })))
    store.applicationActions.pushRepository(computer, repositoryPath, pushTarget)
    expect(attempts).toBe(1)
    // A lost reply schedules the first retry after the 4 s failure backoff.
    await vi.advanceTimersByTimeAsync(4_000)
    expect(store.getSnapshot().source!.repositoryPushOperations).toContainEqual({ computer, repositoryPath, status: "succeeded", commitCount: 2 })
    expect(attempts).toBe(accepted ? 1 : 2)
    const calls = invoke.mock.calls as unknown as Array<[string, { operationId?: string }]>
    const requestIds = calls.filter(([command]) => command === "start_repository_push" || command === "repository_push_status").map(([, args]) => args.operationId)
    expect(new Set(requestIds).size).toBe(1)
    expect(requestIds[0]).toEqual(expect.any(String))
  } finally { store.dispose() }
})


it("backs off failed device snapshots independently and refreshes immediately on focus", async () => {
  vi.useFakeTimers()
  const local = applicationSourceForScenario("running")
  let failing = false
  const invoke = nativeBridgeMock({
    ...initializationHandlers(),
    read_application_state: () => local,
    read_setup_activity: () => [],
    device_list: () => ["broken", "healthy"].map(id => ({ id, name: id, address: `user@${id}` })),
    device_snapshot: args => {
      if (failing && args?.deviceId === "broken") throw new Error("Connection lost")
      return local
    },
    connections_status: () => ({ enabled: false, deviceId: "local", name: "Laptop", address: "user@laptop" }),
    read_network_state: () => ({ computers: [] }),
    remote_network_state: () => ({ computers: [] }),
  })
  const store = createProductionSource({ invoke, listen: async () => () => {} } as ProductionBridge)
  const reads = (id: string) => invoke.mock.calls.filter(([command, args]) => command === "device_snapshot" && args?.deviceId === id).length
  try {
    await store.initialize()
    const initial = reads("broken")
    failing = true
    await vi.advanceTimersByTimeAsync(10_000)
    let failedReads = initial + 1
    expect(reads("broken")).toBe(failedReads)
    for (const delay of [20_000, 40_000, 60_000, 60_000]) {
      const healthy = reads("healthy")
      await vi.advanceTimersByTimeAsync(delay - 1)
      expect(reads("broken")).toBe(failedReads)
      await vi.advanceTimersByTimeAsync(1)
      expect(reads("broken")).toBe(++failedReads)
      expect(reads("healthy")).toBe(healthy + delay / 10_000)
    }
    failing = false
    await vi.advanceTimersByTimeAsync(2_000)
    window.dispatchEvent(new Event("focus"))
    await vi.advanceTimersByTimeAsync(0)
    expect(reads("broken")).toBe(++failedReads)
    expect(store.getSnapshot().source!.devices!.find(device => device.id === "broken")?.connected).toBe(true)
    await vi.advanceTimersByTimeAsync(10_000)
    expect(reads("broken")).toBe(++failedReads)
    store.dispose()
    await vi.advanceTimersByTimeAsync(60_000)
    expect(reads("broken")).toBe(failedReads)
  } finally { store.dispose() }
})


it("backs off failed device-list polling while local state stays fresh", async () => {
  vi.useFakeTimers()
  const local = applicationSourceForScenario("running")
  let failing = false
  const invoke = nativeBridgeMock({
    ...initializationHandlers(),
    read_application_state: () => local,
    read_setup_activity: () => [],
    device_list: () => { if (failing) throw new Error("Unreadable list"); return [] },
    connections_status: () => ({ enabled: false, deviceId: "local", name: "Laptop", address: "user@laptop" }),
    read_network_state: () => ({ computers: [] }),
  })
  const store = createProductionSource({ invoke, listen: async () => () => {} } as ProductionBridge)
  const reads = (command: string) => invoke.mock.calls.filter(([name]) => name === command).length
  try {
    await store.initialize()
    failing = true
    await vi.advanceTimersByTimeAsync(10_000)
    let calls = reads("device_list")
    expect(store.getSnapshot().source!.devicesError).toContain("Unreadable list")
    for (const delay of [20_000, 40_000, 60_000, 60_000]) {
      const localReads = reads("read_application_state")
      await vi.advanceTimersByTimeAsync(delay - 1)
      expect(reads("device_list")).toBe(calls)
      await vi.advanceTimersByTimeAsync(1)
      expect(reads("device_list")).toBe(++calls)
      expect(reads("read_application_state")).toBe(localReads + delay / 10_000)
    }
    failing = false
    await vi.advanceTimersByTimeAsync(2_000)
    window.dispatchEvent(new Event("focus"))
    await vi.advanceTimersByTimeAsync(0)
    expect(reads("device_list")).toBe(++calls)
    expect(store.getSnapshot().source!.devicesError).toBeUndefined()
    await vi.advanceTimersByTimeAsync(10_000)
    expect(reads("device_list")).toBe(++calls)
  } finally { store.dispose() }
})
