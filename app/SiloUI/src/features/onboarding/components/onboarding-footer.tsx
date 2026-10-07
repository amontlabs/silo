import { AlertCircle, Check, Clock3 } from "lucide-react"

import { Button } from "@/components/ui/button"
import { Spinner } from "@/components/ui/spinner"
import type { OnboardingStep, OnboardingViewModel } from "@/features/onboarding/model/onboarding-state"

interface OnboardingFooterProps {
  activeStep: OnboardingStep
  viewModel: OnboardingViewModel
  onBack: () => void
  onContinue: () => void
  completed?: boolean
  onOpenApp?: () => void
}

export function OnboardingFooter({ activeStep, viewModel, onBack, onContinue, completed = false, onOpenApp }: OnboardingFooterProps) {
  const isReview = activeStep === "review"
  const dependenciesBlocked = activeStep === "dependencies" && viewModel.dependencyStatus !== "succeeded"
  const failedItem = viewModel.queueItems.find(({ status }) => status === "failed")
  const runningItem = viewModel.queueItems.find(({ status }) => status === "running")
  const queued = viewModel.queueItems.some(({ status }) => status === "queued")
  const checkingDependencies = viewModel.dependencyStatus === "running"
  const failed = viewModel.dependencyStatus === "failed" || !!failedItem || !!viewModel.error
  const complete = completed || (viewModel.dependencyStatus === "succeeded" && viewModel.queueItems.length > 0 && viewModel.queueItems.every(({ status }) => status === "succeeded"))
  const completedStepMessage = viewModel.stepStatus[activeStep] === "succeeded"
    ? activeStep === "computers" ? "Complete · Computers are ready"
      : activeStep === "github" ? "Complete · Git identity setup is complete" : null
    : null
  const stepComplete = !!completedStepMessage && !checkingDependencies && !failed && !runningItem && !queued
  // A computer that failed, is starting, or is unconfirmed keeps Finish unavailable.
  const blocker = isReview && !viewModel.finishEnabled && !failed && !runningItem && !queued ? viewModel.finishBlocker : null
  const statusText = completed
    ? "Complete · Silo is ready"
    : checkingDependencies
      ? "Checking · Making sure this device can run computers"
    : viewModel.dependencyStatus === "failed"
      ? "Failed · Resolve dependency checks to continue"
      : failed
        ? `Failed · ${failedItem?.failure ?? viewModel.error?.message ?? "Setup did not complete"}`
        : blocker
          ? `${blocker.action === "start" ? "Needs attention" : "Waiting"} · ${blocker.message}`
        : complete
          ? "Complete · Ready to finish setup"
          : runningItem
            ? `In progress · ${runningItem.label}`
            : queued ? "Waiting · Setup tasks are queued"
              : completedStepMessage ? completedStepMessage
              : isReview && viewModel.finishEnabled ? "Ready · Finish setup"
              : activeStep === "dependencies" && viewModel.dependencyStatus === "succeeded" ? "Ready · Continue to configure computers"
                : activeStep === "computers" ? "Not started · Continue to create your computers. The first time can take a few minutes."
                : activeStep === "github" ? "Not started · Continue to save GitHub access and Git identities"
                : "Not started · Continue to begin this step"

  return (
    <footer className="flex shrink-0 flex-wrap items-center justify-between gap-x-4 gap-y-3 border-t border-border bg-muted/20 px-4 py-3 sm:px-6" aria-label="Onboarding actions">
      <div className="flex min-w-0 flex-[1_1_12rem] items-start gap-2 text-xs leading-5 text-muted-foreground" aria-live="polite">
        {(failed || blocker?.action === "start") && !completed ? <AlertCircle className="mt-0.5 size-3.5 shrink-0 text-destructive" />
          : complete || stepComplete ? <Check className="mt-0.5 size-3.5 shrink-0 text-success" />
            : checkingDependencies || runningItem ? <Spinner className="mt-0.5 shrink-0" />
              : <Clock3 className="mt-0.5 size-3.5 shrink-0" />}
        <span className="break-words">{statusText}</span>
      </div>
      <div className="ml-auto flex shrink-0 gap-2">
        {completed ? <Button onClick={onOpenApp} disabled={!onOpenApp}>Open Silo</Button> : <>
          <Button variant="outline" onClick={onBack} disabled={activeStep === "dependencies"}>Back</Button>
          <Button onClick={onContinue} disabled={isReview ? !viewModel.finishEnabled : dependenciesBlocked}>
            {isReview ? "Finish" : "Continue"}
          </Button>
        </>}
      </div>
    </footer>
  )
}
