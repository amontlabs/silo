import { hasBridgeErrorCode } from "@/contracts/bridge-error"
import { errorMessage as sharedErrorMessage } from "@/lib/error-message"
import type { ApplicationActivity, ApplicationComputer, ApplicationPort, NetworkState } from "@/features/application/model/application-source"
import { cancelledActionLabel } from "@/features/application/model/operation-queue"
import type { BackupArchive, BackupOperation, BackupState } from "@/features/application/model/backup-source"
import { parseRemoteComputerTarget } from "@/features/application/model/connections"
import type { LifecycleAction } from "./production-types"

const lifecycleActions: LifecycleAction[] = ["start", "stop", "restart"]

/** How long a caller waits for one device's snapshot before showing its last known state as stale. */
export const REMOTE_READ_WAIT_MS = 15_000

/** Push status polling: the normal interval, the backoff ceiling, and unanswered checks before giving up. */
export const PUSH_STATUS_INTERVAL_MS = 2_000
export const PUSH_STATUS_MAX_INTERVAL_MS = 30_000
export const PUSH_STATUS_ATTEMPTS = 8

/** A refresh that has not finished by then stops holding back polling and focus refreshes; its late result still applies unless a newer read did. */
export const REFRESH_GATE_TIMEOUT_MS = 60_000
/** Returning to the window within this time of the last finished read does not read again. */
export const RETURN_REFRESH_MIN_AGE_MS = 2_000
/** A consumer of network data that polls keeps the source reading it for this long after its last request. */
export const NETWORK_INTEREST_MS = 30_000
/** An ambient watcher (a window whose menus list open sites) reads network services at most this often. */
export const NETWORK_AMBIENT_INTERVAL_MS = 30_000

export function errorMessage(error: unknown): string {
  return sharedErrorMessage(error, { fallback: "Silo could not complete the action. Retry; if it fails again, relaunch Silo." })
}

/** A JSON key that does not depend on object property order. */
export function canonicalKey(value: unknown): string {
  return JSON.stringify(value, (_key, item: unknown) => item && typeof item === "object" && !Array.isArray(item)
    ? Object.fromEntries(Object.entries(item as Record<string, unknown>).sort(([left], [right]) => left < right ? -1 : left > right ? 1 : 0))
    : item)
}

export function isPlainObject(value: unknown): value is Record<string, unknown> {
  if (value === null || typeof value !== "object") return false
  const prototype = Object.getPrototypeOf(value)
  return prototype === Object.prototype || prototype === null
}

/**
 * `next` with every part that equals the same part of `previous` replaced by that part, so unchanged
 * rows keep their identity and the result is `previous` itself when nothing changed. Only plain data
 * is compared; a property holding `undefined` counts as absent.
 */
export function shareStructure<T>(previous: unknown, next: T): T {
  if (Object.is(previous, next)) return next
  if (Array.isArray(next)) {
    if (!Array.isArray(previous)) return next
    let same = previous.length === next.length
    const items = next.map((item, index) => {
      const shared = shareStructure(previous[index], item)
      if (shared !== previous[index]) same = false
      return shared
    })
    return (same ? previous : items) as T
  }
  if (!isPlainObject(next) || !isPlainObject(previous)) return next
  let same = true
  const shared: Record<string, unknown> = {}
  for (const key of Object.keys(next)) {
    const item = next[key]
    if (item === undefined) { if (previous[key] !== undefined) same = false; shared[key] = undefined; continue }
    const kept = shareStructure(previous[key], item)
    if (kept !== previous[key]) same = false
    shared[key] = kept
  }
  if (same) for (const key of Object.keys(previous)) if (previous[key] !== undefined && !(key in shared)) { same = false; break }
  return (same ? previous : shared) as T
}

export function unavailableBackup(message: string): BackupState {
  return { snapshotId: `unavailable:${message}`, availability: "unavailable", availabilityMessage: message, requiredSpaceGB: 0, archives: [], operation: null }
}

/** Runtime lifecycle activity ids, bare or wrapped by the remote activity prefix. */
export function isLifecycleActivity(id: string) {
  return id.startsWith("lifecycle-") || /^silo-remote-activity:[^:]*:lifecycle-/.test(id)
}

/** Defers a state read while the owning device changes computer configuration. */
export function isUpdateInProgress(cause: unknown) {
  return hasBridgeErrorCode(cause, "update_in_progress")
}

/** Identity of one repository's push across native results, pending pushes and dismissals. */
export function pushKey(computer: string, repositoryPath: string) { return `${computer}\u0000${repositoryPath}` }

export function computerOwner(target: string) { return parseRemoteComputerTarget(target)?.deviceId ?? "" }

// A cancelled start, stop or restart is recorded by the runtime (D-26) as a
// lifecycle activity with `cancelled: true`, not as a failure; older devices
// still report it as "<Action> failed: … was cancelled.". Both render as the
// neutral cancelled state the action itself shows, after a reload or on a peer.
export function lastCancelledLifecycle(activities: ApplicationActivity[]) {
  const latest = new Map<string, ApplicationActivity>()
  for (const activity of activities) {
    if (activity.category !== "computer" || !activity.computer || !isLifecycleActivity(activity.id)) continue
    const known = latest.get(activity.computer)
    if (!known || activity.occurredAt > known.occurredAt) latest.set(activity.computer, activity)
  }
  const cancelled = new Map<string, LifecycleAction>()
  for (const [target, activity] of latest) {
    const action = activity.cancelled ? lifecycleActions.find(candidate => cancelledActionLabel(candidate) === activity.title) : undefined
    if (action) cancelled.set(target, action)
  }
  return cancelled
}

export function reportedCancellation(computer: ApplicationComputer, cancelledAction: LifecycleAction | undefined): Partial<ApplicationComputer> {
  if (computer.lifecycleFailure) return {}
  return cancelledAction ? { lifecycleFailure: "The action was cancelled.", lifecycleFailureAction: cancelledAction, lifecycleFailureCancelled: true } : {}
}

/** Stable identity of an export/import result: the runtime's operation id, else its defining fields. */
export function backupResultKey(state: BackupState, operation: BackupOperation) {
  if (state.operationId) return `operation:${state.operationId}`
  return JSON.stringify([operation.operation, operation.kind, operation.archive.archivePath, operation.archive.name, operation.targetName ?? null, operation.kind === "result" ? operation.outcome : null, operation.kind === "result" ? operation.title : null])
}

export function derivePorts(row: NetworkState["computers"][number] | undefined, reachable: boolean): ApplicationPort[] {
  return (row?.ports ?? []).map(port => ({ port: port.port, listening: reachable && !row?.error && port.state === "reachable", hostPort: port.hostPort, scheme: port.scheme, configured: port.configured, host: row?.host }))
}

export function backupFailure(operation: "backup" | "restore", archive: BackupArchive, message: string, targetName?: string): BackupOperation {
  return { operation, archive, runningNames: [], targetName, kind: "result", outcome: "failed", title: `${operation === "backup" ? "Export" : "Import"} failed`, message, detail: "No successful result was recorded." }
}
