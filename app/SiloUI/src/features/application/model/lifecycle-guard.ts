import { showActionFailure, showOperationFailure } from "@/lib/operation-toast"

import type { ApplicationActions, ApplicationSource, ApplicationComputer } from "./application-source"
import { computerTarget } from "./connections"
import { computerAvailability } from "./computer-availability"

export type LifecycleAction = "start" | "stop" | "restart"

/** A question to ask before a lifecycle action runs. */
export interface LifecyclePrompt {
  title: string
  description: string
  confirmLabel: string
  tone: "default" | "destructive"
}

export type LifecycleCheck =
  | { kind: "ready" }
  | { kind: "confirm"; prompt: LifecyclePrompt }
  | { kind: "unavailable"; title: string; message: string }

type LifecycleActions = Pick<ApplicationActions, "startComputer" | "stopComputer" | "restartComputer">

function computerName(computer: ApplicationComputer) {
  return computer.device ? `${computer.configuration.name} on ${computer.device.name}` : computer.configuration.name
}

/**
 * The guards every surface applies to a lifecycle request (list row, computer page, command
 * palette, toasts, and the status panel): local computer operations can be unavailable in this
 * build, starting a computer under memory pressure asks first, and stopping or restarting a
 * running computer always asks first (decision 8). Start and Open never ask otherwise.
 */
export function lifecycleCheck(source: ApplicationSource, computer: ApplicationComputer, action: LifecycleAction): LifecycleCheck {
  const local = !computer.device
  if (local && source.computerOperationsUnavailable) return { kind: "unavailable", title: "Computer operation unavailable", message: source.computerOperationsUnavailable }
  const notice = source.resourceNotice
  if (action === "start" && local && notice?.kind === "start-memory" && notice.computer === computer.configuration.name) {
    return { kind: "confirm", prompt: {
      title: `Starting ${computer.configuration.name} may slow this device`,
      description: `Silo found high memory pressure now. This computer can use up to ${notice.memoryGiB} GiB. Close memory-heavy apps, or start anyway.`,
      confirmLabel: "Start anyway",
      tone: "default",
    } }
  }
  if (action !== "start" && computer.state === "running") return { kind: "confirm", prompt: interruptionPrompt(computer, action) }
  return { kind: "ready" }
}

/** The tray's inline Stop/Restart confirmation, in words every surface shares. */
export function interruptionPrompt(computer: ApplicationComputer, action: "stop" | "restart"): LifecyclePrompt {
  const label = action === "stop" ? "Stop" : "Restart"
  const description = action === "stop"
    ? "Running processes will be interrupted. Files in /workspace are kept."
    : "Running processes will be interrupted, then the computer starts again. Files in /workspace are kept."
  return { title: `${label} ${computerName(computer)}?`, description, confirmLabel: label, tone: "destructive" }
}

/** Sends the action to the computer's own device. */
export function runLifecycle(actions: LifecycleActions, computer: ApplicationComputer, action: LifecycleAction) {
  const target = computerTarget(computer)
  if (action === "start") actions.startComputer(target)
  else if (action === "stop") actions.stopComputer(target)
  else actions.restartComputer(target)
}

export interface LifecyclePresenters {
  /** Reports a request that cannot run. Defaults to a failure notification. */
  notify?: (title: string, message: string) => void
  /** Asks a prompt where the surface has no confirmation of its own; `confirm` proceeds.
   * Defaults to a warning notification with the confirm action. */
  prompt?: (prompt: LifecyclePrompt, confirm: () => void, computer: ApplicationComputer) => void
}

export interface LifecycleGuard {
  check: (computer: ApplicationComputer, action: LifecycleAction) => LifecycleCheck
  /** A new request: reports why it can't run, asks its prompt, or runs it. */
  request: (computer: ApplicationComputer, action: LifecycleAction) => void
  /** Runs a request the user already confirmed, or re-submits one (Retry): prompts are skipped,
   * but a request that can no longer run is still reported instead. */
  confirm: (computer: ApplicationComputer, action: LifecycleAction) => void
}

const verbs: Record<LifecycleAction, string> = { start: "start", stop: "stop", restart: "restart" }

/** The shared guarded lifecycle layer. Pages, the palette and the status panel call this
 * instead of the raw start/stop/restart actions. */
export function lifecycleGuard(source: ApplicationSource, actions: LifecycleActions, presenters: LifecyclePresenters = {}): LifecycleGuard {
  const notify = presenters.notify ?? ((title: string, message: string) => showActionFailure(title, message, undefined, { native: false }))
  const prompt = presenters.prompt ?? ((question: LifecyclePrompt, confirm: () => void, computer: ApplicationComputer) => {
    showOperationFailure(`lifecycle-prompt:${computerTarget(computer)}`, question.title, {
      description: question.description, tone: "warning", native: false, computer: computer.configuration.name,
      action: { label: question.confirmLabel, onClick: confirm },
    })
  })
  function blocked(computer: ApplicationComputer, action: LifecycleAction): LifecycleCheck | undefined {
    const check = lifecycleCheck(source, computer, action)
    if (check.kind === "unavailable") return check
    // The computer may have changed since the control was shown (or the prompt was asked).
    const availability = computerAvailability(computer, source)
    const allowed = action === "start" ? availability.canStart : action === "stop" ? availability.canStop : availability.canRestart
    const reason = availability.reasons[action]
    if (!allowed) return { kind: "unavailable", title: `Could not ${verbs[action]} ${computerName(computer)}`, message: reason ?? "It is busy." }
    return undefined
  }
  const guard: LifecycleGuard = {
    check: (computer, action) => lifecycleCheck(source, computer, action),
    request(computer, action) {
      const unavailable = blocked(computer, action)
      if (unavailable?.kind === "unavailable") { notify(unavailable.title, unavailable.message); return }
      const check = lifecycleCheck(source, computer, action)
      if (check.kind === "confirm") prompt(check.prompt, () => guard.confirm(computer, action), computer)
      else runLifecycle(actions, computer, action)
    },
    confirm(computer, action) {
      const unavailable = blocked(computer, action)
      if (unavailable?.kind === "unavailable") { notify(unavailable.title, unavailable.message); return }
      runLifecycle(actions, computer, action)
    },
  }
  return guard
}
