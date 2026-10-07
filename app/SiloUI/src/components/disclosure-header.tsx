import { useId, type ComponentProps, type ReactNode } from "react"

import { DisclosureIndicator, disclosureTriggerStateClass } from "@/components/disclosure-indicator"
import { CollapsibleTrigger } from "@/components/ui/collapsible"
import { cn } from "@/lib/utils"

interface DisclosureHeaderProps extends Omit<ComponentProps<"div">, "title" | "children"> {
  title: ReactNode
  detail?: ReactNode
  icon?: ReactNode
  actions?: ReactNode
  label?: string
  controlsLabel?: string
  titleClassName?: string
  detailClassName?: string
}

export function DisclosureHeader({ title, detail, icon, actions, label, controlsLabel, className, titleClassName, detailClassName, ...props }: DisclosureHeaderProps) {
  const detailId = useId()
  return <div
    role={controlsLabel ? "group" : undefined}
    aria-label={controlsLabel}
    className={cn("relative flex min-w-0 items-center rounded-md px-2 py-2 transition-colors row-hover", className)}
    {...props}
  >
    <div className="flex min-w-0 flex-1 items-center gap-1.5 pr-6">
      <CollapsibleTrigger
        aria-label={label}
        aria-describedby={label && detail != null ? detailId : undefined}
        className={cn(disclosureTriggerStateClass, "flex min-w-0 flex-1 items-center gap-1.5 text-left outline-none after:absolute after:inset-0 after:rounded-[inherit] focus-visible:after:ring-2 focus-visible:after:ring-inset focus-visible:after:ring-ring/60")}
      >
        {icon}
        <span className="min-w-0 flex-1">
          <span className={cn("block truncate text-ui leading-4 font-medium text-foreground", titleClassName)} title={typeof title === "string" ? title : undefined}>{title}</span>
          {detail != null && <span id={label ? detailId : undefined} className={cn("block truncate text-caption leading-4 text-muted-foreground", detailClassName)} title={typeof detail === "string" ? detail : undefined}>{detail}</span>}
        </span>
        <span className="absolute top-1/2 right-2 -translate-y-1/2 text-muted-foreground"><DisclosureIndicator /></span>
      </CollapsibleTrigger>
      {/* Actions sit above the trigger's hit area, as separate native controls. */}
      {actions && <div className="relative z-10 flex shrink-0 items-center gap-1.5">{actions}</div>}
    </div>
  </div>
}
