import "./computer-list.css"
import { ConnectionIcon } from "@/components/connection-icon"
import type { ComponentProps, ReactNode } from "react"
import { CircleAlert, TriangleAlert } from "lucide-react"

import { ListRow, ListRowIcon } from "@/components/list-row"
import { statusTones, type StatusTone } from "@/components/status-tone"
import { Button } from "@/components/ui/button"
import { ScrollArea } from "@/components/ui/scroll-area"
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "@/components/ui/tooltip"
import { cn } from "@/lib/utils"

export type ComputerIconState = "normal" | "warning" | "error"
export type ComputerRowTone = "running" | "starting" | "stopped" | "warning" | "error"

const computerRowTones: Record<ComputerRowTone, StatusTone> = {
  running: "success",
  starting: "warning",
  stopped: "neutral",
  warning: "warning",
  error: "danger",
}

export function ComputerList({ label, className, children, ...props }: {
  label: string
  children: ReactNode
} & Omit<ComponentProps<typeof ScrollArea>, "children">) {
  return (
    <TooltipProvider delayDuration={150}>
      <ScrollArea className={cn("computer-list rounded-md border border-border", className)} {...props}>
        <ol className="divide-y divide-border p-0" aria-label={label}>{children}</ol>
      </ScrollArea>
    </TooltipProvider>
  )
}

export function ComputerListItem({ className, ...props }: ComponentProps<"li">) {
  return <li className={cn("min-w-0 bg-background", className)} {...props} />
}

function ComputerIcon({ state, remote }: { state: ComputerIconState; remote: boolean }) {
  return (
    <ListRowIcon
      className={cn(
        state === "warning" && "bg-warning/10 text-warning",
        state === "error" && "bg-destructive/10 text-destructive",
      )}
      data-computer-icon-state={state}
      role={state === "normal" ? undefined : "img"}
      aria-label={state === "normal" ? undefined : `${state} status`}
    >
      {state === "error" ? (
        <CircleAlert className="size-3.5" aria-hidden="true" />
      ) : state === "warning" ? (
        <TriangleAlert className="size-3.5" aria-hidden="true" />
      ) : (
        <ConnectionIcon kind="vm" network={remote} label={`${remote ? "Remote" : "Local"} computer`} />
      )}
    </ListRowIcon>
  )
}

export function ComputerListRow({
  name,
  remote = false,
  badge,
  kindBadge,
  iconState = "normal",
  detail,
  detailClassName,
  leading,
  icon,
  actions,
  actionsClassName,
  hoverActions,
  tone,
  onOpen,
}: {
  name: string
  remote?: boolean
  badge?: ReactNode
  kindBadge?: ReactNode
  iconState?: ComputerIconState
  detail: ReactNode
  detailClassName?: string
  leading?: ReactNode
  icon?: ReactNode
  actions?: ReactNode
  actionsClassName?: string
  hoverActions?: ReactNode
  tone?: ComputerRowTone
  onOpen?: () => void
}) {
  return (
    <ListRow
      onOpen={onOpen}
      className={cn(
        "computer-row",
        statusTones[tone ? computerRowTones[tone] : "neutral"].row,
      )}
      data-computer-row-tone={tone}
      leading={leading}
      icon={icon ?? <ComputerIcon state={iconState} remote={remote} />}
      title={
        <>
          {onOpen
            ? <button type="button" aria-label={`Open ${name}`} title={name} onClick={onOpen} className="min-w-0 cursor-pointer truncate rounded-sm text-left outline-none focus-visible:ring-2 focus-visible:ring-ring/50">{name}</button>
            : <span className="truncate" title={name}>{name}</span>}
          {kindBadge}
          {badge}
        </>
      }
      detail={detail}
      detailClassName={cn(
        iconState === "warning" && "text-warning",
        iconState === "error" && "text-destructive",
        detailClassName,
      )}
      actions={<>
        {hoverActions && (
          <div
            role="group"
            className="computer-hover-actions flex shrink-0 items-center gap-0.5 transition-opacity"
            aria-label={`Manage ${name}`}
            data-slot="computer-hover-actions"
          >
            {hoverActions}
          </div>
        )}
        {actions && <div role="group" data-slot="computer-row-actions" className={cn("flex shrink-0 items-center gap-0.5", actionsClassName)} aria-label={`Controls for ${name}`}>{actions}</div>}
      </>}
    />
  )
}

export function ComputerAction({ label, tooltip, destructive = false, children, ...props }: {
  label: string
  tooltip?: string
  destructive?: boolean
  children: ReactNode
} & Omit<ComponentProps<typeof Button>, "children" | "aria-label">) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button
          type="button"
          variant={destructive ? "destructive" : "ghost"}
          size="icon-xs"
          aria-label={label}
          {...props}
        >
          {children}
        </Button>
      </TooltipTrigger>
      <TooltipContent className={tooltip ? "max-w-none whitespace-nowrap" : undefined}>{tooltip ?? label}</TooltipContent>
    </Tooltip>
  )
}
