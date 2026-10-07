import type { NoticeComputer } from "@/desktop/notices"
import { errorMessage } from "@/lib/error-message"
import { showOperationFailure, showOperationProgress, showOperationSuccess, type OperationStep } from "@/lib/operation-toast"
import { computerTarget } from "./connections"
import type { OperationQueue } from "./operation-queue"
import type { ApplicationComputer } from "./application-source"
import type { ComputerCheckpointOperation } from "./checkpoint-source"

/**
 * One progress notification per checkpoint operation (create, fork, restore): the popover that
 * asked closes at once, the work continues here under a stable id, and the same toast turns into
 * the success (stays until closed) or failure (stays, with Retry) message.
 */
export interface CheckpointOperationSpec {
  id: string
  kind: ComputerCheckpointOperation["kind"]
  /** The computer the backend reports the operation on (`computerTarget`). */
  target: string
  /** Computer name(s) the notification concerns, so it is dismissed if that computer is deleted. */
  computer?: string | string[]
  /** The computer the system notification names and opens. */
  noticeComputer?: NoticeComputer
  title: string
  run: () => Promise<void>
  success: { title: string; description?: string; action?: { label: string; onClick: () => void } }
  failureTitle: string
}

const active = new Map<string, { spec: CheckpointOperationSpec; startedAt: number }>()

/** Extra context for the running notification: the gate queue and how to cancel one of its entries. */
export interface CheckpointProgressContext {
  queue?: OperationQueue
  cancel?: (id: number) => void
}

const INITIAL_STEP: Record<CheckpointOperationSpec["kind"], string> = {
  capture: "Saving disk copies",
  restore: "Saving a recovery checkpoint",
  fork: "Copying from the checkpoint",
  delete: "Removing saved data",
}

const RESTORE_STEPS = ["Save recovery checkpoint", "Restore disks", "Verify"]

/** Backend stages for restore are "Creating recovery checkpoint" then "Replacing computer generation". */
function restoreSteps(stage: string | undefined): OperationStep[] {
  const current = stage && /replac/i.test(stage) ? 1 : 0
  return RESTORE_STEPS.map((label, index) => ({ label, state: index < current ? "done" : index === current ? "current" : "pending" }))
}

function show(entry: { spec: CheckpointOperationSpec; startedAt: number }, stage?: string, context?: CheckpointProgressContext, computerId?: string) {
  const { spec, startedAt } = entry
  // The queue toast hides checkpoint capture, so this notification carries its Cancel.
  const capture = spec.kind === "capture" && context?.cancel && context.queue && computerId
    ? context.queue.running.find(item => item.kind === "checkpointCapture" && item.cancellable && item.computerId === computerId)
    : undefined
  showOperationProgress(spec.id, {
    title: spec.title,
    step: stage ?? INITIAL_STEP[spec.kind],
    steps: spec.kind === "restore" ? restoreSteps(stage) : undefined,
    startedAt,
    computer: spec.computer,
    cancel: capture && context?.cancel ? { onCancel: () => context.cancel!(capture.id) } : undefined,
  })
}

export async function runCheckpointOperation(spec: CheckpointOperationSpec): Promise<boolean> {
  for (const entry of active.values()) if (entry.spec.target === spec.target) return false
  const entry = { spec, startedAt: Date.now() }
  active.set(spec.id, entry)
  show(entry)
  try {
    await spec.run()
    active.delete(spec.id)
    showOperationSuccess(spec.id, spec.success.title, { description: spec.success.description, action: spec.success.action, computer: spec.computer, noticeComputer: spec.noticeComputer })
    return true
  } catch (cause) {
    active.delete(spec.id)
    showOperationFailure(spec.id, spec.failureTitle, { description: errorMessage(cause), retry: () => { void runCheckpointOperation(spec) }, computer: spec.computer, noticeComputer: spec.noticeComputer })
    return false
  }
}

/** Refine running notifications with the backend's stage. Call whenever computers or the queue change. */
export function syncCheckpointProgress(computers: ApplicationComputer[], context?: CheckpointProgressContext) {
  if (active.size === 0) return
  for (const entry of active.values()) {
    const computer = computers.find(candidate => computerTarget(candidate) === entry.spec.target)
    const operation = computer?.checkpointOperation
    if (operation?.status === "running" && operation.kind === entry.spec.kind) show(entry, operation.stage, context, computer && !computer.device ? computer.configuration.id : undefined)
  }
}
