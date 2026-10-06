import { useEffect, useEffectEvent, useLayoutEffect, useMemo, useRef } from "react"

import { ErrorDetails } from "@/components/error-details"
import { dismissOperationToast, showOperationFailure, showOperationNotice, showOperationProgress } from "@/lib/operation-toast"
import type { ApplicationActions, ApplicationSource, ApplicationComputer } from "./application-source"
import { lifecycleGuard } from "./lifecycle-guard"
import { startStepProgress } from "./lifecycle-progress"
import { cancelledActionLabel, emptyOperationQueue, waitingOperationForVm, waitingStatusText } from "./operation-queue"

const LIFECYCLE_TOAST_DELAY_MS = 800

/** The computers whose lifecycle state drives notifications. */
function lifecycleComputers(computers: ApplicationComputer[]) {
  return computers.filter(computer => computer.lifecycleAction || computer.lifecycleFailure)
}

/** Everything the lifecycle notifications read from a computer, so unrelated publishes compare equal. */
function lifecycleKey(computers: ApplicationComputer[]) {
  return JSON.stringify(computers.map(computer => [
    computer.device?.id ?? "", computer.configuration.id, computer.configuration.name, computer.lifecycleAction,
    computer.lifecycleStep, computer.lifecycleFailure, computer.lifecycleFailureAction,
    computer.lifecycleFailureCancelled, computer.lifecycleFailureDiagnostic,
  ]))
}

/** Track lifecycle notifications independently of the visible page. */
export function useLifecycleToasts(source: ApplicationSource, actions: ApplicationActions, { enabled = true, readOnly = false } = {}) {
  const relevantComputers = lifecycleComputers(source.computers)
  const relevantKey = lifecycleKey(relevantComputers)
  // oxlint-disable-next-line react-hooks/exhaustive-deps
  const lifecycleSubset = useMemo(() => relevantComputers, [relevantKey])
  const latest = useRef({ source, actions, readOnly })
  useLayoutEffect(() => { latest.current = { source, actions, readOnly } }, [source, actions, readOnly])

  function retry(computer: ApplicationComputer): (() => void) | undefined {
    const action = computer.lifecycleFailureAction ?? "start"
    if (readOnly || !computer.lifecycleFailure || action === "dismiss-error") return undefined
    return () => {
      const current = latest.current
      const fresh = current.source.computers.find(item => item.configuration.id === computer.configuration.id)
      if (fresh && !current.readOnly) lifecycleGuard(current.source, current.actions).confirm(fresh, action)
    }
  }

  // Lifecycle Start/Stop/Restart: a progress notification appears only if the action takes
  // longer than a moment (instant ones never flash) and is dismissed when it finishes; the row
  // state already shows the outcome. Failures keep their own retryable notification.
  const lifecycleProgress = useRef(new Map<string, { timer?: number; shown: boolean; startedAt: number; show: () => void }>())
  const trackLifecycle = useEffectEvent((all: ApplicationComputer[]) => {
    const tracked = lifecycleProgress.current
    const live = new Set<string>()
    for (const computer of all) {
      const action = computer.lifecycleAction
      if (!action || action === "dismiss-error") continue
      const key = `${computer.device?.id ?? ""}:${computer.configuration.id}`
      live.add(key)
      const id = `lifecycle:${key}`
      const name = computer.configuration.name
      const title = action === "restart" ? `Restarting ${name}` : action === "stop" ? `Stopping ${name}` : `Starting ${name}`
      const waiting = !computer.device ? waitingOperationForVm(source.operationQueue ?? emptyOperationQueue, computer.configuration.id) : undefined
      const starting = action === "start" || action === "restart"
      // Remote devices report no steps, so their start keeps the plain text.
      const reported = starting && !computer.device ? startStepProgress(computer.lifecycleStep) : undefined
      const step = waiting && source.operationQueue ? waitingStatusText(source.operationQueue, waiting) : reported ? reported.step : action === "restart" ? "Restarting…" : action === "stop" ? "Stopping…" : "Starting…"
      const progress = waiting ? undefined : reported?.progress
      const existing = tracked.get(key)
      const entry = existing ?? { shown: false, startedAt: Date.now(), show: () => {} } as { timer?: number; shown: boolean; startedAt: number; show: () => void }
      entry.show = () => showOperationProgress(id, { title, step, progress, startedAt: entry.startedAt, computer: name })
      if (!existing) {
        tracked.set(key, entry)
        // A start always takes a while, so its toast is immediate; a stop that is instant never flashes.
        if (starting) { entry.shown = true; entry.show() }
        else entry.timer = window.setTimeout(() => { entry.shown = true; entry.timer = undefined; entry.show() }, LIFECYCLE_TOAST_DELAY_MS)
      } else if (entry.shown) entry.show()
    }
    for (const [key, entry] of tracked) {
      if (live.has(key)) continue
      if (entry.timer) window.clearTimeout(entry.timer)
      if (entry.shown && !all.some(computer => `${computer.device?.id ?? ""}:${computer.configuration.id}` === key && computer.lifecycleFailure)) dismissOperationToast(`lifecycle:${key}`)
      tracked.delete(key)
    }
  })
  useEffect(() => { if (enabled) trackLifecycle(lifecycleSubset) }, [enabled, lifecycleSubset, source.operationQueue])
  useEffect(() => {
    const tracked = lifecycleProgress.current
    return () => {
      for (const [key, entry] of tracked) {
        if (entry.timer) window.clearTimeout(entry.timer)
        if (entry.shown) dismissOperationToast(`lifecycle:${key}`)
      }
      tracked.clear()
    }
  }, [enabled])

  // Lifecycle failures and cancellations arrive from the backend as computer state. Toast each
  // new one (both the list and the detail page render from here); failures already present at
  // first load keep only their row state label.
  const seenLifecycleFailures = useRef<Map<string, string> | null>(null)
  const lifecycleToasts = useEffectEvent((all: ApplicationComputer[]) => {
    const current = new Map<string, string>()
    for (const computer of all) {
      if (computer.lifecycleFailure) current.set(`${computer.device?.id ?? ""}:${computer.configuration.id}`, `${computer.lifecycleFailureAction ?? ""}|${computer.lifecycleFailure}`)
    }
    const previous = seenLifecycleFailures.current
    seenLifecycleFailures.current = current
    if (!previous) return
    for (const computer of all) {
      const key = `${computer.device?.id ?? ""}:${computer.configuration.id}`
      const signature = current.get(key)
      if (!signature || previous.get(key) === signature) continue
      const action = computer.lifecycleFailureAction ?? "start"
      const name = computer.configuration.name
      const id = `lifecycle:${key}`
      if (computer.lifecycleFailureCancelled) {
        showOperationNotice(id, cancelledActionLabel(action))
        continue
      }
      if (action === "dismiss-error") continue
      const verb = action === "restart" ? "restart" : action === "stop" ? "stop" : "start"
      dismissOperationToast(id)
      showOperationFailure(id, `Could not ${verb} ${name}`, { description: computer.lifecycleFailure ? <ErrorDetails message={computer.lifecycleFailure} diagnostic={computer.lifecycleFailureDiagnostic} /> : undefined, retry: retry(computer), computer: name, native: false })
    }
  })
  useEffect(() => { if (enabled) lifecycleToasts(lifecycleSubset) }, [enabled, lifecycleSubset])
}
