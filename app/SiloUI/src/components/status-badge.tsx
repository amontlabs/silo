import type { ComponentProps, ReactNode } from "react"

import { cn } from "@/lib/utils"

interface StatusBadgeProps extends ComponentProps<"span"> {
  indicator: ReactNode
  /** Show the full label as a native tooltip. Turn it off when the badge already has a tooltip of its own. */
  revealLabel?: boolean
}

export function StatusBadge({ indicator, children, className, revealLabel = true, ...props }: StatusBadgeProps) {
  return (
    <span
      {...props}
      data-slot="status-badge"
      className={cn(
        "inline-flex h-5 max-w-full shrink-0 items-center justify-center gap-1 rounded-full border border-border bg-muted px-1.5 text-caption leading-4 font-medium whitespace-nowrap text-muted-foreground",
        className,
      )}
    >
      <span data-slot="status-badge-indicator" className="grid size-2 shrink-0 place-items-center" aria-hidden="true">
        {indicator}
      </span>
      <span data-slot="status-badge-label" className="min-w-0 truncate" title={revealLabel && typeof children === "string" ? children : undefined}>{children}</span>
    </span>
  )
}
