import { Play, RotateCw, Square } from "lucide-react"

import type { ApplicationComputer } from "@/features/application/model/application-source"
import { ComputerAction } from "@/features/computers/components/computer-list"
import type { ComputerAvailability } from "../model/computer-availability"
import { LifecycleControl } from "../components/lifecycle-control"
import type { LifecycleGuard } from "../model/lifecycle-guard"

/** The row's Start or Stop control, through the shared lifecycle guard. */
export function ComputerActions({ computer, availability, readOnly, guard }: { computer: ApplicationComputer; availability: ComputerAvailability; readOnly: boolean; guard: LifecycleGuard }) {
  const { configuration } = computer
  const action = computer.state === "running" || computer.state === "starting" ? "stop" : "start"
  const enabled = !readOnly && (action === "stop" ? availability.canStop : availability.canStart)
  const asks = enabled && guard.check(computer, action).kind === "confirm"
  return <>
    {Boolean(computer.pendingSecretRevocations?.length) && <LifecycleControl guard={guard} computer={computer} action="restart" disabled={readOnly || !availability.canRestart} reason={readOnly ? undefined : availability.reasons.restart}>
      {({ onClick, disabled }) => <ComputerAction label={`Restart ${configuration.name}`} disabled={disabled} onClick={onClick}><RotateCw /></ComputerAction>}
    </LifecycleControl>}
    <LifecycleControl guard={guard} computer={computer} action={action} disabled={!enabled} reason={readOnly ? undefined : availability.reasons[action]}>
      {({ onClick, disabled }) => action === "stop"
        ? <ComputerAction label={`Stop ${configuration.name}`} tooltip={asks ? `Stop ${configuration.name}…` : undefined} disabled={disabled} onClick={onClick}><Square /></ComputerAction>
        : <ComputerAction label={`Start ${configuration.name}`} tooltip={asks ? `Start ${configuration.name}…` : undefined} disabled={disabled} onClick={onClick}><Play /></ComputerAction>}
    </LifecycleControl>
  </>
}
