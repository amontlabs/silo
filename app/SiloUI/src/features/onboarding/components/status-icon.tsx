import { AlertCircle, Check } from "lucide-react"

import { cn } from "@/lib/utils"
import type { PresentationStatus } from "@/features/onboarding/model/onboarding-state"
import { Spinner } from "@/components/ui/spinner"

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
    return <Spinner label="In progress" className={cn("size-4 text-warning", className)} />
  }
  return (
    <span
      aria-label="Waiting"
      className={cn("grid size-4 place-items-center rounded-full border border-border text-caption text-muted-foreground", className)}
    >
      {waitingLabel}
    </span>
  )
}
