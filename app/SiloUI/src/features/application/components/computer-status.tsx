import { ComputerStateLabel } from "@/features/application/components/application-ui"
import { ComputerWaitingStatus } from "@/features/application/components/operation-queue-panel"
import type { ApplicationActions, ApplicationSource, ApplicationComputer } from "@/features/application/model/application-source"
import { emptyOperationQueue, waitingOperationForVm } from "@/features/application/model/operation-queue"

type LifecycleAction = NonNullable<ApplicationComputer["lifecycleAction"]>

const lifecycleLabels: Record<LifecycleAction, string> = {
  start: "Starting…",
  stop: "Stopping…",
  restart: "Restarting…",
  "dismiss-error": "Dismissing…",
}

export const StatusSeparator = () => <span aria-hidden="true" className="mx-1">·</span>

/**
 * A computer's lifecycle-aware status, shared by its list row and its page: the state label,
 * or the pending action (or what it waits for in the operation queue), a remote device's
 * refresh, or its offline state.
 */
export function ComputerStatus({ computer, source, readOnly, onCancel }: { computer: ApplicationComputer; source: ApplicationSource; readOnly: boolean; onCancel?: ApplicationActions["cancelOperation"] }) {
  const lifecycle = computer.lifecycleAction
  // The operation gate keys local per-computer entries by the stable computer id. A remote device's computers
  // run on that device's own gate, so a remote computer never matches a local entry.
  const queueVmId = computer.device ? null : computer.configuration.id
  const waitingForVm = queueVmId !== null ? waitingOperationForVm(source.operationQueue ?? emptyOperationQueue, queueVmId) : undefined
  const cancel = readOnly ? undefined : onCancel
  if (lifecycle) {
    // Until its queue entry runs, a pending action reads "Waiting for <blocker>…".
    return waitingForVm && queueVmId !== null
      ? <ComputerWaitingStatus queue={source.operationQueue} computerId={queueVmId} onCancel={cancel} />
      : <span role="status" className="text-warning">{lifecycleLabels[lifecycle]}</span>
  }
  if (computer.device?.busy) return <span role="status">Updating…</span>
  if (computer.device && !computer.device.connected) return <span>Offline · last known status</span>
  return <span className="inline-flex items-baseline gap-1.5 align-baseline">
    <ComputerStateLabel state={computer.state} />
    {queueVmId !== null && waitingForVm && <><StatusSeparator /><ComputerWaitingStatus queue={source.operationQueue} computerId={queueVmId} onCancel={cancel} /></>}
  </span>
}
