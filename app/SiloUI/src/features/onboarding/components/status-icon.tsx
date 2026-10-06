import { AlertCircle, Check, LoaderCircle } from "lucide-react"

import { cn } from "@/lib/utils"
import type { PresentationStatus } from "@/features/onboarding/model/onboarding-state"

interface StatusIconProps {
  status: PresentationStatus
  waitingLabel?: string
  className?: string
}

export function StatusIcon({ status, waitingLabel, className }: StatusIconProps) {
  if (status === "succeeded") {
    return <Check aria-label="Complete" className={cn("size-4 text-success", className)} />
  }
  if (status === "failed") {
    return <AlertCircle aria-label="Failed" className={cn("size-4 text-destructive", className)} />
  }
  if (status === "running") {
    return <LoaderCircle aria-label="In progress" className={cn("size-4 animate-spin motion-reduce:animate-none text-warning", className)} />
  }
  return (
    <span
      aria-label="Waiting"
      className={cn("grid size-4 place-items-center rounded-full border border-border text-[9px] text-muted-foreground", className)}
    >
      {waitingLabel}
    </span>
  )
}
