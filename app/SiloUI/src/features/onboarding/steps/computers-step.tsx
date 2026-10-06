import { LoaderCircle, RotateCw } from "lucide-react"

import { ListCard, ListRow, ListRowDetails, ListRowIcon } from "@/components/list-row"
import { Button } from "@/components/ui/button"
import { Progress } from "@/components/ui/progress"
import { ActivityOutput } from "@/features/onboarding/components/activity-output"
import { StatusIcon } from "@/features/onboarding/components/status-icon"
import { ComputerConfigurationList } from "@/features/computers/components/computer-configuration-list"
import { computerSummary } from "@/features/computers/model/computer-summary"
import type { SetupComputerConfiguration } from "@/contracts/silo"
import type { ComputerProgressView, ComputerView } from "@/features/onboarding/model/onboarding-state"
import type { ComputerEditorDraft } from "@/features/onboarding/model/onboarding-draft"
import { cn } from "@/lib/utils"

function formatElapsed(seconds: number): string {
  const minutes = Math.floor(seconds / 60)
  const remainder = Math.floor(seconds % 60)
  return `${String(minutes).padStart(2, "0")}:${String(remainder).padStart(2, "0")}`
}

const computerStatusLabel: Record<ComputerView["status"], string> = {
  waiting: "Waiting",
  working: "In progress",
  ready: "Complete",
  failed: "Failed",
}

export function ComputersStep({ onConnectDevice, configurations, progress, onConfigurationsChange, onRetry, initialEditorDraft, onEditorDraftChange }: {
  onConnectDevice?: () => void
  configurations: readonly SetupComputerConfiguration[]
  progress: ComputerProgressView
  onConfigurationsChange: (configurations: SetupComputerConfiguration[]) => void
  onRetry: () => void
  initialEditorDraft?: ComputerEditorDraft | null
  onEditorDraftChange?: (editor: ComputerEditorDraft | null) => void
}) {
  const failed = progress.status === "failed"
  const running = progress.status === "running"
  const complete = progress.status === "succeeded"
  const title = configurations.length === 0 ? "Create computers later" : failed ? "Computer setup could not finish" : complete ? "Computers are ready" : running ? "Creating your computers" : "Computers are waiting"

  return (
    <section aria-labelledby="computers-title" className="flex h-full min-h-[28rem] flex-col gap-4">
      <h2 id="computers-title" className="sr-only" data-visual-heading="hidden">
        {failed ? "Computer setup failed" : title}
      </h2>

      <ListCard className="shrink-0" aria-label="Computer setup progress">
        <ListRow
          className="grid grid-cols-[auto_minmax(0,1fr)] gap-y-2 sm:flex"
          role={failed ? "alert" : "status"}
          aria-live="polite"
          icon={<ListRowIcon className={cn(
            failed && "bg-destructive/10 text-destructive",
            running && "bg-warning/10 text-warning",
            complete && "bg-success/10 text-success",
          )}><StatusIcon status={progress.status} className="size-3.5" /></ListRowIcon>}
          title={<h3>{title}</h3>}
          detail={configurations.length === 0 ? "Continue setup without a computer. Add one from Computers whenever you’re ready." : <>{progress.currentComputer && <span className="font-medium">{progress.currentComputer} · </span>}{progress.currentMessage}</>}
          detailClassName="whitespace-normal break-words select-text"
          actions={failed && progress.retryable && (
            <div className="col-start-2 shrink-0"><Button type="button" variant="outline" size="xs" onClick={onRetry}><RotateCw aria-hidden="true" />Retry</Button></div>
          )}
        />
        <ListRowDetails label="Computer setup details">
          {progress.fraction !== undefined && <Progress value={progress.fraction * 100} aria-label="Computer setup progress" />}
          <div className="flex flex-wrap items-center justify-between gap-x-3 gap-y-1 text-[11px] text-muted-foreground">
            <span>{progress.completedOperations} of {progress.totalOperations} operations complete</span>
            <span aria-label="Elapsed time" className="shrink-0 font-mono tabular-nums">{formatElapsed(progress.elapsedSeconds)}</span>
          </div>
          {failed && <p className="text-[11px] leading-4 text-muted-foreground select-text">{progress.recovery ?? "Resolve the reported computer issue, then retry setup."}</p>}
        </ListRowDetails>
        <ActivityOutput events={progress.visibleEvents} error={progress.activityError} embedded />
      </ListCard>

      <div className="min-h-48 flex-1">
        <ComputerConfigurationList
          onConnectDevice={onConnectDevice}
          isComputerCreated={(configuration) => progress.computers.some((computer) => computer.name === configuration.name && computer.status === "ready")}
          configurations={configurations}
          onConfigurationsChange={onConfigurationsChange}
          initialEditorDraft={initialEditorDraft}
          onEditorDraftChange={onEditorDraftChange}
          getRowPresentation={(configuration) => {
            const status = progress.computers.find(({ name }) => name === configuration.name)
            const state = status?.status ?? "waiting"
            const summary = computerSummary(configuration)
            return {
              busy: state === "working",
              tone: state === "failed" ? "error" : state === "working" ? "starting" : state === "ready" ? "running" : "stopped",
              iconState: state === "failed" ? "error" : "normal",
              badge: <span className={cn(
                "inline-flex shrink-0 items-center gap-1 text-[10px] font-normal",
                state === "failed" ? "text-destructive" : state === "working" ? "text-warning" : state === "ready" ? "text-success" : "text-muted-foreground",
              )}>{state === "working" && <LoaderCircle className="size-2.5 animate-spin motion-reduce:animate-none" aria-hidden="true" />}{computerStatusLabel[state]}</span>,
              detail: <span title={summary}>{summary}{status && state !== "ready" && status.detail !== "Waiting" ? ` · ${status.detail}` : ""}</span>,
              detailClassName: state === "failed" ? "whitespace-normal break-words" : undefined,
            }
          }}
        />
      </div>
    </section>
  )
}
