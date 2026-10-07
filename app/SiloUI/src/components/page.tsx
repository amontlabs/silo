import type { ComponentProps, ReactNode } from "react"

import { cn } from "@/lib/utils"

/** The width, centering and padding every page of the application shares. Pass layout (`grid gap-4`, `flex h-full flex-col`) as `className`. */
export function PageContainer({ className, ...props }: ComponentProps<"div">) {
  return <div data-slot="page-container" className={cn("mx-auto w-full max-w-4xl px-4 py-5 sm:px-6 sm:py-6", className)} {...props} />
}

/** A page's title with an optional subtitle and the page-level actions. */
export function PageHeader({ title, subtitle, actions, className, ...props }: {
  title: ReactNode
  subtitle?: ReactNode
  actions?: ReactNode
} & Omit<ComponentProps<"header">, "title">) {
  return (
    <header data-slot="page-header" className={cn("flex min-w-0 flex-wrap items-center justify-between gap-2", className)} {...props}>
      <div className="min-w-0">
        <h2 className="text-sm font-semibold">{title}</h2>
        {subtitle != null && <p className="text-caption text-muted-foreground">{subtitle}</p>}
      </div>
      {actions}
    </header>
  )
}

/** The label above a group of rows on a page. It reads as a label, below the rows' own titles. */
export function SectionHeading({ className, ...props }: ComponentProps<"h3">) {
  return <h3 data-slot="section-heading" className={cn("text-xs font-medium text-muted-foreground", className)} {...props} />
}
