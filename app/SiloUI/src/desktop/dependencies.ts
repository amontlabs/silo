import { invoke } from "@tauri-apps/api/core"
import { useSyncExternalStore } from "react"
import { z } from "zod"

import { siloPreflightCheckSchema, type SiloPreflightCheck } from "@/contracts/silo"

type InvokeDependencyChecks = (command: "read_dependencies", arguments_: { requestId: string }) => Promise<unknown>
const invokeDependencyChecks: InvokeDependencyChecks = (command, arguments_) => invoke(command, arguments_)

const requiredCheckIDs = ["system-os", "system-virtualization", "runtime-microsandbox", "tool-git", "tool-git-lfs"] as const
const nativeCheckSchema = siloPreflightCheckSchema.extend({
  status: z.enum(["pass", "failed", "unavailable", "timeout"]),
}).strict()
const reportSchema = z.object({
  schemaVersion: z.literal(1),
  requestId: z.string().min(1),
  checkedAtMs: z.number().int().nonnegative(),
  checks: z.array(nativeCheckSchema).length(requiredCheckIDs.length),
}).strict()

export interface DependencyRuntime {
  checks: SiloPreflightCheck[]
  retry: () => void
}

export interface DependencyStore {
  getSnapshot: () => SiloPreflightCheck[]
  subscribe: (listener: () => void) => () => void
  retry: () => void
  dispose: () => void
}

const pendingChecks: SiloPreflightCheck[] = [
  { id: "system-os", title: "Supported OS", status: "pending", detail: "Checking the operating system and build architecture…", remediation: null },
  { id: "system-virtualization", title: "Virtualization", status: "pending", detail: "Waiting for the operating system check…", remediation: null },
  { id: "runtime-microsandbox", title: "Computer runtime", status: "pending", detail: "Checking the bundled computer runtime…", remediation: null },
  { id: "tool-git", title: "Git", status: "pending", detail: "Checking bundled Git…", remediation: null },
  { id: "tool-git-lfs", title: "Git LFS", status: "pending", detail: "Checking bundled Git LFS…", remediation: null },
]

function exceptionalChecks(status: "unavailable" | "timeout", detail: string): SiloPreflightCheck[] {
  return requiredCheckIDs.map((id) => ({
    id,
    title: pendingChecks.find((check) => check.id === id)!.title,
    status,
    detail,
    remediation: "Retry checks. If this keeps happening, quit and reopen Silo, then retry.",
  }))
}

export function createNativeDependencyStore(invokeChecks: InvokeDependencyChecks = invokeDependencyChecks): DependencyStore {
  let snapshot = pendingChecks
  /** The request whose result may still be published. */
  let activeRequest: string | null = null
  /** The native call in flight; null once it settles or its watchdog abandons it. */
  let activeInvocation: string | null = null
  let retryQueued = false
  let disposed = false
  let activeTimeout: ReturnType<typeof globalThis.setTimeout> | undefined
  const listeners = new Set<() => void>()
  const clearWatchdog = () => {
    if (activeTimeout !== undefined) globalThis.clearTimeout(activeTimeout)
    activeTimeout = undefined
  }
  const armWatchdog = (onTimeout: () => void) => {
    clearWatchdog()
    activeTimeout = globalThis.setTimeout(() => {
      activeTimeout = undefined
      onTimeout()
    }, 60_000)
  }
  const publish = (checks: SiloPreflightCheck[]) => {
    if (disposed) return
    snapshot = checks
    for (const listener of listeners) listener()
  }
  function startRequest() {
    if (disposed) return
    const requestId = `${Date.now()}-${Math.random().toString(36).slice(2)}`
    activeRequest = requestId
    activeInvocation = requestId
    armWatchdog(() => {
      // A call that never settles must not block later checks: abandon it (its late
      // result is ignored by request id) so the next Retry starts a new request.
      if (activeInvocation === requestId) activeInvocation = null
      if (activeRequest !== requestId) return
      activeRequest = null
      publish(exceptionalChecks("timeout", "Dependency checks timed out. No successful result was recorded."))
    })
    void Promise.resolve().then(() => {
      if (disposed) return
      return invokeChecks("read_dependencies", { requestId })
    }).then((input) => {
      if (activeRequest !== requestId) return
      try { publish(validateDependencyReport(input, requestId)) }
      catch { publish(exceptionalChecks("unavailable", "Silo returned a malformed or stale dependency result.")) }
    }).catch(() => {
      if (activeRequest === requestId) publish(exceptionalChecks("unavailable", "Dependency checks are unavailable because the desktop bridge did not respond."))
    }).finally(() => {
      // An abandoned call settling late owns neither the watchdog nor the queue.
      if (activeInvocation !== requestId) return
      clearWatchdog()
      activeInvocation = null
      if (!disposed && retryQueued) {
        retryQueued = false
        startRequest()
      }
    })
  }
  function retry() {
    if (disposed) return
    publish(pendingChecks.map((check) => ({ ...check })))
    const inFlight = activeInvocation
    if (inFlight) {
      activeRequest = null
      retryQueued = true
      armWatchdog(() => {
        if (activeInvocation === inFlight) activeInvocation = null
        if (!retryQueued) return
        retryQueued = false
        publish(exceptionalChecks("timeout", "Dependency checks timed out. No successful result was recorded."))
      })
      return
    }
    startRequest()
  }
  return {
    getSnapshot: () => snapshot,
    subscribe(listener) {
      if (disposed) return () => {}
      listeners.add(listener)
      return () => listeners.delete(listener)
    },
    retry,
    dispose() {
      disposed = true
      activeRequest = null
      retryQueued = false
      clearWatchdog()
      listeners.clear()
    },
  }
}

export function useDependencyStore(store: DependencyStore | null): DependencyRuntime | null {
  const checks = useSyncExternalStore(store?.subscribe ?? (() => () => {}), store?.getSnapshot ?? (() => pendingChecks))
  return store ? { checks, retry: store.retry } : null
}

export function validateDependencyReport(input: unknown, requestId: string, now = Date.now()): SiloPreflightCheck[] {
  const report = reportSchema.parse(input)
  if (report.requestId !== requestId) throw new Error("Dependency checks returned a stale request")
  if (Math.abs(now - report.checkedAtMs) > 30_000) throw new Error("Dependency checks returned a stale result")
  const ids = report.checks.map(({ id }) => id)
  if (new Set(ids).size !== requiredCheckIDs.length || requiredCheckIDs.some((id) => !ids.includes(id))) {
    throw new Error("Dependency checks returned an incomplete result")
  }
  return report.checks
}
