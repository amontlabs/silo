import type { ReactNode } from "react"

import { SiloWindow } from "@/components/silo-window"
import { Tabs } from "@/components/ui/tabs"
import { TooltipProvider } from "@/components/ui/tooltip"
import { WindowToolbar } from "@/components/window-toolbar"
import type { AgentChoice } from "@/features/agents/model/agents"
import { OnboardingFooter } from "@/features/onboarding/components/onboarding-footer"
import { OnboardingUpdates } from "@/features/onboarding/components/onboarding-updates"
import { StepNavigation } from "@/features/onboarding/components/step-navigation"
import type { OnboardingStep, OnboardingViewModel } from "@/features/onboarding/model/onboarding-state"
import { useSidebarDisclosure } from "@/hooks/use-sidebar-disclosure"
import { cn } from "@/lib/utils"

interface OnboardingShellProps {
  activeStep: OnboardingStep
  viewModel: OnboardingViewModel
  onStepChange: (step: OnboardingStep) => void
  onBack: () => void
  onContinue: () => void
  completed?: boolean
  onOpenApp?: () => void
  agentChoice?: AgentChoice | null
  completedAction?: { label: string; onClick: () => void }
  reduceMotion?: boolean
  children: ReactNode
}

export function OnboardingShell({ activeStep, viewModel, onStepChange, onBack, onContinue, completed = false, onOpenApp, agentChoice, completedAction, reduceMotion = false, children }: OnboardingShellProps) {
  const {
    collapsed: pinnedCollapsed,
    previewing,
    sidebarRef,
    toggleRef,
    toggle,
    enterToggle,
    leaveToggle,
    enterSidebar,
    leaveSidebar,
    usePointer,
    useKeyboard,
    blurSidebar,
  } = useSidebarDisclosure()
  const collapsed = pinnedCollapsed && !previewing

  return (
    <TooltipProvider delayDuration={300} reduceMotion={reduceMotion}>
    <SiloWindow title="Silo Setup" label="Silo Setup" reduceMotion={reduceMotion} className={cn("silo-onboarding", pinnedCollapsed && "sidebar-pinned-collapsed")} titleBar={
      <WindowToolbar title="Silo Setup" sidebarId="onboarding-sidebar" collapsed={pinnedCollapsed} previewing={previewing} toggleRef={toggleRef} onToggleSidebar={toggle} onPreviewEnter={enterToggle} onPreviewLeave={leaveToggle} />
    }>
      <Tabs orientation="vertical" value={activeStep} onValueChange={(value) => { if (!completed) onStepChange(value as OnboardingStep) }} className="sidebar-layout grid min-h-0 flex-1 gap-0" data-sidebar-layout={pinnedCollapsed ? "collapsed" : "expanded"}>
        <StepNavigation
          ref={sidebarRef}
          status={viewModel.stepStatus}
          collapsed={collapsed}
          completed={completed}
          data-previewing={previewing}
          onPointerEnter={enterSidebar}
          onPointerLeave={leaveSidebar}
          onPointerDown={usePointer}
          onKeyDown={useKeyboard}
          onBlurCapture={blurSidebar}
        />
        <div className="flex min-h-0 min-w-0 flex-col">
          <div className="relative min-h-0 flex-1 overflow-hidden">{children}<OnboardingUpdates /></div>
          <OnboardingFooter activeStep={activeStep} viewModel={viewModel} onBack={onBack} onContinue={onContinue} completed={completed} onOpenApp={onOpenApp} agentChoice={agentChoice} completedAction={completedAction} />
        </div>
      </Tabs>
    </SiloWindow>
    </TooltipProvider>
  )
}
