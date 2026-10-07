import type { ComponentProps, ReactNode } from "react"

import { cn } from "@/lib/utils"

/** Shared class for the heading text so the list heading and the detail
 * breadcrumb root stay pixel-identical and cannot drift. */
export const listHeadingClassName = "font-medium"

/** The header row shared by the computer list and the computer detail page: a
 * heading (or breadcrumb) with an optional subtitle on the left and actions on
 * the right. Both callers render it identically so opening a computer never
 * shifts the heading. */
export function ListHeader({ heading, subtitle, actions, className, ...props }: {
  heading: ReactNode
  subtitle?: ReactNode
  actions?: ReactNode
} & Omit<ComponentProps<"div">, "title">) {
  return (
    <div className={cn("mb-3 flex min-w-0 shrink-0 flex-wrap items-center justify-between gap-2 text-xs", className)} {...props}>
      <div className="min-w-0">
        {heading}
        {subtitle != null && <div className="text-caption text-muted-foreground">{subtitle}</div>}
      </div>
      {actions}
    </div>
  )
}
