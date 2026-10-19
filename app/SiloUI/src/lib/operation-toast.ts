import { createElement, useEffect, useRef, type ReactNode, type MouseEvent } from "react"
import { toast } from "sonner"

import { errorMessage } from "@/lib/error-message"
import { deliverNotice, type Notice, type NoticeComputer } from "@/desktop/notices"
import { OperationToastBody, type OperationProgressOptions } from "@/components/operation-toast-body"

export type { OperationCancel, OperationProgressOptions, OperationStep } from "@/components/operation-toast-body"

/**
 * When to use what: ask in a popover (components/confirm-popover.tsx), do in a progress
 * toast (this file). Popovers close immediately on confirm; the work continues in a toast
 * with the same stable `id` (progress → success/failure replace each other in place).
 * Dialogs are reserved for the ⌘K palette and the quit overlay.
 *
 *   showOperationProgress(id, { title, step, steps, progress, startedAt, cancel })
 *   showOperationSuccess(id, title, { action })   // stays until closed when it has an action or
 *                                                 // followed a progress toast shown > 3 s; else 4 s
 *   showOperationFailure(id, title, { retry })    // stays, with Retry
 *   useOperationProgressToast(id, state)          // for backend-driven operation state
 *
 */
/**
 * One consistent notification lifecycle for background actions:
 * loading → success (stays until the user closes it) or failure (stays, with Retry).
 * Use a stable `id` per operation so each phase replaces the previous toast in place.
 */
interface OperationToastCopy {
  /** Shown while the action runs, e.g. "Pushing 2 commits". */
  loading: string
  /** Shown when it finishes, e.g. "Pushed 2 commits". */
  success: string
  /** Title when it fails, e.g. "Push failed". The error message becomes the description. */
  failure: string
  /** Optional secondary line for the loading and success phases. */
  description?: string
}

interface OperationToastOptions {
  /** Called by the failure toast's Retry action. Omit when a retry makes no sense. */
  retry?: () => void
  /** Extra success action, e.g. { label: "Show in Finder", onClick }. */
  successAction?: { label: string; onClick: () => void }
  /** See `OperationResultOptions.native`. */
  native?: boolean
  /** See `OperationResultOptions.noticeComputer`. */
  noticeComputer?: NoticeComputer
}

/**
 * A result-toast action. Always a label + handler so every notification renders the same
 * Sonner action button (never a hand-made `<Button>`, which would look different).
 */
type OperationAction = { label: string; onClick: (event: MouseEvent<HTMLButtonElement>) => void }

/**
 * Notifications about a specific computer, so they can be dismissed when it is deleted (their
 * actions would point at a computer that no longer exists). Current computer targets by toast ID. Remote targets include their device and computer IDs.
 */
const toastComputers = new Map<string, { targets: Set<string>; computerId?: string }>()

function tagComputer(id: string, computer: string | string[] | undefined, computerId?: string) {
  const owner = { targets: new Set(computer ? Array.isArray(computer) ? computer : [computer] : []), computerId }
  if (owner.targets.size || computerId) toastComputers.set(id, owner)
  else toastComputers.delete(id)
  return () => {
    if (toastComputers.get(id) === owner) toastComputers.delete(id)
  }
}

function resultCallbacks(id: string, computer: string | string[] | undefined, onDismiss?: () => void, action?: OperationAction, computerId?: string) {
  const untag = tagComputer(id, computer, computerId)
  let closed = false
  const close = () => {
    if (closed) return
    closed = true
    untag()
    onDismiss?.()
  }
  return {
    onDismiss: close,
    onAutoClose: close,
    action: action ? { ...action, onClick: (event: MouseEvent<HTMLButtonElement>) => {
      action.onClick(event)
      if (!event.defaultPrevented) close()
    } } : undefined,
  }
}

/** Dismiss every notification tagged with this computer target. Call when the computer is deleted. */
export function dismissComputerToasts(target: string) {
  for (const [id, owner] of toastComputers) {
    if (owner.targets.has(target)) dismissOperationToast(id)
  }
}

/** Dismiss notifications belonging to this computer incarnation, across names and devices. */
export function dismissComputerToastsById(computerId: string) {
  for (const [id, owner] of toastComputers) {
    if (owner.computerId === computerId) dismissOperationToast(id)
  }
}

/** Auto-dismiss delay of a quick confirmation (nothing to act on, finished fast). */
const QUICK_TOAST_DURATION = 4000
/** A progress notification visible at least this long makes its success notification persistent. */
export const LONG_OPERATION_MS = 3000

/** When each in-flight operation's first progress notification was shown, by id. */
const progressStarts = new Map<string, number>()

/** Options shared by every finished-state notification. */
interface OperationResultOptions {
  description?: ReactNode
  action?: OperationAction
  /** Computer this notification is about; see `dismissComputerToasts`. */
  computer?: string | string[]
  /** Called when the user closes the notification or it closes by itself. */
  onDismiss?: () => void
  /**
   * Success only. Stays until closed (true) or auto-dismisses after 4 s (false). Default:
   * stays when it has an action or the operation showed progress for over 3 s.
   */
  persist?: boolean
  /**
   * Mirror this result to the system (default true), which the backend shows only while
   * Silo is in the background. Failures always mirror; a success mirrors only after a
   * progress notification shown for over 3 s. Pass false when the backend already sends
   * the system notification (computer lifecycle, export/import, setup), or when the failure
   * is not the result of background work (a validation message, a cancelled file dialog).
   */
  native?: boolean
  /** The computer the system notification is about; it names the computer and opens it on click. */
  noticeComputer?: NoticeComputer
}

function mirror(category: Notice["category"], key: string, title: string, options: Pick<OperationResultOptions, "description" | "native" | "noticeComputer">) {
  if (options.native === false) return
  deliverNotice({ category, key, title, body: typeof options.description === "string" ? options.description : "", computer: options.noticeComputer ?? null })
}

export function showOperationSuccess(id: string, title: string, options: OperationResultOptions = {}) {
  const callbacks = resultCallbacks(id, options.computer, options.onDismiss, options.action, options.noticeComputer?.id)
  const started = progressStarts.get(id)
  progressStarts.delete(id)
  const long = started !== undefined && Date.now() - started > LONG_OPERATION_MS
  const persist = options.persist ?? (Boolean(options.action) || long)
  toast.success(title, { id, description: options.description, duration: persist ? Infinity : QUICK_TOAST_DURATION, closeButton: true, ...callbacks })
  if (long) mirror("completions", id, title, options)
}

export function showOperationFailure(id: string, title: string, options: OperationResultOptions & { retry?: () => void; tone?: "error" | "warning" } = {}) {
  const action = options.action ?? (options.retry ? {
    label: "Retry",
    onClick: (event: MouseEvent<HTMLButtonElement>) => { event.preventDefault(); options.retry?.() },
  } : undefined)
  const callbacks = resultCallbacks(id, options.computer, options.onDismiss, action, options.noticeComputer?.id)
  progressStarts.delete(id)
  const notify = options.tone === "warning" ? toast.warning : toast.error
  notify(title, {
    id,
    description: options.description,
    duration: Infinity,
    closeButton: true,
    ...callbacks,
  })
  mirror("failures", id, title, options)
}

/** Close a notification (e.g. when the state it reported has gone away). */
export function dismissOperationToast(id: string) {
  toastComputers.delete(id)
  progressStarts.delete(id)
  toast.dismiss(id)
}

/** A neutral, short-lived notice for an outcome that is neither success nor failure (e.g. cancelled). */
export function showOperationNotice(id: string, title: string, options: { description?: ReactNode; onDismiss?: () => void; duration?: number; computer?: string | string[] } = {}) {
  const callbacks = resultCallbacks(id, options.computer, options.onDismiss)
  toast(title, { id, description: options.description, duration: options.duration ?? 4000, closeButton: true, ...callbacks })
}

/**
 * Rich progress toast: progress bar (determinate when `progress` is 0–1, else indeterminate),
 * current step, optional step list, elapsed time and an optional Cancel (with in-toast confirm).
 * Call again with the same id to update in place.
 */
export function showOperationProgress(id: string, options: OperationProgressOptions) {
  const { title, computer, ...body } = options
  const untag = tagComputer(id, computer)
  if (!progressStarts.has(id)) progressStarts.set(id, options.startedAt ?? Date.now())
  toast.loading(title, { id, duration: Infinity, action: undefined, description: createElement(OperationToastBody, { ...body, title }), onDismiss: untag })
}

/** Backend-driven operation state understood by `useOperationProgressToast`. */
export type OperationProgressState =
  | { status: "idle" }
  | ({ status: "running" } & OperationProgressOptions)
  | ({ status: "success"; title: string } & OperationResultOptions)
  | { status: "failure"; title: string; description?: ReactNode; retry?: () => void; onDismiss?: () => void; computer?: string | string[]; native?: boolean; noticeComputer?: NoticeComputer }

/**
 * Maps an operation state to progress/success/failure toasts under `id`. A state that is
 * already finished when the component mounts is ignored (no stale toast); running states
 * update in place; going back to idle dismisses a progress toast this hook showed.
 */
export function useOperationProgressToast(id: string, state: OperationProgressState) {
  const previous = useRef<OperationProgressState["status"] | null>(null)
  useEffect(() => {
    const before = previous.current
    previous.current = state.status
    if (state.status === "running") {
      const { status: _status, ...options } = state
      showOperationProgress(id, options)
    } else if (before === null) {
      return
    } else if (state.status === "success" && before !== "success") {
      showOperationSuccess(id, state.title, { description: state.description, action: state.action, computer: state.computer, onDismiss: state.onDismiss, persist: state.persist, native: state.native, noticeComputer: state.noticeComputer })
    } else if (state.status === "failure" && before !== "failure") {
      showOperationFailure(id, state.title, { description: state.description, retry: state.retry, computer: state.computer, onDismiss: state.onDismiss, native: state.native, noticeComputer: state.noticeComputer })
    } else if (state.status === "idle" && before === "running") {
      toast.dismiss(id)
    }
  }, [id, state])
}

/** Run a user-initiated action with the standard loading → success/failure notifications. */
export async function runWithOperationToast<T>(id: string, copy: OperationToastCopy, action: () => Promise<T>, options: OperationToastOptions = {}): Promise<T | undefined> {
  showOperationProgress(id, { title: copy.loading, step: copy.description, progress: null })
  try {
    const result = await action()
    showOperationSuccess(id, copy.success, { description: copy.description, action: options.successAction, native: options.native, noticeComputer: options.noticeComputer })
    return result
  } catch (error) {
    showOperationFailure(id, copy.failure, { description: errorMessage(error), retry: options.retry, native: options.native, noticeComputer: options.noticeComputer })
    return undefined
  }
}

/** A short confirmation for instant actions (copy, open). Auto-dismisses after 4 s. */
export function showQuickConfirmation(title: string, description?: string) {
  toast.success(title, { description, duration: QUICK_TOAST_DURATION, closeButton: true })
}

/**
 * A standalone failure for an instant action that failed (no loading phase). Stays until
 * closed. Its key derives from the title, so a repeat replaces the earlier toast and system
 * notification. Pass `native: false` for a failure that is not the result of background
 * work (a cancelled file dialog, a validation message).
 */
export function showActionFailure(title: string, error: unknown, retry?: () => void, options: { id?: string; native?: boolean; noticeComputer?: NoticeComputer } = {}) {
  const id = options.id ?? `action-failure:${title}`
  const description = errorMessage(error)
  toast.error(title, {
    id,
    description,
    duration: Infinity,
    closeButton: true,
    action: retry ? { label: "Retry", onClick: retry } : undefined,
  })
  mirror("failures", id, title, { description, ...options })
}
