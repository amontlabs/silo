import type { ReactNode } from "react"

import { cn } from "@/lib/utils"

/**
 * The one empty-state style: a centered explanation with an optional icon and a next step.
 * The default fills a page or pane in a dashed frame; `inline` sits unframed inside a card,
 * list or popover that already has its own border. Titles are short and carry no trailing period.
 */
export function EmptyState({ icon, title, description, action, variant = "default", className }: {
  icon?: ReactNode
  title: string
  description?: ReactNode
  action?: ReactNode
  variant?: "default" | "inline"
  className?: string
}) {
  const inline = variant === "inline"
  return (
    <div data-slot="empty-state" data-variant={variant} className={cn(inline ? "grid place-items-center px-3 py-4 text-center" : "grid min-h-48 place-items-center rounded-lg border border-dashed border-border px-6 py-6 text-center", className)}>
      <div className="grid justify-items-center">
        {icon && <span aria-hidden="true" className={cn("text-muted-foreground [&_svg]:size-5", inline ? "mb-1.5" : "mb-3")}>{icon}</span>}
        <p className={cn("font-medium", inline ? "text-xs" : "text-sm")}>{title}</p>
        {description && <p className="mt-1 text-xs text-muted-foreground">{description}</p>}
        {action && <div className="mt-3">{action}</div>}
      </div>
    </div>
  )
}
