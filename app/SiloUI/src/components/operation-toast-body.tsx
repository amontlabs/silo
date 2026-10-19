import { useEffect, useRef, useState } from "react"
import { CheckIcon, CircleAlertIcon, CircleIcon } from "lucide-react"

import { Button } from "@/components/ui/button"
import { Progress } from "@/components/ui/progress"
import { Spinner } from "@/components/ui/spinner"
import { formatElapsed } from "@/lib/format-elapsed"
import { restoreFocus } from "@/lib/focus"
import { useClock } from "@/lib/use-clock"

export type OperationStepState = "done" | "current" | "pending" | "failed"
export interface OperationStep { label: string; state: OperationStepState }

export interface OperationCancel {
  /** Button label. Defaults to "Cancel". */
  label?: string
  /** When set, clicking the button swaps the toast body to an inline confirmation. */
  confirm?: { prompt: string; confirmLabel: string; keepLabel?: string }
  onCancel: () => void
}

export interface OperationProgressOptions {
  title: string
  /** The current step, e.g. "Saving disk copies". */
  step?: string
  steps?: OperationStep[]
  /** 0–1 when known; null/undefined renders an indeterminate bar. */
  progress?: number | null
  /** Epoch ms the operation began; drives the elapsed timer. Defaults to when the toast first rendered. */
  startedAt?: number
  cancel?: OperationCancel
  /** A button beside Cancel for a choice the step offers, such as Retry. */
  action?: { label: string; onClick: () => void }
  /** Computer this notification is about (see `dismissComputerToasts`). */
  computer?: string | string[]
}

function useElapsed(startedAt: number | undefined) {
  const [fallback] = useState(() => Date.now())
  const start = startedAt ?? fallback
  const now = useClock()
  return formatElapsed(Math.max(0, now - start))
}

const stepIcon: Record<OperationStepState, React.ReactNode> = {
  done: <CheckIcon className="size-3 text-muted-foreground" aria-hidden />,
  current: <Spinner size="sm" />,
  pending: <CircleIcon className="size-3 text-muted-foreground/50" aria-hidden />,
  failed: <CircleAlertIcon className="size-3 text-destructive" aria-hidden />,
}

const stepStatus: Record<OperationStepState, string> = {
  done: "Completed",
  current: "In progress",
  pending: "Pending",
  failed: "Failed",
}

/** Body of a progress toast (rendered as the Sonner description). Use `showOperationProgress`. */
/**
 * True when a step line only repeats the title ("Creating checkpoint…" under "Creating
 * checkpoint “X”"). Compared case-insensitively, ignoring quotes and ellipses.
 */
function isRedundantStep(title: string | undefined, step: string | undefined): boolean {
  if (!step) return true
  if (!title) return false
  const normalize = (text: string) => text.toLowerCase().replace(/[“”‘’"'`]/g, "").replace(/(\.{3}|…)/g, "").replace(/\s+/g, " ").trim()
  const normalizedStep = normalize(step)
  return normalizedStep.length === 0 || normalize(title).startsWith(normalizedStep)
}

export function OperationToastBody({ title, step, steps, progress, startedAt, cancel, action }: Omit<OperationProgressOptions, "title"> & { title?: string }) {
  const elapsed = useElapsed(startedAt)
  const [confirming, setConfirming] = useState(false)
  const cancelButton = useRef<HTMLButtonElement>(null)
  const wasConfirming = useRef(false)
  useEffect(() => {
    if (wasConfirming.current && !confirming) restoreFocus(cancelButton.current)
    wasConfirming.current = confirming
  }, [confirming])
  const value = progress == null ? null : Math.min(100, Math.max(0, progress * 100))

  if (confirming && cancel?.confirm) {
    const { prompt, confirmLabel, keepLabel = "Keep going" } = cancel.confirm
    return <div className="grid gap-2 text-xs" role="group" aria-label="Confirm cancel" onKeyDown={event => {
      if (event.key !== "Escape" || event.defaultPrevented || event.nativeEvent.isComposing) return
      event.preventDefault()
      event.stopPropagation()
      setConfirming(false)
    }}>
      <p>{prompt}</p>
      <div className="flex gap-2">
        <Button type="button" variant="ghost" size="sm" autoFocus onClick={() => setConfirming(false)}>{keepLabel}</Button>
        <Button type="button" variant="outline" size="sm" onClick={() => { setConfirming(false); cancel.onCancel() }}>{confirmLabel}</Button>
      </div>
    </div>
  }

  const showStep = !isRedundantStep(title, step)
  return <div className="grid w-full min-w-0 gap-1.5 text-xs">
    <Progress className="w-full" value={value} aria-label={step ?? title ?? "Progress"} />
    <div className="flex min-w-0 items-center justify-between gap-2 text-muted-foreground">
      <span className="min-w-0 flex-1 truncate" title={showStep ? step : undefined}>{showStep ? step : null}</span>
      <span className="shrink-0 tabular-nums" data-slot="operation-elapsed">{elapsed}</span>
    </div>
    {steps && steps.length > 0 && <ul className="grid gap-0.5" aria-label="Steps">
      {steps.map((entry) => <li key={entry.label} data-state={entry.state} aria-current={entry.state === "current" ? "step" : undefined} className={`flex items-center gap-1.5 ${entry.state === "pending" ? "text-muted-foreground" : entry.state === "failed" ? "text-destructive" : ""}`}>
        {stepIcon[entry.state]}<span className="min-w-0 truncate" title={entry.label}>{entry.label}<span className="sr-only">: {stepStatus[entry.state]}</span></span>
      </li>)}
    </ul>}
    {(cancel || action) && <div className="flex justify-end gap-2">
      {action && <Button type="button" variant="outline" size="xs" onClick={action.onClick}>{action.label}</Button>}
      {cancel && <Button ref={cancelButton} type="button" variant="outline" size="xs" onClick={() => (cancel.confirm ? setConfirming(true) : cancel.onCancel())}>{cancel.label ?? "Cancel"}</Button>}
    </div>}
  </div>
}
