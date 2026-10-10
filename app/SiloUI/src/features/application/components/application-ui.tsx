
import { StatusBadge } from "@/components/status-badge"
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "@/components/ui/tooltip"
import { statusTones, type StatusTone } from "@/components/status-tone"
import { cn } from "@/lib/utils"
import type { ComputerState } from "@/features/application/model/application-source"
import type { ComputerDevice } from "@/features/application/model/connections"

const computerStateTones: Record<ComputerState, StatusTone> = {
  running: "success",
  starting: "warning",
  stopped: "neutral",
  failed: "danger",
}

// Each state also has its own shape (● running, ▶ starting, ■ stopped, ✕ failed) so the
// state reads without relying on color.
const computerStateShapes = {
  running: { shape: "circle", className: "rounded-full" },
  starting: { shape: "triangle", className: "[clip-path:polygon(10%_0,100%_50%,10%_100%)]" },
  stopped: { shape: "square", className: "rounded-[1px]" },
  failed: { shape: "cross", className: "[clip-path:polygon(20%_0,50%_30%,80%_0,100%_20%,70%_50%,100%_80%,80%_100%,50%_70%,20%_100%,0_80%,30%_50%,0_20%)]" },
} as const satisfies Record<ComputerState, { shape: string; className: string }>

export function ComputerStateDot({ state, className }: { state: ComputerState; className?: string }) {
  const { shape, className: shapeClassName } = computerStateShapes[state]
  return <span className={cn("size-2", shapeClassName, statusTones[computerStateTones[state]].dot, className)} data-computer-state-dot={state} data-computer-state-shape={shape} aria-hidden="true" />
}

export function ComputerBadge({ name, state, device }: { name: string; state: ComputerState; device?: ComputerDevice }) {
  const stateLabel = state.charAt(0).toUpperCase() + state.slice(1)
  return (
    <TooltipProvider delayDuration={150}><Tooltip><TooltipTrigger asChild><StatusBadge
      indicator={<ComputerStateDot state={state} />}
      revealLabel={false}
      role="group"
      aria-label={`${name}, ${stateLabel}${device ? `, on ${device.name}` : ""}`}
      tabIndex={0}
      className="focus-ring"
    >
      {device ? `${name} · ${device.name}` : name}
    </StatusBadge></TooltipTrigger><TooltipContent>{stateLabel} on {device?.name ?? "this device"}</TooltipContent></Tooltip></TooltipProvider>
  )
}

export function ComputerStateLabel({ state }: { state: ComputerState }) {
  return (
    <span className={cn("font-medium", statusTones[computerStateTones[state]].text)} data-computer-state={state}>
      {state.charAt(0).toUpperCase() + state.slice(1)}
    </span>
  )
}

/** A port's reachability: the same dot-and-label pattern as a computer's state. */
export function PortStateLabel({ state }: { state: string }) {
  const { dot, text } = statusTones[state === "Reachable" ? "success" : "neutral"]
  return (
    <span className="inline-flex items-center gap-1.5">
      <span className={cn("size-1.5 rounded-full", dot)} aria-hidden="true" />
      <span className={text}>{state}</span>
    </span>
  )
}

export function ComputerStatus({ state, detail }: { state: ComputerState; detail?: string }) {
  return (
    <span className="inline-flex items-center gap-2 text-xs text-muted-foreground" aria-label={detail ?? state}>
      <ComputerStateDot state={state} />
      <ComputerStateLabel state={state} />
    </span>
  )
}
