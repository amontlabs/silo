import type { ApplicationComputer } from "@/features/application/model/application-source"
import type { ComputerIconState, ComputerRowTone } from "../components/computer-list"

export function computerIconState(computer?: ApplicationComputer): ComputerIconState {
  if (computer?.state === "failed") return "error"
  return computer?.attention?.level ?? "normal"
}

/**
 * Why a computer cannot be edited or deleted right now. The runtime edits only running,
 * stopped or created computers and deletes only stopped, created or crashed ones, so a computer that is
 * starting, stopping or restarting would be offered the action and then rejected.
 * A checkpoint also owns the computer until its operation finishes. Stale status cannot
 * establish whether a resource edit needs confirmation to stop the computer.
 */
export function computerBusyReason(computer?: ApplicationComputer): string | undefined {
  if (!computer) return undefined
  if (computer.checkpointOperation?.status === "running") return "Wait for the checkpoint to finish."
  const action = computer.lifecycleAction === "dismiss-error" ? undefined : computer.lifecycleAction
  if (computer.state === "starting" && computer.stateDetail === "Stopping") return `Wait until ${computer.configuration.name} finishes stopping.`
  if (computer.state === "starting" || action === "start") return `Wait until ${computer.configuration.name} finishes starting.`
  if (action === "stop") return `Wait until ${computer.configuration.name} finishes stopping.`
  if (action === "restart") return `Wait until ${computer.configuration.name} finishes restarting.`
  if (computer.freshness === "stale") return computer.device && !computer.device.connected
    ? `${computer.device.name} is offline. Reconnect it to manage this computer.`
    : "Silo could not refresh this computer’s status."
  return undefined
}

export function computerRowTone(computer?: ApplicationComputer): ComputerRowTone {
  if (computer?.state === "failed" || computer?.attention?.level === "error") return "error"
  if (computer?.attention?.level === "warning") return "warning"
  // A start in progress reads as starting even while the reported state is still stopped.
  if (computer?.lifecycleAction === "start" || computer?.lifecycleAction === "restart") return "starting"
  return computer?.state ?? "stopped"
}

