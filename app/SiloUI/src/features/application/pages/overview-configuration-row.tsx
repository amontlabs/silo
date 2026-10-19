import { CircleAlert, Loader2 } from "lucide-react"

import { ErrorDetails } from "@/components/error-details"
import { ListRowIcon } from "@/components/list-row"
import { Progress } from "@/components/ui/progress"
import type { ConfigurationRowView } from "./overview-configuration"

export function ConfigurationIcon({ failed }: { failed: boolean }) {
  return (
    <ListRowIcon aria-hidden="true" className={failed
      ? "mt-1 self-start bg-destructive/10 text-destructive"
      : undefined}
    >
      {failed
        ? <CircleAlert className="size-3.5" aria-hidden="true" />
        : <Loader2 className="size-3.5 animate-spin" aria-hidden="true" />}
    </ListRowIcon>
  )
}

export function ConfigurationDetail({ view }: { view: ConfigurationRowView }) {
  const failed = view.status === "failed"
  const progressLabel = view.completedSteps === undefined ? undefined : `${view.completedSteps} of 3 steps complete`
  if (!failed) {
    return (
      <div role="status" aria-live="polite" aria-atomic="true" className="relative h-4 min-w-0">
        <div className="flex min-w-0 items-center gap-3">
          <span className="min-w-0 flex-1 truncate" title={view.message}>{view.message}</span>
          {progressLabel && <span className="shrink-0 text-[10px] text-muted-foreground">{progressLabel}</span>}
        </div>
        {view.completedSteps !== undefined && (
          <Progress aria-label={progressLabel} value={(view.completedSteps / 3) * 100} className="absolute inset-x-0 -bottom-1 h-0.5" />
        )}
      </div>
    )
  }
  return (
    <div
      role={failed ? "alert" : "status"}
      aria-live={failed ? "assertive" : "polite"}
      aria-atomic="true"
      className="grid gap-1.5 py-0.5"
    >
      <div className="flex min-w-0 items-start justify-between gap-3">
        <ErrorDetails className="flex-1 text-destructive" message={view.message} diagnostic={view.diagnostic} fallbackSummary="Computer changes failed." />
        {progressLabel && <span className="shrink-0 text-[10px] text-muted-foreground">{progressLabel}</span>}
      </div>
      {view.completedSteps !== undefined && (
        <Progress aria-label={progressLabel} value={(view.completedSteps / 3) * 100} className="mt-0.5" />
      )}
      {view.recovery && <p className="text-[10px] text-muted-foreground">{view.recovery}</p>}
    </div>
  )
}
