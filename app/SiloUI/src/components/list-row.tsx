import type { ComponentProps, ReactNode } from "react"

import { Skeleton } from "@/components/ui/skeleton"
import { cn } from "@/lib/utils"

export function ListCard({ divided = false, className, ...props }: ComponentProps<"div"> & { divided?: boolean }) {
  // Divide peer rows only; expanded details provide their own inset border.
  return <div className={cn("overflow-hidden rounded-md border border-border", divided && "divide-y divide-border", className)} {...props} />
}

export function ListRowDetails({ label, className, ...props }: { label: string } & ComponentProps<"div">) {
  return <div role="group" aria-label={label} className={cn("mx-2 grid gap-3 border-t border-border py-3 pr-1 pl-8 text-xs", className)} {...props} />
}

export function ListRow({
  icon,
  title,
  detail,
  leading,
  actions,
  detailClassName,
  selectable,
  onOpen,
  className,
  ...props
}: {
  icon: ReactNode
  title: ReactNode
  detail?: ReactNode
  leading?: ReactNode
  actions?: ReactNode
  detailClassName?: string
  /** Opens the row's own view when the row body is clicked. Put a real button in `title` for keyboard access. */
  onOpen?: () => void
  /** Whether the title and detail text can be selected. Defaults to off for rows that open on click, so selecting never competes with opening. */
  selectable?: boolean
} & Omit<ComponentProps<"div">, "title" | "children">) {
  const textSelectable = selectable ?? !onOpen
  const content = (
    <>
      <div data-slot="list-row-title" className={cn("flex min-w-0 items-center gap-1.5 text-ui leading-4 font-medium", textSelectable && "select-text")}>{title}</div>
      {detail != null && <div className={cn("truncate text-caption leading-4 text-muted-foreground", textSelectable && "select-text", detailClassName)} title={typeof detail === "string" ? detail : undefined}>{detail}</div>}
    </>
  )
  return (
    <div className={cn("flex min-w-0 items-center gap-1.5 px-2 py-2 transition-colors", className)} {...props}>
      {leading}
      {icon}
      {onOpen ? (
        // Pointer convenience only: the title provides the real button, and
        // controls inside the row keep their own clicks and keys.
        <div
          data-slot="list-row-content"
          className="min-w-0 flex-1 text-left"
          onClick={(event) => { if (!isInteractiveTarget(event.target, event.currentTarget)) onOpen() }}
        >
          {content}
        </div>
      ) : (
        <div data-slot="list-row-content" className="min-w-0 flex-1">{content}</div>
      )}
      {actions}
    </div>
  )
}

const interactiveSelector = "a, button, input, select, textarea, label, [role='button'], [role='link'], [role='note'], [tabindex]"

function isInteractiveTarget(target: EventTarget, container: Element) {
  const element = target instanceof Element ? target.closest(interactiveSelector) : null
  return Boolean(element && element !== container && container.contains(element))
}

export function ListRowIcon({ className, ...props }: ComponentProps<"span">) {
  return <span className={cn("grid size-7 shrink-0 place-items-center rounded-md bg-muted text-muted-foreground", className)} {...props} />
}

/** A loading placeholder with the height of a two-line `ListRow`, so content replacing it does not shift the page. */
export function ListRowSkeleton({ label }: { label: string }) {
  return (
    <div role="status" aria-label={label} className="flex min-w-0 items-center gap-1.5 px-2 py-2">
      <Skeleton className="size-7 rounded-md" />
      <div className="grid min-w-0 gap-1.5">
        <Skeleton className="h-3 w-28" />
        <Skeleton className="h-2.5 w-44 max-w-full" />
      </div>
    </div>
  )
}
