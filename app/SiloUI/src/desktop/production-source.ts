import { isLifecycleStep, type LifecycleStep } from "@/features/application/model/lifecycle-progress"
import { workspaceStorageStateSchema } from "@/features/application/model/workspace-storage"
import { logPageSchema } from "@/features/application/model/logs"
import { invoke } from "@tauri-apps/api/core"
import { listen } from "@tauri-apps/api/event"
import { useMemo, useSyncExternalStore } from "react"
import { z } from "zod"
import { showOperationFailure } from "@/lib/operation-toast"
import { directoryPageShape, githubStateShape, parseApplicationSource, parseBackupState, parseRemoteApplicationSource, secretShape } from "./production-schemas"
export { parseApplicationSource, parseBackupState, parseNetworkState, parseRemoteApplicationSource, parseSshAccessState } from "./production-schemas"

import { siloProgressEventSchema, setupComputerConfigurationSchema, type SetupComputerConfiguration, type SiloProgressEvent, type SetupComputerConfigurationRequest, type SetupQueueItemID } from "@/contracts/silo"
import type { OnboardingCompletionRequest } from "@/features/onboarding/model/onboarding-source"
import type { ApplicationActions, ApplicationSource, ApplicationComputer, SecretConfigurationRequest } from "@/features/application/model/application-source"
import { operationQueueSchema, isCancelledError, type OperationQueue } from "@/features/application/model/operation-queue"
import { deriveComputerChanges, isStaleConfigurationError, type ComputerConfigurationChange } from "@/features/application/model/computer-change"
import type { BackupController, BackupState } from "@/features/application/model/backup-source"
import { checkpointUsageSchema, type ComputerCheckpointOperation } from "@/features/application/model/checkpoint-source"
import type { StatusBarActions, StatusBarRoute } from "@/features/status-bar/status-bar-types"

import { downloadOutcomeSchema, transferProgressEvent, transferProgressSchema, uploadOutcomeSchema, uploadSelectionSchema } from "@/features/application/model/file-transfer"
import { deviceSchema, connectionsStatusSchema, remoteComputerTarget, parseRemoteComputerTarget, computerTarget, type Device, type ConnectionsStatus } from "@/features/application/model/connections"

import type { ProductionContext } from "./production-context"
import { createBackupControls } from "./production-backup"
import { createDeviceServices } from "./production-services"
import { createSetupQueue } from "./production-setup-queue"
import type { EventHandler, ListedDevice, ProductionBridge, ProductionSnapshot } from "./production-types"
import { PUSH_STATUS_ATTEMPTS, PUSH_STATUS_INTERVAL_MS, PUSH_STATUS_MAX_INTERVAL_MS, REFRESH_GATE_TIMEOUT_MS, REMOTE_READ_WAIT_MS, RETURN_REFRESH_MIN_AGE_MS, canonicalKey, derivePorts, errorMessage, isUpdateInProgress, lastCancelledLifecycle, pushKey, reportedCancellation, shareStructure, unavailableBackup } from "./production-helpers"

export type { ProductionBridge, ProductionSnapshot } from "./production-types"
export { isUpdateInProgress, shareStructure } from "./production-helpers"

const bridge: ProductionBridge = {
  invoke: (command, arguments_) => invoke(command, arguments_),
  listen: (event, handler) => listen(event, handler),
}

export const localUpdatingNotice = "Computers on this device are updating. They appear here when the update finishes."

export function createProductionSource(native: ProductionBridge = bridge) {
  let snapshot: ProductionSnapshot = {
    setupQueue: ["computerRun", "computerVerify", "identityRun", "identityVerify", "githubRun", "githubVerify", "completion"].map((id) => ({ id: id as SetupQueueItemID, status: "idle" })),
    setupEvents: [],
    source: null,
    backup: unavailableBackup("Export and import state has not loaded. No computer data changed."),
    loading: true,
    error: null,
  }
  let view = snapshot
  let lastComputerJob: { key: string; promise: Promise<ApplicationSource> } | undefined
  let identityVerificationSequence = 0
  let lastVerificationKey: string | undefined
  let lastGitHubJob: { key: string; promise: Promise<void> } | undefined
  let lastIdentityJob: { key: string; promise: Promise<void> } | undefined
  let activeConfiguration: ApplicationSource["computerConfigurationOperation"] = null
  let activeRequestId: string | null = null
  let operationSequence = 0
  let devices: Device[] = []
  let connections: ConnectionsStatus | undefined
  let connectionsError: string | undefined
  let connectionsSequence = 0
  let connectionsSaveSequence = 0
  let devicesError: string | undefined
  const remoteSnapshots = new Map<string, ApplicationSource>()
  let knownDevices: ListedDevice[] = []
  let remoteListRevision = 0
  let remoteListRead: Promise<boolean> | undefined
  let remoteListFailureDelay = 0
  let remoteListNextRead = 0
  let remotePasses = 0
  const remoteRevisions = new Map<string, number>()
  const remoteReads = new Map<string, { repositories: boolean; promise: Promise<void> }>()
  const remoteRepositoryReads = new Set<string>()
  const slowDevices = new Set<string>()
  const remoteFailures = new Map<string, { delay: number; nextRead: number }>()
  let remoteTimer: ReturnType<typeof setInterval> | undefined
  let operationQueue: OperationQueue | undefined
  let operationQueueRequest: Promise<void> | undefined
  let operationQueueDirty = false
  let disposed = false
  let activeRefreshes = 0
  let lastRefreshFinishedAt = 0
  /** Bumped whenever newer state is published outside a read: reads started earlier are dropped. */
  let refreshSequence = 0
  let readSequence = 0
  let appliedApplicationRead = 0
  const appliedComputerReads = new Map<string, number>()
  let appliedBackupRead = 0
  let refreshRepositoriesOnReturn = false
  let githubMutationSequence = 0
  let githubMutationPending = false
  let secretMutationSequence = 0
  let appliedSecretMutation = 0
  const unlisten: Array<() => void> = []
  const listeners = new Set<() => void>()
  const pendingComputerActions = new Set<string>()
  const pendingCheckpointOperations = new Map<string, ComputerCheckpointOperation>()
  const pushPollTimers = new Set<ReturnType<typeof setTimeout>>()
  const pendingRepositoryPushes = new Map<string, ApplicationSource["repositoryPushOperations"][number]>()
  /** The push whose status polling currently owns each repository key. */
  const activePushes = new Map<string, object>()
  /** Pushes whose status could not be confirmed; shown as "unknown" until dismissed or reported by their host. */
  const unconfirmedPushes = new Map<string, ApplicationSource["repositoryPushOperations"][number]>()
  const pendingLifecycle = new Map<string, "start" | "stop" | "restart" | "dismiss-error">()
  const lifecycleSteps = new Map<string, LifecycleStep>()
  const computerFailures = new Map<string, { computerId: string; action: string; message: string; cancelled: boolean }>()
  const context: ProductionContext = { native, snapshot: () => snapshot, publish: next => publish(next), disposed: () => disposed }
  const setup = createSetupQueue(context)
  const services = createDeviceServices({ ...context, devices: () => devices, remoteSnapshots, localName: () => connections?.name, live: () => live })
  const backups = createBackupControls({ ...context, view: () => view, bumpRefreshSequence: () => { ++refreshSequence }, readSequence: () => readSequence, refresh: () => refresh(), reportActionFailure, reportUnavailable })
  const { enqueue: enqueueSetup, setStatus: setSetupStatus, setJobStatus, recordGitHubActivity, delay: setupDelay, drain: drainSetup } = setup

  function refreshOperationQueue(): Promise<void> {
    if (operationQueueRequest) { operationQueueDirty = true; return operationQueueRequest }
    operationQueueRequest = (async () => { do {
      operationQueueDirty = false
      try {
        const next = operationQueueSchema.parse(await native.invoke("read_operation_queue"))
        if (disposed) return
        operationQueue = next
        publish({ ...snapshot })
      } catch {
        // A read failure leaves the last queue in place; a later event refetches.
      }
    } while (operationQueueDirty && !disposed) })().finally(() => { operationQueueRequest = undefined })
    return operationQueueRequest
  }

  async function changeSecret(command: string, arguments_: Record<string, unknown>) {
    const sequence = ++secretMutationSequence
    const secrets = z.array(secretShape).parse(await native.invoke(command, arguments_))
    if (disposed) return
    // An older whole-list reply cannot undo a later successful secret mutation.
    if (sequence > appliedSecretMutation) {
      appliedSecretMutation = sequence
      ++refreshSequence
      if (snapshot.source) publish({ ...snapshot, source: { ...snapshot.source, secrets } })
    }
    refreshFromEvent()
  }

  // `snapshot` is the base state: the local application source as last read or
  // returned by a mutation, without frontend overlays. `view` is what subscribers
  // see: the base plus remote devices, network ports, and pending, failed and
  // unconfirmed actions. It is derived once per publish and never written back,
  // so an overlay disappears as soon as its reason does.
  function publish(next: ProductionSnapshot) {
    if (disposed) return
    snapshot = next
    // Native reads return fresh objects even when the state is unchanged: rows that did not
    // change keep their identity, and a view with no changed row is not published.
    const nextView = shareStructure(view, derive(next))
    if (nextView === view) return
    view = nextView
    listeners.forEach((listener) => listener())
  }

  function withPendingCheckpoint(computer: ApplicationComputer, target: string): ApplicationComputer {
    const pending = pendingCheckpointOperations.get(target)
    return pending ? { ...computer, checkpointOperation: computer.checkpointOperation?.status === "running" ? computer.checkpointOperation : pending } : computer
  }

  function derive(base: ProductionSnapshot): ProductionSnapshot {
    const next = { ...base, backup: backups.derive(base.backup) }
    if (!base.source) return next
    const networkRows = new Map((services.network()?.computers ?? []).map(row => [row.computer, row]))
    const computers = base.source.computers.filter(computer => !computer.device).map(computer => ({
      ...withPendingCheckpoint(computer, computer.configuration.name),
      ports: derivePorts(networkRows.get(computer.configuration.name), true),
    }))
    let pushes = base.source.repositoryPushOperations.filter(push => !parseRemoteComputerTarget(push.computer))
    const activities = base.source.activities.filter(activity => !activity.id.startsWith("silo-remote-activity:"))
    for (const device of devices) {
      const owner = remoteSnapshots.get(device.id)
      if (!owner) continue
      const vms = new Map(owner.computers.map(computer => [computer.configuration.name, computer]))
      const slow = slowDevices.has(device.id)
      for (const computer of vms.values()) {
        const target = remoteComputerTarget(device.id, computer.configuration.id)
        computers.push({
          ...withPendingCheckpoint(computer, target),
          configuration: { ...computer.configuration, id: target },
          device: { ...device, computerId: computer.configuration.id },
          ports: derivePorts(networkRows.get(target), device.connected),
          freshness: device.connected && !device.busy && !slow ? computer.freshness : "stale",
          stateDetail: device.busy || (device.connected && slow) ? "Updating…" : device.connected ? computer.stateDetail : "Offline · last known status",
        })
      }
      for (const push of owner.repositoryPushOperations) {
        const computer = vms.get(push.computer)
        if (computer) pushes.push({ ...push, computer: remoteComputerTarget(device.id, computer.configuration.id) })
      }
      for (const activity of owner.activities) {
        activities.push({ ...activity,
          id: `silo-remote-activity:${encodeURIComponent(device.id)}:${encodeURIComponent(activity.id)}`,
          detail: `${device.name}: ${activity.detail}`,
          computer: activity.computer ? remoteComputerTarget(device.id, vms.get(activity.computer)?.configuration.id ?? activity.computer) : undefined,
        })
      }
    }
    if (pendingRepositoryPushes.size || unconfirmedPushes.size) {
      // A push whose computer (or device) is gone has nothing left to show or poll.
      const targets = new Set(computers.map(computerTarget))
      for (const [key, push] of pendingRepositoryPushes) if (!targets.has(push.computer)) { pendingRepositoryPushes.delete(key); activePushes.delete(key) }
      for (const [key, push] of unconfirmedPushes) if (!targets.has(push.computer)) unconfirmedPushes.delete(key)
      // The owning host reporting this very push replaces its unconfirmed result.
      for (const [key, unconfirmed] of unconfirmedPushes) if (pushes.some(push => pushKey(push.computer, push.repositoryPath) === key && push.operationId === unconfirmed.operationId)) unconfirmedPushes.delete(key)
      // Remote polling can return a snapshot captured before the push started.
      // Keep the operation loading until its host supplies a terminal result.
      pushes = [
        ...pushes.filter(push => !pendingRepositoryPushes.has(pushKey(push.computer, push.repositoryPath)) && !unconfirmedPushes.has(pushKey(push.computer, push.repositoryPath))),
        ...pendingRepositoryPushes.values(),
        ...unconfirmedPushes.values(),
      ]
    }
    const cancelledActions = lastCancelledLifecycle(activities)
    return { ...next, source: { ...base.source,
      devices, connections, connectionsError, devicesError, network: services.network(), networkError: services.networkError(), sshAccess: services.sshAccess(), sshAccessError: services.sshAccessError(), operationQueue,
      repositoryPushOperations: pushes,
      activities,
      computers: computers.map(({ lifecycleAction: _reported, ...computer }) => {
        const target = computerTarget(computer)
        const failure = computerFailures.get(computer.configuration.id)
        const action = pendingLifecycle.get(computer.configuration.id)
        // A resubmitted lifecycle action supersedes the last failure or cancellation
        // until it reports its own result.
        const current = action ? { ...computer, lifecycleFailure: undefined, lifecycleFailureAction: undefined, lifecycleFailureCancelled: undefined } : { ...computer, ...reportedCancellation(computer, cancelledActions.get(target)) }
        return { ...current,
          ...(failure?.computerId === computer.configuration.id && { lifecycleFailure: failure.message, lifecycleFailureAction: failure.action as "start" | "stop" | "restart" | "dismiss-error", lifecycleFailureCancelled: failure.cancelled }),
          ...(action && { lifecycleAction: action }),
          ...(action && lifecycleSteps.has(computer.configuration.id) && { lifecycleStep: lifecycleSteps.get(computer.configuration.id) }),
        }
      }),
    } }
  }

  // Mutation responses carry the configuration and computer state but not the enrichment of
  // a full read (D-08): no log output, no repositories, no push operations, and a
  // placeholder GitHub state. Keep those from the state they replace until the full
  // refresh that follows every mutation; the merge stays as a guard once D-08 lands.
  function parseMutationSource(value: unknown, previousSource = snapshot.source, parse = parseApplicationSource): ApplicationSource {
    const parsed = parse(value)
    const previous = new Map((previousSource?.computers ?? []).map(computer => [computer.configuration.id, computer]))
    const computers = parsed.computers.map(computer => ({
      ...computer,
      logs: computer.logs.length ? computer.logs : previous.get(computer.configuration.id)?.logs ?? [],
      repositories: computer.repositories.length ? computer.repositories : previous.get(computer.configuration.id)?.repositories ?? [],
    }))
    const repositoryPushOperations = parsed.repositoryPushOperations.length ? parsed.repositoryPushOperations : previousSource?.repositoryPushOperations ?? []
    return { ...parsed, computers, repositoryPushOperations, github: mutationGitHub(parsed.github, previousSource?.github) }
  }

  function mutationGitHub(github: ApplicationSource["github"], previous: ApplicationSource["github"] | undefined): ApplicationSource["github"] {
    if (!previous) return github
    // A placeholder has neither a policy revision nor a catalog status (the runtime's
    // unavailable fallback has the latter); an older revision is stale.
    const placeholder = github.policyRevision === undefined && github.repositoryCatalogStatus === undefined
    if (placeholder || (github.policyRevision !== undefined && github.policyRevision < (previous.policyRevision ?? 0))) return previous
    return github.deviceIdentity === undefined ? { ...github, deviceIdentity: previous.deviceIdentity } : github
  }

  // A failed or malformed read says nothing about an export or import in progress:
  // keep the last known state (including a running or just-requested operation)
  // and report the problem, instead of inventing a failed result whose Retry the
  // still-running operation would reject.
  function unreadableBackup(message: string): BackupState {
    return { ...snapshot.backup, availability: "unavailable", availabilityMessage: message }
  }

  async function readSetupActivity(requestId?: string) {
    const sequence = operationSequence
    try {
      const events = z.array(siloProgressEventSchema).max(2000).parse(await native.invoke("read_setup_activity"))
      if (disposed || sequence !== operationSequence) return
      if (requestId && !events.some((event) => event.requestId === requestId)) return
      publish({ ...snapshot, setupActivity: events, setupActivityError: undefined })
    } catch {
      if (!disposed && sequence === operationSequence) publish({ ...snapshot, setupActivityError: "Saved setup activity could not be loaded. Retry by reopening Silo." })
    }
  }

  // Remote devices refresh independently: a remote snapshot can take minutes, so
  // each device has at most one read in flight, publishes as soon as it settles,
  // and never holds back another device's status. Callers wait for a device at
  // most REMOTE_READ_WAIT_MS; after that it shows its last known state as stale.
  function remoteRevision(deviceId: string) { return remoteRevisions.get(deviceId) ?? 0 }
  /** Every remote mutation bumps its device's revision: a read that started earlier is dropped and read again. */
  function bumpRemote(deviceId: string) { remoteRevisions.set(deviceId, remoteRevision(deviceId) + 1) }

  function setDevice(device: Device) {
    const order = knownDevices.map(host => host.id)
    devices = [...devices.filter(item => item.id !== device.id), device]
      .sort((left, right) => order.indexOf(left.id) - order.indexOf(right.id))
  }

  function readDevice(deviceId: string, refreshRepositories: boolean): Promise<void> {
    const inFlight = remoteReads.get(deviceId)
    // A read already fetching repositories answers this request too; otherwise one
    // follow-up read with repositories is chained after it.
    if (inFlight) { if (refreshRepositories && !inFlight.repositories) remoteRepositoryReads.add(deviceId); return inFlight.promise }
    const entry = { repositories: refreshRepositories, promise: Promise.resolve() }
    entry.promise = (async () => {
      for (;;) {
        remoteRepositoryReads.delete(deviceId)
        const revision = remoteRevision(deviceId)
        let outcome: { source: ApplicationSource } | { cause: unknown }
        try { outcome = { source: parseRemoteApplicationSource(await native.invoke("device_snapshot", { deviceId, ...(entry.repositories && { refreshRepositories: true }) })) } }
        catch (cause) { outcome = { cause } }
        const host = knownDevices.find(item => item.id === deviceId)
        if (disposed || !host) return
        // A mutation during the read makes this result older than what is shown.
        if (revision === remoteRevision(deviceId)) {
          const lastSeen = devices.find(item => item.id === deviceId)?.lastSeen
          slowDevices.delete(deviceId)
          if ("source" in outcome) {
            remoteFailures.delete(deviceId)
            const previous = new Map((remoteSnapshots.get(deviceId)?.computers ?? []).map(row => [row.configuration.id, row]))
            remoteSnapshots.set(deviceId, { ...outcome.source, computers: outcome.source.computers.map(row => {
              const known = previous.get(row.configuration.id)
              return row.settling && known ? { ...known, settling: true } : row
            }) })
            setDevice({ ...host, connected: true, lastSeen: Date.now() })
          } else if (isUpdateInProgress(outcome.cause)) {
            remoteFailures.delete(deviceId)
            setDevice({ ...host, connected: true, busy: true, lastSeen })
          } else {
            const delay = Math.min((remoteFailures.get(deviceId)?.delay ?? 10_000) * 2, 60_000)
            remoteFailures.set(deviceId, { delay, nextRead: Date.now() + delay })
            setDevice({ ...host, connected: false, error: errorMessage(outcome.cause), lastSeen })
          }
          publish({ ...snapshot })
          if (!remoteRepositoryReads.has(deviceId)) return
          entry.repositories = true
        }
      }
    })().finally(() => { if (remoteReads.get(deviceId) === entry) remoteReads.delete(deviceId) })
    remoteReads.set(deviceId, entry)
    return entry.promise
  }

  function waitForDevice(deviceId: string, read: Promise<void>): Promise<void> {
    let timer: ReturnType<typeof setTimeout> | undefined
    const slow = new Promise<void>(resolve => {
      timer = setTimeout(() => {
        const host = knownDevices.find(item => item.id === deviceId)
        if (!disposed && host && remoteReads.get(deviceId)?.promise === read && !slowDevices.has(deviceId)) {
          slowDevices.add(deviceId)
          const known = devices.find(item => item.id === deviceId)
          setDevice({ ...host, connected: known?.connected ?? false, lastSeen: known?.lastSeen, error: `${host.name} is not responding. Showing its last known state.` })
          publish({ ...snapshot })
        }
        resolve()
      }, REMOTE_READ_WAIT_MS)
    })
    return Promise.race([read, slow]).finally(() => clearTimeout(timer))
  }

  /** The device list; a list read that overlapped a connect or removal is read again. */
  function readDeviceList(background = false): Promise<boolean> {
    if (background && remoteListNextRead > Date.now()) return Promise.resolve(false)
    remoteListRead ??= (async () => {
      for (;;) {
        const revision = remoteListRevision
        let listedDevices: ListedDevice[]
        try { listedDevices = z.array(deviceSchema).parse(await native.invoke("device_list")) }
        catch (cause) {
          if (disposed) return false
          if (revision !== remoteListRevision) continue
          remoteListFailureDelay = Math.min((remoteListFailureDelay || 10_000) * 2, 60_000)
          remoteListNextRead = Date.now() + remoteListFailureDelay
          // The known devices stay listed; the failure is about the list itself.
          devicesError = `Silo could not read its list of devices: ${errorMessage(cause)}`
          publish({ ...snapshot })
          return false
        }
        if (disposed) return false
        if (revision !== remoteListRevision) continue
        devicesError = undefined
        remoteListFailureDelay = 0
        remoteListNextRead = 0
        for (const [id] of remoteFailures) {
          const host = listedDevices.find(host => host.id === id)
          if (!host || host.address !== knownDevices.find(previous => previous.id === id)?.address) remoteFailures.delete(id)
        }
        services.forgetFailures(id => {
          const host = listedDevices.find(host => host.id === id)
          return !host || host.address !== knownDevices.find(previous => previous.id === id)?.address
        })
        knownDevices = listedDevices
        devices = listedDevices.flatMap(host => {
          const known = devices.find(item => item.id === host.id)
          return known ? [{ ...known, name: host.name, address: host.address }] : []
        })
        for (const id of remoteSnapshots.keys()) if (!listedDevices.some(host => host.id === id)) remoteSnapshots.delete(id)
        for (const id of slowDevices) if (!listedDevices.some(host => host.id === id)) slowDevices.delete(id)
        publish({ ...snapshot })
        return true
      }
    })().finally(() => { remoteListRead = undefined })
    return remoteListRead
  }

  async function readConnectionsStatus() {
    const sequence = ++connectionsSequence
    try {
      const management = connectionsStatusSchema.parse(await native.invoke("connections_status"))
      if (disposed || sequence !== connectionsSequence) return
      connections = management
      connectionsError = undefined
    } catch (cause) {
      if (!disposed && sequence === connectionsSequence) connectionsError = errorMessage(cause)
    }
  }

  async function refreshDevices(refreshRepositories = false, background = false) {
    remotePasses++
    const [listed] = await Promise.all([readDeviceList(background), readConnectionsStatus()])
    if (disposed) return
    if (listed) await Promise.all(knownDevices
      .filter(host => !background || (remoteFailures.get(host.id)?.nextRead ?? 0) <= Date.now())
      .map(host => waitForDevice(host.id, readDevice(host.id, refreshRepositories))))
    if (!snapshot.source && snapshot.error) {
      const source = await unavailableLocalSource(snapshot.error)
      if (!snapshot.source && source) publish({ ...snapshot, source })
    } else if (!snapshot.source && localStateUpdating) {
      const source = await updatingLocalSource()
      if (!snapshot.source && source) publish({ ...snapshot, source, loading: false, localUpdating: true })
    }
    publish({ ...snapshot })
  }

  // While this device's computer state is updating (device-wide work such as
  // resuming computers at launch), connected devices stay usable through the shell
  // source instead of a skeleton for the whole operation. Updating is not a runtime
  // failure, and local changes wait for it; `localUpdating` marks the shell.
  let localStateUpdating = false
  async function updatingLocalSource(): Promise<ApplicationSource | null> {
    if (!devices.some(device => device.connected && remoteSnapshots.has(device.id))) return null
    try {
      const shell = parseApplicationSource(await native.invoke("read_application_shell", { error: localUpdatingNotice }))
      return { ...shell, runtimeRepair: null, computerOperationsUnavailable: localUpdatingNotice }
    } catch { return null }
  }

  async function unavailableLocalSource(message: string): Promise<ApplicationSource | null> {
    // A transient read failure never replaces a loaded application with the
    // full-screen error: keep the previous source, marked stale with the error.
    if (!snapshot.source) {
      if (devices.length === 0) return null
      if (!devices.some(device => device.connected && remoteSnapshots.has(device.id))) return null
      try { return parseApplicationSource(await native.invoke("read_application_shell", { error: message })) }
      catch { return null }
    }
    return { ...snapshot.source, computerOperationsUnavailable: message,
      computers: snapshot.source.computers.map(computer => computer.device ? computer : {
        ...computer, freshness: "stale", attention: { level: "error", message },
      }),
    }
  }

  // A read that never answers must not hold polling and event refreshes back for good: after
  // REFRESH_GATE_TIMEOUT_MS the refresh stops counting as active and its caller continues. The
  // read itself keeps running, and its result is still ordered by its sequence number.
  async function refresh(refreshRepositories = false, background = false) {
    activeRefreshes++
    let released = false
    let timer: ReturnType<typeof setTimeout> | undefined
    const release = () => { if (!released) { released = true; activeRefreshes--; clearTimeout(timer) } }
    const stalled = new Promise<void>(resolve => { timer = setTimeout(() => { release(); resolve() }, REFRESH_GATE_TIMEOUT_MS) })
    try { await Promise.race([readSnapshots(refreshRepositories, background), stalled]) }
    finally { release(); lastRefreshFinishedAt = Date.now() }
  }

  /** Refreshes when the window comes back, unless a read is running or one finished moments ago. */
  async function refreshOnReturn() {
    if (document.visibilityState === "hidden" || activeRefreshes > 0 || Date.now() - lastRefreshFinishedAt < RETURN_REFRESH_MIN_AGE_MS) return
    await refresh()
  }

  // Reads may overlap. A read's result is dropped when a mutation published newer
  // state after it started, or when a later read's result is already shown. An
  // UPDATING reply carries no state, so it never displaces an earlier read's result.
  async function readSnapshots(refreshRepositories: boolean, background: boolean) {
    const epoch = refreshSequence
    const sequence = ++readSequence
    const remotePassesAtStart = remotePasses
    const [applicationResult, backupResult] = await Promise.allSettled([
      refreshRepositories ? native.invoke<unknown>("read_application_state", { refreshRepositories: true }) : native.invoke<unknown>("read_application_state"),
      native.invoke<unknown>("read_backup_state"),
    ])
    if (disposed || epoch !== refreshSequence) return
    let source = snapshot.source
    let backup = snapshot.backup
    let error: string | null = null
    let reportedSource: ApplicationSource | undefined
    let configurationUpdating = false
    if (applicationResult.status === "fulfilled") {
      try { source = reportedSource = parseApplicationSource(applicationResult.value) }
      catch (cause) {
        error = `Silo returned invalid application state: ${errorMessage(cause)}`
        source = await unavailableLocalSource(error)
      }
    } else {
      configurationUpdating = isUpdateInProgress(applicationResult.reason)
      if (!configurationUpdating) {
        error = `Silo could not read application state: ${errorMessage(applicationResult.reason)}`
        source = await unavailableLocalSource(error)
      } else if (!source) source = await updatingLocalSource()
    }
    let backendBackup: BackupState | null = null
    if (backupResult.status === "fulfilled") {
      try {
        backup = backendBackup = parseBackupState(backupResult.value)
      }
      catch (cause) { backup = unreadableBackup(`Silo returned invalid export and import state: ${errorMessage(cause)} Refresh to confirm the operation result.`) }
    } else backup = unreadableBackup(`Silo could not read export and import state: ${errorMessage(backupResult.reason)} Refresh to confirm the operation result.`)
    if (disposed || epoch !== refreshSequence) return
    if (backendBackup) { backups.notifyRead(backendBackup, sequence) }
    const applicationCurrent = sequence > appliedApplicationRead
    const backupCurrent = sequence > appliedBackupRead
    if (!applicationCurrent && !backupCurrent && !reportedSource) return
    if (!applicationCurrent) { source = snapshot.source; error = snapshot.error; configurationUpdating = false }
    else if (!configurationUpdating) appliedApplicationRead = sequence
    if (source && reportedSource) {
      // A settling row carries an older runtime reading and cannot advance that row's
      // sequence. An earlier fresh read can still complete it after this read publishes
      // newer metadata and fresh readings for other computers.
      const previousRows = new Map((snapshot.source?.computers ?? []).map(row => [row.configuration.id, row]))
      const incomingRows = new Map(reportedSource.computers.map(row => [row.configuration.id, row]))
      const rows = applicationCurrent ? reportedSource.computers : source.computers
      source = { ...source, computers: rows.map(row => {
        const incoming = incomingRows.get(row.configuration.id)
        const previous = previousRows.get(row.configuration.id)
        if (!incoming) return row
        if (incoming.settling && !applicationCurrent) return row
        if (incoming.settling) return previous ? { ...previous, settling: true } : incoming
        if (sequence <= (appliedComputerReads.get(row.configuration.id) ?? 0)) return previous ?? row
        appliedComputerReads.set(row.configuration.id, sequence)
        return incoming
      }) }
    }
    if (backupCurrent) appliedBackupRead = sequence
    else backup = snapshot.backup
    // Only a known older policy revision is stale. The runtime's fallback for a failed
    // GitHub read carries no revision and must replace the last verified state.
    const githubRevision = source?.github.policyRevision
    if (source && snapshot.source && (githubMutationPending || (githubRevision !== undefined && githubRevision < (snapshot.source.github.policyRevision ?? 0)))) source = { ...source, github: snapshot.source.github }
    if (source && activeConfiguration) source = { ...source, computerConfigurationOperation: activeConfiguration }
    localStateUpdating = configurationUpdating
    publish({ ...snapshot, source, backup, loading: configurationUpdating && !source, error, localUpdating: configurationUpdating && source !== null && (snapshot.source === null || snapshot.localUpdating === true) })
    if (services.networkReadDue()) void services.readNetwork({ background })
    // One remote read per refresh; one that started during this refresh is recent enough.
    if (remotePasses === remotePassesAtStart) void refreshDevices(false, background)
  }

  async function onWindowFocus() {
    await refreshOnReturn()
    if (disposed || !refreshRepositoriesOnReturn) return
    refreshRepositoriesOnReturn = false
    if (snapshot.source?.github.state === "connected") {
      await githubMutation("refresh_github_repositories").catch(() => {})
    }
  }

  // Idempotent: Retry after a failed start calls it again. It subscribes only while
  // not yet subscribed and installs focus, visibility and polling once; later calls
  // just refresh.
  let live = false
  let initialization: Promise<void> | undefined
  function initialize(): Promise<void> {
    initialization ??= (live ? refresh() : startLiveUpdates()).finally(() => { initialization = undefined })
    return initialization
  }

  async function startLiveUpdates() {
    if (disposed) return
    try {
      const subscriptions: Array<[string, EventHandler]> = [
        ["silo://network-state-changed", () => { if (services.networkWanted()) void services.readNetwork() }],
        ["silo://operation-queue-changed", () => { void refreshOperationQueue() }],
        ["silo://application-state-changed", refreshFromEvent],
        ["desktop:status-opened", refreshFromEvent],
        // A cancelled Quit (computers would not stop, settings failed to save) keeps Silo open,
        // so setup and computer configuration must be accepted again.
        ["silo://shutdown-state-changed", (event) => {
          if (event?.payload !== false) return
          setup.resume()
          if (snapshot.setupDrain) publish({ ...snapshot, setupDrain: undefined })
        }],
        ["silo://lifecycle-progress", (event) => {
          const payload = event?.payload as { computerId?: unknown; action?: unknown; step?: unknown } | undefined
          if (typeof payload?.computerId !== "string" || !isLifecycleStep(payload.step) || pendingLifecycle.get(payload.computerId) !== payload.action) return
          lifecycleSteps.set(payload.computerId, payload.step)
          publish({ ...snapshot })
        }],
        ["silo://computer-configuration-progress", (event) => {
          const parsed = siloProgressEventSchema.safeParse(event?.payload)
          if (!parsed.success || parsed.data.requestId !== activeRequestId || !activeConfiguration) return
          const progressEvents = [...activeConfiguration.progressEvents, parsed.data]
          activeConfiguration = { ...activeConfiguration, progressEvents }
          publish({ ...snapshot, setupEvents: progressEvents, setupActivity: progressEvents, source: snapshot.source ? { ...snapshot.source, computerConfigurationOperation: activeConfiguration } : null })
          if (parsed.data.step === "computer-verification" && setup.activeComputerJob) setJobStatus(setup.activeComputerJob, ["computerVerify"], "running")
        }],
      ]
      const results = await Promise.allSettled(subscriptions.map(([event, handler]) => native.listen(event, payload => { if (!disposed) handler(payload) })))
      const stops = results.flatMap(result => result.status === "fulfilled" ? [result.value] : [])
      const failure = results.find(result => result.status === "rejected")
      if (disposed || failure) {
        stops.forEach(stop => stop())
        if (disposed) return
        throw (failure as PromiseRejectedResult).reason
      }
      unlisten.push(...stops)
    } catch (cause) {
      unlisten.splice(0).forEach((stop) => stop())
      if (disposed) return
      const error = `Silo could not subscribe to application updates: ${errorMessage(cause)}`
      publish({ ...snapshot, loading: false, error })
      throw new Error(error)
    }
    if (disposed) { unlisten.splice(0).forEach((stop) => stop()); return }
    live = true
    window.addEventListener("focus", onWindowFocus)
    document.addEventListener("visibilitychange", onVisibilityChange)
    // Polling starts before the first loads finish, so a slow device cannot hold it back.
    remoteTimer ??= setInterval(() => {
      // Repository changes inside a computer do not emit application events. A hidden
      // window (the closed main window, the unopened status panel) does no polling,
      // including remote SSH snapshots; slow reads finish before another poll, and
      // each refresh reads remote devices once when it completes.
      if (document.visibilityState === "hidden") return
      void refreshOperationQueue()
      if (activeRefreshes > 0) return
      void refresh(false, true)
    }, 10_000)
    await Promise.all([refresh(), readSetupActivity(), refreshDevices(), refreshOperationQueue()])
  }

  function onVisibilityChange() { void refreshOnReturn() }

  // Native state events arrive in bursts. Run at most one refresh at a time and
  // one trailing refresh for everything that arrived while it was running.
  let eventRefresh: Promise<void> | undefined
  let eventRefreshAgain = false
  function refreshFromEvent() {
    if (eventRefresh) { eventRefreshAgain = true; return }
    eventRefresh = (async () => {
      do { eventRefreshAgain = false; await refresh() } while (eventRefreshAgain && !disposed)
    })().finally(() => { eventRefresh = undefined })
  }

  // Remote Ports are opened through the remote bridge; the local command
  // never handles `silo-remote:` targets.
  function openNetworkPort(computer: string, port: number) {
    const remote = parseRemoteComputerTarget(computer)
    return native.invoke<void>(remote ? "remote_open_network_port" : "open_network_port", remote ? { ...remote, port } : { computer, port })
  }

  // Once the application has loaded, `snapshot.error` is no longer rendered, so an
  // action failure becomes a keyed failure notice (a repeat replaces the earlier one,
  // and it is mirrored to the system while Silo is in the background).
  function reportActionFailure(key: string, title: string, message: string) {
    if (!snapshot.source) { publish({ ...snapshot, error: message }); return }
    showOperationFailure(key, title, { description: message })
  }

  function reportUnavailable(message: string) {
    void native.invoke("show_integration_error", { message }).catch((cause) => {
      console.error("Silo request failure:", message, errorMessage(cause))
    })
  }

  function setComputerFailure(action: string, computerId: string, cause: unknown) {
    const computer = view.source?.computers.find(computer => computer.configuration.id === computerId)
    if (!computer) return
    const message = errorMessage(cause)
    // A user-requested cancellation is not a failure: record it as a neutral,
    // retryable state so the row shows "<Action> cancelled", not a red error.
    const cancelled = isCancelledError(cause)
    const label = `${action[0].toUpperCase()}${action.slice(1)}`
    computerFailures.set(computerId, { computerId, action, message: cancelled ? message : `${label} failed: ${message}`, cancelled })
    publish({ ...snapshot, source: snapshot.source ? { ...snapshot.source,
      computers: snapshot.source.computers.map(item => item.configuration.id === computerId ? { ...item, freshness: "stale" } : item),
    } : null })
  }

  /** The computer's display name for system notifications; the target encodes the host and id. */
  function remoteDisplayName(target: string): string | null {
    return view.source?.computers.find(item => computerTarget(item) === target)?.configuration.name ?? null
  }

  function computerAction(action: string, name: string, extras: Record<string, unknown> = {}) {
    const remote = parseRemoteComputerTarget(name)
    if (remote && !devices.find(device => device.id === remote.deviceId)?.connected) {
      reportUnavailable(`${devices.find(device => device.id === remote.deviceId)?.name ?? "The remote device"} is offline. Reconnect to it before changing ${remoteDisplayName(name) ?? "the computer"}.`)
      return
    }
    const computerId = view.source?.computers.find(computer => computerTarget(computer) === name)?.configuration.id ?? name
    const stillExists = () => view.source?.computers.some(computer => computer.configuration.id === computerId)
    const key = `${action}:${computerId}`
    if (pendingComputerActions.has(key) || pendingLifecycle.has(computerId)) return
    pendingComputerActions.add(key)
    const lifecycle = action === "start" || action === "stop" || action === "restart" || action === "dismiss-error"
    // Submitting a lifecycle action supersedes any prior failure or cancellation for
    // this computer: the view hides it while the resubmitted action waits or runs.
    if (lifecycle) {
      computerFailures.delete(computerId)
      pendingLifecycle.set(computerId, action)
      lifecycleSteps.delete(computerId)
      publish({ ...snapshot })
    }
    void native.invoke<unknown>(remote && lifecycle ? "remote_computer_action" : "computer_action", remote && lifecycle ? { ...remote, action, name: remoteDisplayName(name), ...extras } : { action, name, ...extras })
      // The follow-up refresh is not awaited: the action is finished, so a repeat must
      // not be ignored while a slow remote snapshot settles.
      .then((result) => {
        if (!stillExists()) return
        if (remote && !lifecycle) { void refreshDevices(); return }
        const previous = remote ? remoteSnapshots.get(remote.deviceId) ?? null : snapshot.source
        let source = remote ? parseMutationSource(result, previous, parseRemoteApplicationSource) : parseMutationSource(result)
        if (lifecycle && previous) {
          // A lifecycle response confirms only its computer. Other rows were captured
          // before this response arrived; the follow-up read refreshes them independently.
          const target = source.computers.find(row => remote ? row.configuration.id === remote.computerId : computerTarget(row) === name)
          source = { ...source, computers: previous.computers.map(row =>
            (remote ? row.configuration.id === remote.computerId : computerTarget(row) === name) ? target ?? row : row) }
        }
        if (lifecycle || computerFailures.get(computerId)?.action === action) computerFailures.delete(computerId)
        if (lifecycle && pendingLifecycle.get(computerId) === action) { pendingLifecycle.delete(computerId); lifecycleSteps.delete(computerId) }
        if (remote) {
          bumpRemote(remote.deviceId)
          remoteSnapshots.set(remote.deviceId, source)
          publish({ ...snapshot, error: null })
          void refreshDevices()
          return
        }
        ++refreshSequence
        publish({ ...snapshot, source, error: null })
        void refresh()
      })
      .catch((cause) => {
        if (!stillExists()) return
        if (!remote) { setComputerFailure(action, computerId, cause); return }
        // One computer's failure (e.g. insufficient memory) belongs on that computer's row. Whether
        // the device itself is reachable is decided by the next transport check.
        if (lifecycle) { setComputerFailure(action, computerId, cause); void refreshDevices(); return }
        reportActionFailure(key, `Could not ${action.replace(/-/g, " ")}`, errorMessage(cause))
      })
      .finally(() => {
        pendingComputerActions.delete(key)
        if (lifecycle && pendingLifecycle.get(computerId) === action) { pendingLifecycle.delete(computerId); lifecycleSteps.delete(computerId) }
        publish({ ...snapshot })
      })
  }

  // The committed local computer inventory the user is editing from. Targeted changes carry
  // this as their `expected` baseline so a queued edit applies to fresh state.
  function committedConfigurations(): SetupComputerConfiguration[] {
    return (snapshot.source?.computers ?? [])
      .filter((computer) => !computer.device)
      .map((computer) => setupComputerConfigurationSchema.parse(computer.configuration))
  }

  // What to apply: a set of targeted changes, or a resume of a failed attempt. An empty
  // change list is a no-op that never reaches the backend.
  type ConfigureAction =
    | { kind: "changes"; changes: ComputerConfigurationChange[] }
    | { kind: "retry"; computer?: string }

  function configureConfigurations(request: SetupComputerConfigurationRequest, action?: ConfigureAction): Promise<ApplicationSource> {
    if (!setup.isAccepting()) return Promise.reject(new Error("Silo is quitting. Setup was not submitted."))
    const resolved: ConfigureAction = action ?? { kind: "changes", changes: deriveComputerChanges(committedConfigurations(), request.computers) }
    // A no-op submission changes nothing; resolve with the current source untouched.
    if (resolved.kind === "changes" && resolved.changes.length === 0) {
      if (!snapshot.source) return Promise.reject(new Error("Computer configuration has not loaded. Refresh and retry."))
      return Promise.resolve(snapshot.source)
    }
    const key = canonicalKey([request, resolved])
    if (lastComputerJob?.key === key) return lastComputerJob.promise
    ++identityVerificationSequence
    lastVerificationKey = undefined
    lastIdentityJob = undefined
    lastGitHubJob = undefined
    setSetupStatus(["identityRun", "identityVerify", "githubRun", "githubVerify", "completion"], "idle")
    const promise = enqueueSetup(["computerRun", "computerVerify"], async (job) => {
      setup.activeComputerJob = job
      setSetupStatus(["identityRun", "identityVerify", "githubRun", "githubVerify", "completion"], "idle")
      ++operationSequence
      const requestId = crypto.randomUUID()
      activeRequestId = requestId
      activeConfiguration = { id: requestId, status: "applying", candidate: request, progressEvents: [], result: null, error: null }
      publish({ ...snapshot, setupCandidate: request, setupEvents: [], setupActivity: [], setupStartedAt: Math.floor(Date.now() / 1000), setupFinishedAt: undefined, source: snapshot.source ? { ...snapshot.source, computerConfigurationOperation: activeConfiguration } : null })
      let failed = false
      let stale = false
      try {
        const result = parseMutationSource(await (resolved.kind === "retry"
          ? native.invoke("retry_computer_configuration", { requestId, ...(resolved.computer ? { retryComputer: resolved.computer } : {}) })
          : native.invoke("change_computer_configuration", { change: resolved.changes.length === 1 ? resolved.changes[0] : { kind: "batch", changes: resolved.changes }, requestId })))
        activeConfiguration = null
        ++refreshSequence
        publish({ ...snapshot, source: result, error: null })
        return result
      } catch (cause) {
        failed = true
        // A stale-baseline rejection is not a setup failure: the edit never applied
        // because the computer changed underneath it. Surface it inline in the editor that
        // raised it (which keeps the user's edits) instead of the configuration-failed
        // banner, and let the caller reject so that editor can react.
        stale = isStaleConfigurationError(cause)
        if (stale) {
          activeConfiguration = null
          if (snapshot.source) publish({ ...snapshot, source: { ...snapshot.source, computerConfigurationOperation: null } })
        } else {
          activeConfiguration = { ...activeConfiguration!, status: "failed", error: { code: "native_bridge_failed", message: errorMessage(cause), recovery: "Review the configuration and retry.", computer: snapshot.setupEvents.at(-1)?.computer ?? null, retryable: true } }
          if (snapshot.source) publish({ ...snapshot, source: { ...snapshot.source, computerConfigurationOperation: activeConfiguration } })
        }
        throw cause
      } finally {
        await readSetupActivity(requestId)
        await refresh()
        if (failed && !stale && !snapshot.setupActivity?.some((event) => event.requestId === requestId && (event.step === "setup-failed" || event.step === "setup-interrupted"))) {
          const event: SiloProgressEvent = { schemaVersion: 1, type: "progress", requestId, phase: "computers", step: "setup-failed", timestamp: Date.now(), level: "error", message: "Silo could not finish computer setup. Review the reported error and retry. This failure could not be retained in activity history.", safeForDisplay: true }
          publish({ ...snapshot, setupActivity: [...(snapshot.setupActivity ?? []), event] })
        }
        activeRequestId = null
        setup.activeComputerJob = undefined
        publish({ ...snapshot, setupFinishedAt: Math.floor(Date.now() / 1000) })
      }
    })
    // Only an in-flight job is shared: a later identical request (Continue on a
    // starting computer, Retry after a failure) must reach the backend again.
    lastComputerJob = { key, promise }
    const settled = () => { if (lastComputerJob?.promise === promise) lastComputerJob = undefined }
    void promise.then(settled, settled)
    return promise
  }

  async function verifySetupIdentities(request: Pick<OnboardingCompletionRequest, "computerConfiguration" | "github">): Promise<void> {
    const key = JSON.stringify([request.computerConfiguration, request.github.computers.map(({ computer, identity }) => ({ computer, identity }))])
    if (key === lastVerificationKey) return
    const sequence = ++identityVerificationSequence
    if (snapshot.setupQueue.some(({ status }) => status === "queued" || status === "running")) {
      await setup.settled()
      if (disposed || sequence !== identityVerificationSequence) return
      return verifySetupIdentities(request)
    }
    lastVerificationKey = key
    lastIdentityJob = undefined
    setSetupStatus(["identityRun", "identityVerify"], "idle")
    const identities = request.github.computers.map(({ computer, identity }) => ({ computer, ...identity }))
    const configurations = request.computerConfiguration.computers
    if (identities.length !== configurations.length || configurations.some(({ name }) => !identities.some(({ computer }) => computer === name))) return
    try {
      const verified = z.boolean().parse(await native.invoke("verify_computer_identities", { identities }))
      if (disposed || sequence !== identityVerificationSequence) return
      setSetupStatus(["identityRun", "identityVerify"], verified ? "succeeded" : "idle")
    } catch {
      // A read failure cannot establish completion. Continue will run the normal
      // setup operation and report any actionable runtime error there.
      if (!disposed && sequence === identityVerificationSequence) setSetupStatus(["identityRun", "identityVerify"], "idle")
    }
  }

  function submitSetupStep(step: "computers" | "github", request: OnboardingCompletionRequest): Promise<unknown> {
    if (!setup.isAccepting()) return Promise.reject(new Error("Silo is quitting. Setup was not submitted."))
    ++identityVerificationSequence
    const current = snapshot.source
    const configurationsUnchanged = current && current.computers.length > 0
      && !current.computerConfigurationOperation && !activeConfiguration
      && !setup.isBusy()
      && current.computers.every(({ freshness, state }) => freshness === "fresh" && state !== "failed" && state !== "starting")
      && JSON.stringify(current.computers.map(({ configuration }) => setupComputerConfigurationSchema.parse(configuration))) === JSON.stringify(request.computerConfiguration.computers.map((configuration) => setupComputerConfigurationSchema.parse(configuration)))
    // Initial setup and continues send the specific creations/edits as one batch. When
    // the configuration already matches what was committed but an earlier attempt did
    // not complete, resume that attempt instead of sending an empty change set.
    const changes = configurationsUnchanged ? [] : deriveComputerChanges(committedConfigurations(), request.computerConfiguration.computers)
    const replaced = replacesEveryComputer(request)
    if (replaced) return Promise.reject(replaced)
    const computerJob = configurationsUnchanged
      ? Promise.resolve(current)
      : configureConfigurations(request.computerConfiguration, changes.length > 0 ? { kind: "changes", changes } : { kind: "retry" })
    if (step === "computers") return computerJob
    const activityId = crypto.randomUUID()
    const identities = request.github.computers.map(({ computer, identity }) => ({ computer, ...identity }))
    const identityKey = JSON.stringify([request.computerConfiguration, identities])
    if (lastIdentityJob?.key !== identityKey) {
      const promise = enqueueSetup(["identityRun", "identityVerify"], async () => {
        await computerJob
        await native.invoke("configure_computer_identities", { identities })
        lastVerificationKey = JSON.stringify([request.computerConfiguration, request.github.computers.map(({ computer, identity }) => ({ computer, identity }))])
      }, activityId)
      lastIdentityJob = { key: identityKey, promise }
      void promise.catch(() => { if (lastIdentityJob?.promise === promise) lastIdentityJob = undefined })
    }
    const identityJob = lastIdentityJob.promise
    const key = JSON.stringify([request.computerConfiguration, request.github])
    if (lastGitHubJob?.key === key) return lastGitHubJob.promise
    const promise = enqueueSetup(["githubRun", "githubVerify"], async (job) => {
      await identityJob
      const policies = request.github.computers.filter((policy) => request.github.connectionState === "connected" || policy.authenticationMethod === "token")
      if (policies.length > 0) {
        const previous = snapshot.source?.github
        // Setup never turns GitHub access on or off: an explicit Disable access stays in effect.
        let github = await githubMutation("save_github_configuration", { configuration: { baseRevision: previous?.policyRevision, deviceIdentity: snapshot.source?.github.deviceIdentity ?? null, computers: policies.map((policy) => ({ repositoryMode: "selected", allRepositoriesAllowChanges: false, ...policy })) } })
        if (previous?.policyRevision === github.policyRevision && previous?.computerOperations?.some(({ status }) => status === "failed") && github.computerOperations?.some(({ status }) => status === "failed")) github = await githubMutation("retry_github_configuration")
        setJobStatus(job, ["githubRun"], "succeeded")
        setJobStatus(job, ["githubVerify"], "running")
        recordGitHubActivity(job.activityId, "github", "GitHub settings saved. Waiting for each computer to confirm access.")
        await waitForGitHubAccess(github, policies.map(({ computer }) => computer))
      }
    }, activityId)
    lastGitHubJob = { key, promise }
    void promise.catch(() => { if (lastGitHubJob?.promise === promise) lastGitHubJob = undefined })
    return promise
  }

  // Onboarding can mount before the real configuration loads and seed its draft
  // from defaults. A draft that keeps none of the existing computers is therefore never
  // treated as a request to delete them all; single removals remain explicit edits.
  function replacesEveryComputer(request: OnboardingCompletionRequest) {
    const committed = committedConfigurations()
    if (committed.length === 0) return null
    const kept = new Set(request.computerConfiguration.computers.map(({ id }) => id))
    if (committed.some(({ id }) => kept.has(id))) return null
    return new Error(`Setup does not delete existing computers (${committed.map(({ name }) => name).join(", ")}). Reopen Silo to load them, or delete them from Silo after setup. No computer changed.`)
  }

  function finishSetup(request: OnboardingCompletionRequest, markComplete: () => Promise<void>) {
    if (!setup.isAccepting()) return Promise.reject(new Error("Silo is quitting. Setup was not submitted."))
    const preceding = submitSetupStep("github", request)
    void preceding.catch(() => {})
    if (replacesEveryComputer(request)) return preceding
    return enqueueSetup(["completion"], async () => { await preceding; await markComplete() })
  }

  function saveComputerConfiguration(request: SetupComputerConfigurationRequest, baseline?: SetupComputerConfiguration[]): Promise<void> {
    // Send the specific create/edit/delete/reorder against the baseline the user started
    // editing from — the committed configuration as it was when the editor opened — so a
    // queued edit applies to the latest settings, and is rejected instead of silently
    // overwriting concurrent work, when the computer changed while the edit waited. When no
    // baseline is supplied (e.g. onboarding drafts) the current committed list is used.
    // Several simultaneous changes travel as one atomic batch; a no-op does nothing.
    const changes = deriveComputerChanges(baseline ?? committedConfigurations(), request.computers)
    if (changes.length === 0) return Promise.resolve()
    return configureConfigurations(request, { kind: "changes", changes }).then(() => undefined)
  }

  async function waitForGitHubAccess(initial: z.infer<typeof githubStateShape>, computers: string[]) {
    let github = initial
    const revision = initial.policyRevision
    const deadline = Date.now() + 300_000
    const quitting = () => new Error("Silo is quitting. GitHub access was not verified; Continue after reopening Silo to check again.")
    while (true) {
      if (disposed) throw new Error("Silo closed before GitHub access was verified.")
      if (!setup.isAccepting()) throw quitting()
      if (revision !== undefined && (github.policyRevision !== revision || (snapshot.source?.github.policyRevision ?? revision) > revision)) throw new Error("GitHub settings changed during setup. Continue again to verify the latest settings.")
      const operations = computers.map((computer) => github.computerOperations?.find((operation) => operation.computer === computer))
      const failure = operations.find((operation) => operation?.status === "failed")
      if (failure) throw new Error(failure.message)
      if (operations.every((operation) => operation?.status === "succeeded")) return
      if (Date.now() >= deadline) throw new Error("GitHub access has not been verified in every computer. Retry to check again.")
      await setupDelay(500)
      if (disposed) throw new Error("Silo closed before GitHub access was verified.")
      if (!setup.isAccepting()) throw quitting()
      github = githubStateShape.parse(await native.invoke("read_github_state"))
      if (!githubMutationPending && snapshot.source && (github.policyRevision ?? 0) >= (snapshot.source.github.policyRevision ?? 0)) publish({ ...snapshot, source: { ...snapshot.source, github } })
    }
  }

  async function githubMutation(command: string, arguments_?: Record<string, unknown>) {
    const sequence = ++githubMutationSequence
    githubMutationPending = command === "save_github_configuration"
    ++refreshSequence
    const connectionAttempt = command === "connect_github" ? crypto.randomUUID() : null
    if (connectionAttempt) recordGitHubActivity(connectionAttempt, "github", "Opening GitHub authorization in your browser.")
    try {
      const github = githubStateShape.parse(await native.invoke(command, arguments_))
      if (sequence === githubMutationSequence && snapshot.source && (github.policyRevision ?? 0) >= (snapshot.source.github.policyRevision ?? 0)) publish({ ...snapshot, source: { ...snapshot.source, github }, error: null })
      if (connectionAttempt && sequence === githubMutationSequence) recordGitHubActivity(connectionAttempt, "github", github.state === "connected" ? "GitHub account connected." : "GitHub authorization is pending.")
      return github
    } catch (cause) {
      if (connectionAttempt && sequence === githubMutationSequence) recordGitHubActivity(connectionAttempt, "github", "GitHub connection did not complete. You can try connecting again.", true)
      if (command === "connect_github" && sequence === githubMutationSequence) {
        try {
          const github = githubStateShape.parse(await native.invoke("read_github_state"))
          if (sequence === githubMutationSequence && snapshot.source) publish({ ...snapshot, source: { ...snapshot.source, github } })
        } catch { /* Keep the last verified state if reading also fails. */ }
      }
      const message = `GitHub operation failed: ${errorMessage(cause)}`
      if (sequence === githubMutationSequence && snapshot.source) publish({ ...snapshot, error: message, source: { ...snapshot.source, github: { ...snapshot.source.github,
        ...(!(["save_github_configuration", "save_github_personal_token", "remove_github_personal_token"].includes(command)) && { repositoryCatalogStatus: { status: "unavailable" as const, message, canRetry: command === "refresh_github_repositories" } }),
      } } })
      throw cause
    } finally {
      if (sequence === githubMutationSequence) githubMutationPending = false
    }
  }

  async function checkpointAction(command: string, target: string, arguments_: Record<string, unknown>) {
    const remote = parseRemoteComputerTarget(target)
    const localComputer = remote ? undefined : snapshot.source?.computers.find(item => !item.device && (item.configuration.name === target || item.configuration.id === target))
    if (!remote && !localComputer) throw new Error("This computer is unavailable. Refresh and try again.")
    const checkpointTarget = remote ? remoteComputerTarget(remote.deviceId, remote.computerId) : computerTarget(localComputer!)
    const ownerComputer = remote
      ? remoteSnapshots.get(remote.deviceId)?.computers.find(item => item.configuration.id === remote.computerId)
      : localComputer
    if (pendingCheckpointOperations.has(checkpointTarget) || ownerComputer?.checkpointOperation?.status === "running") {
      throw new Error("A checkpoint operation is already running for this computer.")
    }
    const kind: ComputerCheckpointOperation["kind"] = command === "create_checkpoint" ? "capture" : command === "fork_checkpoint" ? "fork" : command === "restore_checkpoint" ? "restore" : command === "delete_checkpoint" ? "delete" : command === "abandon_restore" ? "restore" : (() => { throw new Error("Unsupported checkpoint operation.") })()
    if (remote && kind === "delete") throw new Error("Delete checkpoints of this computer in Silo on its own device.")
    if (remote && command === "abandon_restore") throw new Error("Abandon this Restore in Silo on the computer’s own device.")
    const operation: ComputerCheckpointOperation = {
      kind,
      status: "running",
      stage: kind === "capture" ? "Creating checkpoint…" : kind === "fork" ? "Creating stopped fork…" : kind === "delete" ? "Deleting checkpoint…" : command === "abandon_restore" ? "Abandoning Restore…" : "Saving recovery checkpoint and restoring…",
    }
    pendingCheckpointOperations.set(checkpointTarget, operation)
    publish({ ...snapshot })
    if (remote) {
      const action = kind === "capture" ? "create" : kind
      try {
        await native.invoke("remote_checkpoint_action", { deviceId: remote.deviceId, computerId: remote.computerId, action, ...arguments_ })
        bumpRemote(remote.deviceId)
        await refreshDevices(true)
      } catch (cause) {
        bumpRemote(remote.deviceId)
        void refreshDevices()
        throw cause
      } finally {
        pendingCheckpointOperations.delete(checkpointTarget)
        publish({ ...snapshot })
      }
      return
    }
    try {
      const result = await native.invoke<unknown>(command, { computerId: localComputer!.configuration.id, ...arguments_ })
      ++refreshSequence
      let source = parseMutationSource(result)
      if (snapshot.source) {
        const incoming = new Map(source.computers.map(computer => [computer.configuration.id, computer]))
        const computers = snapshot.source.computers.map(computer => computer.configuration.id === localComputer!.configuration.id ? incoming.get(computer.configuration.id) ?? computer : computer)
        if (kind === "fork" && typeof arguments_.newName === "string") {
          const fork = source.computers.find(computer => computer.configuration.name === arguments_.newName && !computers.some(current => current.configuration.id === computer.configuration.id))
          if (fork) computers.push(fork)
        }
        source = { ...source, computers }
      }
      publish({ ...snapshot, source, error: null })
      void refresh()
    } catch (cause) {
      void refresh()
      throw cause
    } finally {
      pendingCheckpointOperations.delete(checkpointTarget)
      publish({ ...snapshot })
    }
  }

  function publishRemoteComputerChange(deviceId: string, computerId: string, result: unknown, remove = false) {
    const previous = remoteSnapshots.get(deviceId) ?? null
    let source = parseMutationSource(result, previous, parseRemoteApplicationSource)
    if (previous) {
      const target = source.computers.find(computer => computer.configuration.id === computerId)
      const computers = remove ? previous.computers.filter(computer => computer.configuration.id !== computerId)
        : previous.computers.map(computer => computer.configuration.id === computerId ? target ?? computer : computer)
      if (!remove && target && !computers.some(computer => computer.configuration.id === computerId)) computers.push(target)
      source = { ...source, computers }
    }
    bumpRemote(deviceId)
    remoteSnapshots.set(deviceId, source)
    publish({ ...snapshot })
    void refreshDevices()
  }

  const applicationActions: ApplicationActions = {
    createCheckpoint: (computer, name) => checkpointAction("create_checkpoint", computer, { name }),
    forkCheckpoint: (computer, checkpointId, newName) => checkpointAction("fork_checkpoint", computer, { checkpointId, newName }),
    restoreCheckpoint: (computer, checkpointId) => checkpointAction("restore_checkpoint", computer, { checkpointId }),
    deleteCheckpoint: (computer, checkpointId) => checkpointAction("delete_checkpoint", computer, { checkpointId }),
    abandonRestore: computer => checkpointAction("abandon_restore", computer, {}),
    readCheckpointUsage: async computerId => checkpointUsageSchema.parse(await native.invoke("read_checkpoint_usage", { computerId })),
    refreshRepositories: async () => {
      await Promise.all([refresh(true), refreshDevices(true)])
      if (snapshot.error) throw new Error(snapshot.error)
    },
    queryLogs: async request => logPageSchema.parse(await native.invoke("query_computer_logs", { request })),
    exportLogs: async requests => z.boolean().parse(await native.invoke("export_computer_logs", { requests })),
    cancelLogExport: async () => { await native.invoke("cancel_log_export") },
    setConnectionsEnabled: async enabled => {
      const sequence = ++connectionsSaveSequence
      ++connectionsSequence
      const management = connectionsStatusSchema.parse(await native.invoke("set_connections_enabled", { enabled }))
      if (disposed || sequence !== connectionsSaveSequence) return
      ++connectionsSequence
      connections = management
      connectionsError = undefined
      publish({ ...snapshot })
    },
    connectDevice: async (address, options) => {
      deviceSchema.parse(await native.invoke("connect_device", { address, replace: options?.replaceAddress ?? false }))
      // A list read that started before the connection is read again, so the new
      // device is listed when this resolves.
      remoteListRevision++
      await refreshDevices()
    },
    removeDevice: async deviceId => {
      await native.invoke("remove_device", { deviceId })
      remoteListRevision++
      bumpRemote(deviceId)
      knownDevices = knownDevices.filter(host => host.id !== deviceId)
      devices = devices.filter(device => device.id !== deviceId)
      remoteSnapshots.delete(deviceId)
      slowDevices.delete(deviceId)
      publish({ ...snapshot })
    },
    saveRemoteComputer: async (deviceId, configuration, expected) => {
      const target = parseRemoteComputerTarget(configuration.id)
      const result = await native.invoke("remote_upsert_computer", {
        deviceId, configuration: { ...configuration, id: target?.computerId ?? configuration.id },
        expected: expected ? { ...expected, id: parseRemoteComputerTarget(expected.id)?.computerId ?? expected.id } : null,
      })
      publishRemoteComputerChange(deviceId, target?.computerId ?? configuration.id, result)
    },
    deleteRemoteComputer: async (deviceId, configuration) => {
      const computerId = parseRemoteComputerTarget(configuration.id)?.computerId
      if (!computerId) throw new Error("Silo could not identify the remote computer. Refresh its device and retry.")
      const result = await native.invoke("remote_delete_computer", { deviceId, computerId, expected: { ...configuration, id: computerId } })
      publishRemoteComputerChange(deviceId, computerId, result, true)
    },
    saveSecret: (request: SecretConfigurationRequest) => changeSecret("save_secret", { request }),
    removeSecret: (id: string) => changeSecret("remove_secret", { id }),
    retrySecret: (id: string) => changeSecret("retry_secret", { id }),
    readWorkspaceStorage: async computerId => workspaceStorageStateSchema.parse(await native.invoke("read_workspace_storage", { computerId })),
    reclaimWorkspaceStorage: async computerId => workspaceStorageStateSchema.parse(await native.invoke("reclaim_workspace_storage", { computerId })),
    refreshSshAccess: services.refreshSshAccess,
    sshConnection: (computer, download, network) => {
      const remote = parseRemoteComputerTarget(computer)
      return native.invoke<string | null>("ssh_connection", remote ? { ...remote, download, ...(network === undefined ? {} : { network }) } : { computer, download, ...(network === undefined ? {} : { network }) })
    },
    saveSshAccess: services.saveSshAccess,
    refreshNetwork: services.refreshNetwork,
    saveNetworkPort: request => services.changeNetwork("save_network_port", { ...request }),
    removeNetworkPort: (computer, port) => services.changeNetwork("remove_network_port", { computer, port }),
    openNetworkPort,
    authorizeDevice: address => native.invoke<void>("authorize_device", { address }),
    setupDeviceKey: address => native.invoke<void>("setup_device_key", { address }),
    listComputerDirectory: async (computer, path, offset, snapshotId) => directoryPageShape.parse(await native.invoke("list_computer_directory", { computer, path, offset, snapshotId: snapshotId ?? null })),
    fileTransfers: {
      chooseUploadFiles: async () => uploadSelectionSchema.nullable().parse(await native.invoke("choose_upload_files")),
      upload: async ({ id, computer, directory, selection, conflict }) => uploadOutcomeSchema.parse(await native.invoke("upload_files", { transferId: id, computer, directory, selection, conflict })),
      download: async ({ id, computer, path }) => downloadOutcomeSchema.parse(await native.invoke("download_file", { transferId: id, computer, path })),
      cancel: async id => { await native.invoke("cancel_transfer", { transferId: id }) },
      onProgress: handler => native.listen(transferProgressEvent, event => {
        const parsed = transferProgressSchema.safeParse(event?.payload)
        if (parsed.success) handler(parsed.data)
      }),
    },
    retryRuntimeChecks: () => { void refresh() },
    saveComputerConfiguration,
    dismissComputerConfigurationError: () => {
      if (activeConfiguration?.status !== "failed") return
      activeConfiguration = null
      publish({ ...snapshot, setupCandidate: undefined, source: snapshot.source ? { ...snapshot.source, computerConfigurationOperation: null } : null })
    },
    retryComputerConfiguration: (computer) => {
      const operation = snapshot.source?.computerConfigurationOperation
      if (operation) void configureConfigurations(operation.candidate, { kind: "retry", computer }).catch(() => {})
    },
    dismissRepositoryPush: (computer, repositoryPath) => statusActions.dismissRepositoryPush(computer, repositoryPath),
    pushRepository: (computer, repositoryPath, target) => {
      const key = pushKey(computer, repositoryPath)
      if (pendingRepositoryPushes.has(key) || view.source?.repositoryPushOperations.some(operation => operation.computer === computer && operation.repositoryPath === repositoryPath && (operation.status === "pushing" || operation.status === "unknown"))) return
      const commitCount = view.source?.computers.find(item => computerTarget(item) === computer)?.repositories.find(repository => repository.path === repositoryPath)?.ahead ?? 0
      // Polling stops once this push is finished or its computer (or device) is gone.
      const owner = {}
      const current = () => !disposed && activePushes.get(key) === owner
      activePushes.set(key, owner)
      unconfirmedPushes.delete(key)
      pendingRepositoryPushes.set(key, { computer, repositoryPath, commitCount, status: "pushing" })
      publish({ ...snapshot })
      // Push results are shown on the repository row, never through `snapshot.error`,
      // which is not rendered once the application has loaded.
      const finish = (operation: ApplicationSource["repositoryPushOperations"][number]) => {
        pendingRepositoryPushes.delete(key)
        activePushes.delete(key)
        ++refreshSequence
        const remote = parseRemoteComputerTarget(computer)
        if (remote) bumpRemote(remote.deviceId)
        const host = remote && remoteSnapshots.get(remote.deviceId)
        if (remote && host) {
          const name = host.computers.find(computer => computer.configuration.id === remote.computerId)?.configuration.name
          if (name) remoteSnapshots.set(remote.deviceId, { ...host, repositoryPushOperations: [
            ...host.repositoryPushOperations.filter(operation => operation.computer !== name || operation.repositoryPath !== repositoryPath),
            { ...operation, computer: name },
          ] })
        }
        if (snapshot.source) publish({ ...snapshot, source: { ...snapshot.source,
          repositoryPushOperations: [...snapshot.source.repositoryPushOperations.filter(operation => operation.computer !== computer || operation.repositoryPath !== repositoryPath), { ...operation, computer }],
        } })
      }
      let operationId: string = crypto.randomUUID()
      const terminalShape = z.discriminatedUnion("status", [
        z.object({ operationId: z.string().optional(), status: z.literal("succeeded"), commitCount: z.number() }),
        z.object({ operationId: z.string().optional(), status: z.literal("unknown"), commitCount: z.number(), message: z.string() }),
        z.object({ operationId: z.string().optional(), status: z.literal("failed"), commitCount: z.number(), message: z.string(), diagnosticDetails: z.string().optional() }),
      ])
      // Unanswered status checks back off exponentially; after PUSH_STATUS_ATTEMPTS in a
      // row the push ends as a dismissible "unknown" result instead of spinning forever.
      let failures = 0
      const schedule = () => {
        if (!current()) return
        const timer = setTimeout(() => {
          pushPollTimers.delete(timer)
          void observe(false)
        }, Math.min(PUSH_STATUS_INTERVAL_MS * 2 ** failures, PUSH_STATUS_MAX_INTERVAL_MS))
        pushPollTimers.add(timer)
      }
      const observe = async (start: boolean): Promise<void> => {
        if (!current()) return
        try {
          // The host pushes exactly the confirmed target or reports that the repository changed.
          const result = await native.invoke(start ? "start_repository_push" : "repository_push_status", start ? { computer, repositoryPath, operationId, target } : { computer, repositoryPath, operationId })
          if (!current()) return
          // No saved job means the first request never arrived. Reuse its identifier.
          if (result === null && !start) return observe(true)
          const parsed = terminalShape.safeParse(result)
          if (parsed.success) {
            const { operationId: _id, ...operation } = parsed.data
            finish({ ...operation, computer, repositoryPath })
            if (operation.status === "succeeded") void refresh()
            return
          }
          const active = z.object({ operationId: z.string(), status: z.literal("pushing") }).parse(result)
          operationId = active.operationId
          failures = 0
          pendingRepositoryPushes.set(key, { computer, repositoryPath, commitCount, status: "pushing" })
          publish({ ...snapshot })
        } catch (cause) {
          if (!current()) return
          // A lost connection is not a failed push. Keep the button disabled and
          // query the host-owned operation until it supplies an actual result.
          if (++failures >= PUSH_STATUS_ATTEMPTS) {
            pendingRepositoryPushes.delete(key)
            activePushes.delete(key)
            unconfirmedPushes.set(key, { operationId, computer, repositoryPath, commitCount, status: "unknown", message: `Silo could not confirm this push (${errorMessage(cause)}). Check the branch on GitHub before pushing again.` })
            publish({ ...snapshot })
            return
          }
          pendingRepositoryPushes.set(key, { computer, repositoryPath, commitCount, status: "pushing", message: `Waiting for push status: ${errorMessage(cause)}` })
          publish({ ...snapshot })
        }
        schedule()
      }
      void observe(true)
    },
    startComputer: (name) => computerAction("start", name),
    stopComputer: (name) => computerAction("stop", name),
    restartComputer: (name) => computerAction("restart", name),
    cancelOperation: (id) => {
      void native.invoke("cancel_operation", { id })
        .then(() => refreshOperationQueue())
        .catch((cause) => reportUnavailable(`Silo could not cancel the operation: ${errorMessage(cause)}`))
    },
    dismissComputerError: (name) => computerAction("dismiss-error", name),
    openDesktop: async (computer) => {
      try { await native.invoke("open_desktop", { computer }) }
      catch (cause) { reportActionFailure(`open-desktop:${computer}`, "Could not open the Linux desktop", errorMessage(cause)) }
    },
    openTerminal: (name) => computerAction("open-terminal", name),
    openEditor: (name, path) => computerAction("open-editor", name, path ? { path } : undefined),
    saveGitHubPersonalToken: async token => { await githubMutation("save_github_personal_token", { token }) },
    removeGitHubPersonalToken: async () => { await githubMutation("remove_github_personal_token") },
    connectGitHub: () => { void githubMutation("connect_github").catch(() => {}) },
    cancelGitHubConnection: () => { void githubMutation("cancel_github_connection").catch(() => {}) },
    reopenGitHubAuthorization: () => {
      const sequence = githubMutationSequence
      void native.invoke("reopen_github_authorization").catch((cause: unknown) => {
        if (sequence === githubMutationSequence) reportActionFailure("github-reopen-authorization", "Could not reopen GitHub authorization", errorMessage(cause))
      })
    },
    manageGitHubRepositories: () => {
      refreshRepositoriesOnReturn = true
      void native.invoke("manage_github_repositories").catch((cause: unknown) => {
        refreshRepositoriesOnReturn = false
        reportActionFailure("github-manage-repositories", "Could not open GitHub repository access", errorMessage(cause))
      })
    },
    disconnectGitHub: () => { void githubMutation("disconnect_github").catch(() => {}) },
    setGitHubAccessEnabled: (enabled) => { void githubMutation("set_github_access_enabled", { enabled }).catch(() => {}) },
    saveGitHubConfiguration: async (configuration) => { await githubMutation("save_github_configuration", { configuration }) },
    retryGitHubConfiguration: (computer) => { void githubMutation("retry_github_configuration", { computer: computer ?? null }).catch(() => {}) },
    retryGitHubRepositoryCatalog: () => { void githubMutation("refresh_github_repositories").catch(() => {}) },
  }

  const statusActions: StatusBarActions = {
    listComputerDirectory: applicationActions.listComputerDirectory,
    startComputer: applicationActions.startComputer,
    stopComputer: applicationActions.stopComputer,
    restartComputer: applicationActions.restartComputer,
    openTerminal: applicationActions.openTerminal,
    pushRepository: applicationActions.pushRepository,
    openSilo: (route?: StatusBarRoute) => { void native.invoke("open_main", { route: route ?? null }).catch((cause) => console.error("Silo main window:", errorMessage(cause))) },
    quit: () => { void native.invoke("quit_app").catch((cause: unknown) => reportActionFailure("quit", "Could not quit Silo", errorMessage(cause))) },
    refresh: () => { void refresh() },
    openEditor: (name, path) => computerAction("open-editor", name, { path }),
    openSite: (computer, port) => { void Promise.resolve().then(() => openNetworkPort(computer, port)).catch(() => reportUnavailable("Could not open this service. Check its port in Network.")) },
    dismissRepositoryPush: (computer, repositoryPath) => {
      // An unconfirmed result exists only here: acknowledging it never waits on its
      // (possibly unreachable) host, which is told on a best-effort basis.
      if (unconfirmedPushes.delete(pushKey(computer, repositoryPath))) {
        publish({ ...snapshot })
        void native.invoke("dismiss_repository_push", { computer, repositoryPath }).catch(() => {})
        return
      }
      void native.invoke("dismiss_repository_push", { computer, repositoryPath }).then(() => {
        ++refreshSequence
        const remote = parseRemoteComputerTarget(computer)
        if (remote) bumpRemote(remote.deviceId)
        const owner = remote && remoteSnapshots.get(remote.deviceId)
        if (remote && owner) {
          const name = owner.computers.find(computer => computer.configuration.id === remote.computerId)?.configuration.name
          remoteSnapshots.set(remote.deviceId, { ...owner, repositoryPushOperations: owner.repositoryPushOperations.filter(operation => operation.computer !== name || operation.repositoryPath !== repositoryPath || operation.status === "pushing") })
        }
        if (snapshot.source) publish({ ...snapshot, source: { ...snapshot.source,
          repositoryPushOperations: snapshot.source.repositoryPushOperations.filter(operation => operation.computer !== computer || operation.repositoryPath !== repositoryPath || operation.status === "pushing"),
        } })
        void refresh()
      }).catch(cause => console.error("Silo push dismissal:", errorMessage(cause)))
    },
  }

  return {
    getSnapshot: () => view,
    subscribe(listener: () => void) { listeners.add(listener); return () => listeners.delete(listener) },
    initialize,
    // The saved list only draws loading rows before live state arrives, so it never
    // blocks or fails startup: an unreadable or unexpected list shows no rows.
    async loadConfiguration() {
      try {
        const configuration = z.object({ computers: z.array(z.unknown()) }).parse(await native.invoke("read_computer_configuration"))
        const configurations = configuration.computers.flatMap((configuration) => {
          const parsed = setupComputerConfigurationSchema.safeParse(configuration)
          return parsed.success ? [parsed.data] : []
        })
        if (!disposed) publish({ ...snapshot, savedConfigurations: configurations })
      } catch (cause) {
        console.error("Silo saved computers:", errorMessage(cause))
        if (!disposed) publish({ ...snapshot, savedConfigurations: [] })
      }
    },
    refresh,
    watchNetwork: services.watchNetwork,
    configureConfigurations,
    submitSetupStep,
    verifySetupIdentities,
    finishSetup,
    drainSetup,
    applicationActions,
    backupActions: backups.actions,
    statusActions,
    dispose() { backups.close(); pushPollTimers.forEach(clearTimeout); pushPollTimers.clear(); if (remoteTimer) clearInterval(remoteTimer); disposed = true; setup.wake(); refreshSequence++; unlisten.splice(0).forEach((stop) => stop()); window.removeEventListener("focus", onWindowFocus); document.removeEventListener("visibilitychange", onVisibilityChange); listeners.clear() },
  }
}

export type ProductionSource = ReturnType<typeof createProductionSource>

/** The current snapshot with its backup controller; the same object until the snapshot changes. */
export function useProductionSource(source: ProductionSource) {
  const snapshot = useSyncExternalStore(source.subscribe, source.getSnapshot)
  return useMemo(() => ({
    ...snapshot,
    backup: { state: snapshot.backup, actions: source.backupActions } satisfies BackupController,
  }), [snapshot, source])
}
