import { Clock3, GitBranch, Pencil, Play, RotateCw, UserRound } from "lucide-react"

import { ListCard, ListRow, ListRowIcon } from "@/components/list-row"
import { statusTones } from "@/components/status-tone"
import { Button } from "@/components/ui/button"
import { Spinner } from "@/components/ui/spinner"
import { StatusLabel } from "@/features/onboarding/components/status-label"
import { reviewQueueStatuses } from "@/features/onboarding/components/status-presentation"
import { SetupNotice } from "@/features/onboarding/components/setup-notice"
import { ComputerList, ComputerListItem, ComputerListRow } from "@/features/computers/components/computer-list"
import { computerSummary } from "@/features/computers/model/computer-summary"
import type { SetupComputerConfiguration } from "@/contracts/silo"
import type { OnboardingViewModel, ReviewQueueItemView, ComputerView } from "@/features/onboarding/model/onboarding-state"

interface ReviewStepProps {
  computerRetryable: boolean
  queueItems: ReviewQueueItemView[]
  configurations: readonly SetupComputerConfiguration[]
  computers: ComputerView[]
  identitySummary: string
  githubSummary: string
  githubConnected?: boolean
  errorMessage?: string
  errorRecovery?: string
  onRetryComputerSetup: () => void
  onEditStep?: (step: "computers" | "github") => void
  /** Why Finish is unavailable once computer setup settled. */
  finishBlocker?: OnboardingViewModel["finishBlocker"]
  onStartComputer?: (computer: string) => void
  onRefresh?: () => void
}

function FinishBlockerNotice({ blocker, onStartComputer, onRefresh }: { blocker: NonNullable<OnboardingViewModel["finishBlocker"]>; onStartComputer?: (computer: string) => void; onRefresh?: () => void }) {
  const action = blocker.action === "start" && onStartComputer
    ? <Button type="button" variant="outline" size="xs" onClick={() => onStartComputer(blocker.computer)}><Play aria-hidden="true" />Start {blocker.computer}</Button>
    : blocker.action === "refresh" && onRefresh
      ? <Button type="button" variant="outline" size="xs" onClick={onRefresh}><RotateCw aria-hidden="true" />Check again</Button>
      : null
  if (blocker.action === "start") return <SetupNotice title="Finish is unavailable" detail={blocker.message} action={action} />
  return <ListCard role="status" aria-live="polite">
    <ListRow
      className="grid grid-cols-[auto_minmax(0,1fr)] gap-y-2 sm:flex"
      icon={<ListRowIcon aria-hidden="true">{blocker.action === null ? <Spinner /> : <Clock3 className="size-3.5" />}</ListRowIcon>}
      title={<h3>Finish is unavailable</h3>}
      detail={blocker.message}
      detailClassName="whitespace-normal break-words"
      actions={action && <div className="col-start-2 shrink-0">{action}</div>}
    />
  </ListCard>
}

function ValidationBadge({ status }: { status: ReviewQueueItemView["status"] }) {
  return <StatusLabel {...reviewQueueStatuses[status]} busy={status === "running"} />
}

export function ReviewStep({ computerRetryable, queueItems, configurations, computers, identitySummary, githubSummary, githubConnected = true, errorMessage, errorRecovery, onRetryComputerSetup, onEditStep, finishBlocker, onStartComputer, onRefresh }: ReviewStepProps) {
  const identityItems = queueItems.filter(({ id }) => id === "identityRun" || id === "identityVerify")
  const githubItems = queueItems.filter(({ id }) => id === "githubRun" || id === "githubVerify")
  const githubStatus = githubItems.some(({ status }) => status === "failed") ? "failed" : githubItems.some(({ status }) => status === "running") ? "running" : githubItems.length === 2 && githubItems.every(({ status }) => status === "succeeded") ? "succeeded" : githubItems.some(({ status }) => status === "queued") ? "queued" : "idle"
  const githubComplete = githubItems.length === 2 && githubItems.every(({ status }) => status === "succeeded")
  const identityFailure = identityItems.find(({ status }) => status === "failed")
  const identityStatus = identityFailure ? "failed"
    : identityItems.some(({ status }) => status === "running") ? "running"
    : identityItems.some(({ status }) => status === "queued") ? "queued"
    : identityItems.length === 2 && identityItems.every(({ status }) => status === "succeeded") ? "succeeded" : "idle"

  return (
    <section aria-labelledby="review-title" className="grid gap-4">
      <h2 id="review-title" className="sr-only" data-visual-heading="hidden">Review setup</h2>

      {errorMessage && <SetupNotice
        title="Setup could not finish"
        detail={errorMessage}
        recovery={errorRecovery}
        action={computerRetryable && <Button type="button" variant="outline" size="xs" onClick={onRetryComputerSetup}><RotateCw aria-hidden="true" />Retry</Button>}
      />}
      {!errorMessage && finishBlocker && <FinishBlockerNotice blocker={finishBlocker} onStartComputer={onStartComputer} onRefresh={onRefresh} />}

      <section aria-labelledby="review-configurations-heading" className="min-w-0">
        <div className="mb-2 flex items-center justify-between gap-2">
          <h3 id="review-configurations-heading" className="text-xs font-medium">Computers</h3>
          {onEditStep && <Button type="button" variant="ghost" size="xs" onClick={() => onEditStep("computers")} aria-label="Edit computers"><Pencil aria-hidden="true" />Edit</Button>}
        </div>
        <ComputerList label="Computers">
          {configurations.map((configuration, index) => {
            const computer = computers.find(({ name }) => name === configuration.name)
            const state = computer?.status ?? "waiting"
            const status = state === "ready" ? "succeeded" : state === "working" ? "running" : state === "failed" ? "failed"
              : queueItems.some(({ id, status }) => (id === "computerRun" || id === "computerVerify") && status !== "idle") ? "queued" : "idle"
            const summary = computerSummary(configuration)
            return <ComputerListItem key={configuration.id} aria-busy={state === "working"}>
              <ComputerListRow
                name={configuration.name}
                leading={<span className="w-5 shrink-0 text-center font-mono text-caption tabular-nums text-muted-foreground">{index + 1}</span>}
                tone={state === "failed" ? "error" : state === "working" ? "starting" : state === "ready" ? "running" : "stopped"}
                iconState={state === "failed" ? "error" : "normal"}
                badge={<ValidationBadge status={status} />}
                detail={<span title={summary}>{summary}{computer && state !== "ready" && computer.detail !== "Waiting" ? ` · ${computer.detail}` : ""}</span>}
                detailClassName={state === "failed" ? "whitespace-normal break-words" : undefined}
              />
            </ComputerListItem>
          })}
        </ComputerList>
      </section>

      <section aria-labelledby="review-preferences-heading">
        <div className="mb-2 flex items-center justify-between gap-2">
          <h3 id="review-preferences-heading" className="text-xs font-medium">GitHub and Git identity</h3>
          {onEditStep && <Button type="button" variant="ghost" size="xs" onClick={() => onEditStep("github")} aria-label="Edit GitHub and Git identity"><Pencil aria-hidden="true" />Edit</Button>}
        </div>
        <ListCard divided>
          {[
            { title: "GitHub access", detail: githubSummary, Icon: GitBranch, complete: githubComplete },
            { title: "Git identity", detail: identitySummary, Icon: UserRound, complete: identityStatus === "succeeded" },
          ].map(({ title, detail, Icon, complete }) => <ListRow
            key={title}
            icon={<ListRowIcon aria-hidden="true"><Icon className="size-3.5" /></ListRowIcon>}
            role="group"
            aria-label={title}
            className={complete ? statusTones.success.row : undefined}
            title={<>{title}{title === "Git identity" ? <ValidationBadge status={identityStatus} /> : githubConnected ? <ValidationBadge status={githubStatus} /> : <span className="text-caption font-normal text-muted-foreground">Skipped</span>}</>}
            detail={title === "Git identity" && identityFailure?.failure ? `${detail} · ${identityFailure.failure}` : detail}
            detailClassName={title === "Git identity" && identityFailure ? "whitespace-normal break-words text-destructive" : undefined}
          />)}
        </ListCard>
      </section>
    </section>
  )
}
