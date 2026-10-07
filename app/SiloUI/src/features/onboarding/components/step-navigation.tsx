import type { ComponentProps } from "react"
import { Boxes, Check, CircleAlert, ClipboardCheck, GitFork, PackageCheck } from "lucide-react"

import { SiloMark } from "@/components/silo-mark"
import { TabsList, TabsTrigger } from "@/components/ui/tabs"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { Spinner } from "@/components/ui/spinner"
import { progressStatuses, statusTones } from "@/components/status-tone"
import { cn } from "@/lib/utils"
import type { OnboardingStep, PresentationStatus } from "@/features/onboarding/model/onboarding-state"

const steps = [
  { id: "dependencies", label: "Dependencies", icon: PackageCheck },
  { id: "computers", label: "Computers", icon: Boxes },
  { id: "github", label: "GitHub", icon: GitFork },
  { id: "review", label: "Review", icon: ClipboardCheck },
] as const

const statusLabels = Object.fromEntries(Object.entries(progressStatuses).map(([status, { label }]) => [status, label])) as Record<PresentationStatus, string>

function StepStatus({ status, collapsed }: { status: PresentationStatus; collapsed: boolean }) {
  if (status === "waiting") return null
  const className = cn(
    "shrink-0",
    collapsed ? "absolute -top-1 -right-1 size-2 rounded-full bg-sidebar ring-2 ring-sidebar" : "size-3.5",
    status === "running" ? "text-foreground" : statusTones[progressStatuses[status].tone].text,
  )
  const indicator = status === "running" ? <Spinner className={className} />
    : status === "failed" ? <CircleAlert aria-hidden="true" className={className} />
      : <Check aria-hidden="true" className={className} />
  return collapsed ? indicator : <span className="grid size-5 shrink-0 place-items-center">{indicator}</span>
}

interface StepNavigationProps extends ComponentProps<"nav"> {
  status: Record<OnboardingStep, PresentationStatus>
  collapsed: boolean
  completed: boolean
}

export function StepNavigation({ status, collapsed, completed, ...props }: StepNavigationProps) {
  return (
    <nav
      {...props}
      id="onboarding-sidebar"
      aria-label="Setup steps"
      data-collapsed={collapsed}
      className="silo-sidebar flex min-h-0 min-w-0 flex-col overflow-x-hidden overflow-y-auto border-r border-border bg-sidebar py-4"
    >
      <div className="sidebar-brand mb-4 flex shrink-0 items-center gap-3 overflow-hidden border-b border-border pb-4">
        <SiloMark className="size-8 shrink-0" />
        <span className="sidebar-label text-base font-semibold tracking-tight">Silo</span>
      </div>
      <TabsList className="flex h-auto w-full min-w-0 flex-col items-stretch justify-start gap-1 bg-transparent p-0" aria-label="Setup steps">
        {steps.map(({ id, label, icon: Icon }) => (
          <Tooltip key={id}>
            <TooltipTrigger asChild>
              <TabsTrigger
                value={id}
                disabled={completed}
                aria-label={label}
                aria-describedby={`setup-step-${id}-status`}
                aria-busy={status[id] === "running" || undefined}
                data-appearance="borderless"
                className="sidebar-primary relative flex h-10 w-full min-w-0 flex-none items-center gap-2 rounded-md border-0 bg-transparent py-2 text-ui text-muted-foreground shadow-none data-[state=active]:shadow-none hover:bg-sidebar-accent hover:text-sidebar-accent-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-sidebar-ring/70"
              >
                <span className="relative flex shrink-0">
                  <Icon aria-hidden="true" className="size-4" />
                  {collapsed && <StepStatus status={status[id]} collapsed />}
                </span>
                <span className="sidebar-label flex-1 text-left">{label}</span>
                {!collapsed && <StepStatus status={status[id]} collapsed={false} />}
                <span id={`setup-step-${id}-status`} className="sr-only">{statusLabels[status[id]]}</span>
              </TabsTrigger>
            </TooltipTrigger>
            <TooltipContent side="right" hidden={!collapsed}>{label} · {statusLabels[status[id]]}</TooltipContent>
          </Tooltip>
        ))}
      </TabsList>
    </nav>
  )
}
