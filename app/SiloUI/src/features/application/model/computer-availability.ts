import { computerTarget } from "./connections"
import type { ApplicationSource, ApplicationComputer } from "./application-source"

export interface ComputerAvailability {
  /** Work is running on the computer (or its device); the row shows progress. */
  busy: boolean
  canOpen: boolean
  canStart: boolean
  canStop: boolean
  canRestart: boolean
  /** Why each unavailable control is disabled, for its tooltip. */
  reasons: { open?: string; start?: string; stop?: string; restart?: string }
}

const lifecycleProgress = { start: "starting", stop: "stopping", restart: "restarting", "dismiss-error": "clearing its error" } as const

/** Why nothing can be done with the computer right now, or undefined when it is free. */
function blockedReason(computer: ApplicationComputer, source: ApplicationSource): string | undefined {
  const { device, configuration } = computer
  if (!device && source.runtimeRepair) return "Resolve the system issue first."
  // A macOS computer is not part of the Linux configuration being applied.
  if (!device && !computer.macos && source.computerConfigurationOperation) {
    return source.computerConfigurationOperation.status === "failed" ? "Dismiss the computer changes error first." : "Wait for computer changes to finish."
  }
  if (computer.lifecycleAction) return `${configuration.name} is ${lifecycleProgress[computer.lifecycleAction]}.`
  if (computer.checkpointOperation?.status === "running") return "Wait for the checkpoint to finish."
  if (device?.busy) return `${device.name} is updating. Wait before changing this computer.`
  if (computer.state === "starting") return `Wait for ${configuration.name} to finish ${computer.stateDetail === "Stopping" ? "stopping" : "starting"}.`
  if (source.activities.some((activity) => activity.category === "computer" && activity.computer === computerTarget(computer) && activity.status === "running")) return "Wait for the current operation to finish."
  if (computer.freshness === "stale") return device && !device.connected ? `${device.name} is offline. Reconnect it to manage this computer.` : "Silo could not refresh this computer’s status."
  return undefined
}

/**
 * The one availability rule for a computer's Open and lifecycle controls, shared by the
 * Computers list, the computer page, the command palette and the status panel. A running
 * computer can be stopped or restarted; a stopped or crashed one started; a crashed one also
 * restarted (its documented recovery). An error notice blocks only opening the computer.
 */
export function computerAvailability(computer: ApplicationComputer, source: ApplicationSource): ComputerAvailability {
  const { state, configuration } = computer
  const busy = Boolean(computer.device?.busy) || Boolean(computer.lifecycleAction) || computer.checkpointOperation?.status === "running" || state === "starting"
    || source.activities.some((activity) => activity.category === "computer" && activity.computer === computerTarget(computer) && activity.status === "running")
  const blocked = blockedReason(computer, source)
  const error = computer.attention?.level === "error" ? computer.attention.message : undefined
  const canOpen = !blocked && !error && state === "running"
  // A macOS computer starts once it is installed and personalised, and has no restart.
  const macos = computer.macos
  // A computer whose storage could not be moved is closed to Start and Restart; its notice says why.
  const storageUpgrade = computer.needsStorageUpgrade ? computer.attention?.message ?? "This computer’s storage needs attention before it can start." : undefined
  const canStart = !blocked && !storageUpgrade && (state === "stopped" || state === "failed") && (!macos || (macos.installed && !macos.needsPersonalizing))
  const canStop = !blocked && state === "running"
  const canRestart = !blocked && !storageUpgrade && !macos && (state === "running" || state === "failed")
  const reasons: ComputerAvailability["reasons"] = {}
  if (!canOpen) reasons.open = blocked ?? error ?? `Start ${configuration.name} to open it.`
  if (!canStart) reasons.start = blocked ?? storageUpgrade ?? `${configuration.name} is already running.`
  if (!canStop) reasons.stop = blocked ?? `${configuration.name} isn’t running.`
  if (!canRestart) reasons.restart = blocked ?? storageUpgrade ?? (computer.macos ? "A macOS computer can’t be restarted. Stop it, then start it." : `${configuration.name} isn’t running.`)
  return { busy, canOpen, canStart, canStop, canRestart, reasons }
}
