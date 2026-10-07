import { useEffect, useState } from "react"
import { X } from "lucide-react"

import { dismissOperationToast, showOperationProgress } from "@/lib/operation-toast"
import { useClock } from "@/lib/use-clock"

import {
  emptyOperationQueue,
  isOperationStuck,
  toastableQueue,
  waitingOperationForVm,
  waitingStatusText,
  type OperationEntry,
  type OperationQueue,
} from "@/features/application/model/operation-queue"

/** Stable id so the queue always updates one toast in place rather than stacking them. */
const OPERATION_QUEUE_TOAST_ID = "operation-queue"
/** Quick operations never flash: the toast appears only once an entry has run this long. */
const TOAST_DEBOUNCE_MS = 500

/** Small inline Cancel control for a queued operation shown near its computer row. */
function CancelOperationButton({ entry, onCancel }: { entry: OperationEntry; onCancel: (id: number) => void }) {
  return (
    <button
      type="button"
      aria-label={`Cancel ${entry.label}`}
      title={`Cancel ${entry.label}`}
      onClick={() => onCancel(entry.id)}
      className="flex shrink-0 items-center gap-1 rounded px-1 text-muted-foreground hover:text-foreground focus-ring"
    >
      <X aria-hidden="true" className="size-3" />
      Cancel
    </button>
  )
}

/** Step line for the operation-queue toast: a stuck warning, else a summary of waiting entries. */
function queueStep(queue: OperationQueue, now: number, full: OperationQueue = queue): string | undefined {
  if (queue.running.some((entry) => isOperationStuck(entry, now))) return "Taking longer than expected"
  const first = queue.waiting[0]
  if (!first) return undefined
  const more = queue.waiting.length - 1
  return `${first.label} — ${waitingStatusText(full, first)}${more > 0 ? ` (and ${more} more)` : ""}`
}

/**
 * Drives a single Sonner toast reflecting the computer-changing operation queue. Renders nothing
 * itself. Export and import already have their own transfer toast, so their entries are
 * excluded here. The toast is debounced so operations that finish within {@link
 * TOAST_DEBOUNCE_MS} never flash, and it is dismissed as soon as the queue empties.
 */
export function OperationQueueToast({ queue, onCancel }: { queue?: OperationQueue; onCancel?: (id: number) => void }) {
  const visibleQueue = queue ? toastableQueue(queue) : emptyOperationQueue
  const running = visibleQueue.running
  const waiting = visibleQueue.waiting
  const active = running.length + waiting.length > 0
  // The earliest entry decides the debounce: once *something* has been active long enough,
  // the whole toast may appear. Stable across renders while the same entry stays oldest.
  const earliest = active ? Math.min(...[...running, ...waiting].map((entry) => entry.sinceMs)) : 0
  const [debouncedSince, setDebouncedSince] = useState<number>()
  const show = active && debouncedSince === earliest
  const now = useClock(show)

  useEffect(() => {
    if (!active) {
      setDebouncedSince(undefined)
      return
    }
    const remaining = TOAST_DEBOUNCE_MS - (Date.now() - earliest)
    if (remaining <= 0) {
      setDebouncedSince(earliest)
      return
    }
    const timer = window.setTimeout(() => setDebouncedSince(earliest), remaining)
    return () => window.clearTimeout(timer)
    // oxlint-disable-next-line react-hooks/exhaustive-deps
  }, [active, earliest])

  useEffect(() => {
    if (!show) {
      dismissOperationToast(OPERATION_QUEUE_TOAST_ID)
      return
    }
    const total = running.length + waiting.length
    const title = running.length === 1
      ? running[0].label
      : `${total} ${total === 1 ? "operation" : "operations"} in progress`
    const cancellable = running.find((entry) => entry.cancellable)
    const primary = running[0]
    // With several operations under one title, Cancel names the one it stops.
    const cancelLabel = cancellable && title !== cancellable.label ? `Cancel “${cancellable.label}”` : undefined
    showOperationProgress(OPERATION_QUEUE_TOAST_ID, {
      title,
      step: queueStep(visibleQueue, now, queue),
      startedAt: primary?.sinceMs ?? earliest,
      cancel: onCancel && cancellable ? { label: cancelLabel, onCancel: () => onCancel(cancellable.id) } : undefined,
    })
    // oxlint-disable-next-line react-hooks/exhaustive-deps
  }, [show, queue, now, onCancel])

  // Never let the toast outlive the app that drives it.
  useEffect(() => () => { dismissOperationToast(OPERATION_QUEUE_TOAST_ID) }, [])

  return null
}

/**
 * Inline per-computer waiting status shown near a computer's activity indicator when an
 * operation for that computer is waiting its turn behind other running work.
 */
export function ComputerWaitingStatus({ queue, computerId, onCancel }: { queue?: OperationQueue; computerId: string; onCancel?: (id: number) => void }) {
  if (!queue) return null
  const waiting = waitingOperationForVm(queue, computerId)
  if (!waiting) return null
  return (
    <span role="status" className="inline-flex items-center gap-1 text-muted-foreground">
      {waitingStatusText(queue, waiting)}
      {onCancel && <CancelOperationButton entry={waiting} onCancel={onCancel} />}
    </span>
  )
}
