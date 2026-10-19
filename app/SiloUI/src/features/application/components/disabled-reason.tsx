import type { ReactNode } from "react"

import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"

/**
 * Says why a control is disabled. A disabled button receives no pointer events, so the
 * tooltip hangs on a wrapper that still takes hover and keyboard focus. With no reason the
 * control renders as is.
 */
export function DisabledReason({ reason, children }: { reason?: string; children: ReactNode }) {
  if (!reason) return children
  return <Tooltip>
    <TooltipTrigger asChild>
      <span className="inline-flex rounded-lg outline-none focus-visible:ring-2 focus-visible:ring-ring/50" tabIndex={0} role="group" aria-label={reason} data-disabled-reason="">{children}</span>
    </TooltipTrigger>
    <TooltipContent>{reason}</TooltipContent>
  </Tooltip>
}
