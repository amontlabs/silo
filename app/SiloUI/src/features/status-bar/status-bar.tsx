import { ErrorDetails } from "@/components/error-details"
import { configurationFailureDiagnostic } from "@/features/application/model/configuration-failure"
import { interruptionPrompt, lifecycleGuard, type LifecyclePrompt } from "@/features/application/model/lifecycle-guard"
import { computerTarget } from "@/features/application/model/connections"
import { DeviceBadge } from "@/features/computers/components/device-badge"
import { useEffect, useRef, useState, type ComponentType, type ReactNode } from "react"
import { CircleAlert, Code, GitBranch, Loader2, Monitor, Play, Power, RotateCw, Square, Terminal, TriangleAlert } from "lucide-react"

import { ListCard, ListRow, ListRowDetails, ListRowIcon } from "@/components/list-row"
import { SiloMark } from "@/components/silo-mark"
import { Button } from "@/components/ui/button"
import { ComputerStateLabel } from "@/features/application/components/application-ui"
import { RepositoryPushButton, RepositoryPushFeedback } from "@/features/application/components/repository-push-feedback"
import type { ApplicationSource, ApplicationComputer } from "@/features/application/model/application-source"
import { commitLabel } from "@/features/application/model/repository-push"
import { ComputerAction, ComputerListItem, ComputerListRow } from "@/features/computers/components/computer-list"
import { SecretChangesLabel } from "@/features/computers/components/secret-changes-label"
import { computerIconState, computerRowTone } from "@/features/computers/model/computer-presentation"
import { cn } from "@/lib/utils"
import { visibleText } from "@/lib/visible-text"
import { lifecycleOutcome, computerTargetLabel } from "./status-bar-model"
import { computerAvailability } from "@/features/application/model/computer-availability"
import type { StatusBarActions, ComputerMenuProps } from "./status-bar-types"
import { ComputerMenu } from "./computer-menu"
import { StatusFolderPicker } from "./status-folder-picker"
import { QuitConfirmation } from "./quit-confirmation"
import { computersStoppedByQuit } from "./quit-confirmation-model"

function OperationIssue({ title, detail, actionLabel, actionText = "Details", tone = "error", onReview, retry }: { title: string; detail: ReactNode; actionLabel: string; actionText?: string; tone?: "error" | "warning"; onReview: () => void; retry?: ReactNode }) {
  return (
    <ListCard className="mb-2" role={tone === "error" ? "alert" : "status"} aria-label={title}>
      <ListRow
        icon={tone === "error"
          ? <ListRowIcon className="bg-destructive/10 text-destructive"><CircleAlert className="size-3.5" aria-hidden="true" /></ListRowIcon>
          : <ListRowIcon className="bg-warning/10 text-warning"><TriangleAlert className="size-3.5" aria-hidden="true" /></ListRowIcon>}
        title={title}
        detail={detail}
        detailClassName="whitespace-normal break-words"
        actions={<div className="flex shrink-0 items-center gap-1">
          <Button variant="outline" size="xs" aria-label={actionLabel} onClick={onReview}>{actionText}</Button>
          {retry}
        </div>}
      />
    </ListCard>
  )
}

function RepositoryPushes({ computer, source, actions }: { computer: ApplicationComputer; source: ApplicationSource; actions: StatusBarActions }) {
  const repositories = computer.repositories.flatMap((repository) => {
    const operation = source.repositoryPushOperations.find((push) => push.computer === computerTarget(computer) && push.repositoryPath === repository.path)
    return operation?.status !== "failed" && (operation || repository.ahead > 0) ? [{ repository, operation }] : []
  })
  if (!repositories.length) return null
  const canPush = computerAvailability(computer, source).canOpen
  return (
    <div className="grid gap-1 px-2 pb-2">
      {repositories.map(({ repository, operation }) => {
        // The row authorizes a push, so name the repository by its full path with any
        // invisible or bidirectional characters revealed: a basename could imitate another.
        const path = visibleText(repository.path)
        return <div key={repository.path} className="flex min-h-6 min-w-0 items-center gap-2" role="group" aria-label={`${path} in ${computer.configuration.name}`}>
          <span className="flex min-w-0 flex-1 items-center gap-1 text-[11px] text-muted-foreground" title={path}>
            <GitBranch className="size-3 shrink-0" aria-hidden="true" />
            <span className="truncate">{path}</span>
          </span>
          {operation ? <RepositoryPushFeedback
            disabled={!canPush}
            operation={operation}
            computer={computerTarget(computer)}
            repositoryPath={repository.path}
            repository={repository}
            onPush={(target) => actions.pushRepository(computerTarget(computer), repository.path, target)}
            onDismiss={actions.dismissRepositoryPush}
            showSuccess
          /> : <RepositoryPushButton repository={repository} disabled={!canPush} label={`Push ${commitLabel(repository.ahead)} for ${path} in ${computer.configuration.name}`} onPush={(target) => { if (canPush) actions.pushRepository(computerTarget(computer), repository.path, target) }}>
            Push {commitLabel(repository.ahead)}
          </RepositoryPushButton>}
        </div>
      })}
    </div>
  )
}

/**
 * `quitRequest` lets a host-side quit request (menu, ⌘Q, backend event) open the same
 * confirmation as the power button: bump it to a new number for each request.
 */
export function StatusBarContent({ source, actions, focusContent, computerMenu: ComputerActions = ComputerMenu, quitRequest }: { source: ApplicationSource; actions: StatusBarActions; focusContent: () => void; computerMenu?: ComponentType<ComputerMenuProps>; quitRequest?: number }) {
  const [quitPending, setQuitPending] = useState(false)
  const stoppedByQuit = computersStoppedByQuit(source.computers)
  function requestQuit() {
    if (stoppedByQuit.length) setQuitPending(true)
    else actions.quit()
  }
  const [seenQuitRequest, setSeenQuitRequest] = useState(quitRequest)
  if (quitRequest !== seenQuitRequest) {
    setSeenQuitRequest(quitRequest)
    if (quitRequest !== undefined && stoppedByQuit.length) setQuitPending(true)
  }
  const lastQuitRequest = useRef(quitRequest)
  useEffect(() => {
    if (quitRequest === lastQuitRequest.current) return
    lastQuitRequest.current = quitRequest
    if (quitRequest !== undefined && !stoppedByQuit.length) actions.quit()
  })
  const [hasNavigated, setHasNavigated] = useState(false)
  const [folderComputer, setFolderComputer] = useState<string | null>(null)
  const [confirmation, setConfirmation] = useState<{ computer: string; action: "stop" | "restart" } | null>(null)
  const [startPrompt, setStartPrompt] = useState<{ target: string; prompt: LifecyclePrompt } | null>(null)
  const [lifecycleIssue, setLifecycleIssue] = useState<{ title: string; message: string } | null>(null)
  const guarded = lifecycleGuard(source, actions, {
    notify: (title, message) => setLifecycleIssue({ title, message }),
    prompt: (prompt, _confirm, computer) => {
      // Store the target, then confirm against the latest source after any refresh.
      setStartPrompt({ target: computerTarget(computer), prompt })
    },
  })
  const guardedActions: StatusBarActions = {
    ...actions,
    startComputer: (target) => {
      const computer = source.computers.find(computer => computerTarget(computer) === target)
      if (computer) { setLifecycleIssue(null); guarded.request(computer, "start") }
    },
    stopComputer: (target) => {
      const computer = source.computers.find(computer => computerTarget(computer) === target)
      if (computer) guarded.confirm(computer, "stop")
    },
    restartComputer: (target) => {
      const computer = source.computers.find(computer => computerTarget(computer) === target)
      if (computer) guarded.confirm(computer, "restart")
    },
  }
  const repair = source.runtimeRepair
  const folders = source.computers.find(({ configuration }) => configuration.id === folderComputer)
  const failedPushes = source.repositoryPushOperations.filter((operation) => operation.status === "failed")
  const failedConfiguration = source.computerConfigurationOperation?.status === "failed" ? source.computerConfigurationOperation : null
  const approval = source.computerConfigurationOperation?.status === "awaiting-approval" ? source.computerConfigurationOperation : null

  function openFolders(id: string) {
    setHasNavigated(true)
    setFolderComputer(id)
  }

  if (folders && computerAvailability(folders, source).canOpen) {
    return <div key="folders" className="status-page status-page-forward flex max-h-[518px] shrink-0 flex-col overflow-hidden">
      <StatusFolderPicker listDirectory={actions.listComputerDirectory} computer={folders} editor={source.preferences.editor} onBack={() => { setFolderComputer(null); focusContent() }} onOpen={(path) => actions.openEditor(computerTarget(folders), path)} />
    </div>
  }

  return (
    <div key="computers" className={cn("status-page flex max-h-[518px] shrink-0 flex-col overflow-hidden", hasNavigated && "status-page-back")}>
      <div className="shrink-0 px-2 pt-2">
        {lifecycleIssue && <OperationIssue
          title={lifecycleIssue.title}
          detail={lifecycleIssue.message}
          actionLabel="Review computer operation availability"
          onReview={() => actions.openSilo({ computerSection: "overview" })}
        />}
        {repair && <ListCard className="mb-2">
          <ListRow
            icon={<ListRowIcon className="bg-destructive/10 text-destructive"><CircleAlert className="size-3.5" /></ListRowIcon>}
            title="System issue"
            detail={repair.reason}
            actions={<Button variant="outline" size="xs" onClick={() => actions.openSilo({ tab: "system" })}>View issue</Button>}
          />
        </ListCard>}
        {failedConfiguration && <OperationIssue
          title="Computer changes failed"
          detail={<ErrorDetails message={failedConfiguration.error.message} diagnostic={configurationFailureDiagnostic(failedConfiguration)} fallbackSummary="Computer changes failed." />}
          actionLabel="Review computer changes"
          onReview={() => actions.openSilo({ computerSection: "overview" })}
        />}
        {approval && <OperationIssue
          tone="warning"
          title="Computer changes need approval"
          detail={approval.result.message}
          actionLabel="Review computer changes"
          actionText="Review"
          onReview={() => actions.openSilo({ computerSection: "overview" })}
        />}
        {failedPushes.map((operation) => {
          const computer = source.computers.find(computer => computerTarget(computer) === operation.computer)
          const repository = computer?.repositories.find(({ path, ahead }) => path === operation.repositoryPath && ahead > 0)
          const canRetry = computer && repository && computerAvailability(computer, source).canOpen
          const label = computerTargetLabel(operation.computer, source)
          const path = visibleText(operation.repositoryPath)
          return <OperationIssue
            key={`${operation.computer}:${operation.repositoryPath}`}
            title={`Push failed · ${label}`}
            detail={`${path} · ${operation.message}`}
            actionLabel={`Review push failure for ${label}, ${path}`}
            onReview={() => actions.openSilo({ computer: operation.computer, computerSection: "files" })}
            retry={repository
              ? <RepositoryPushButton repository={repository} disabled={!canRetry} label={`Retry push for ${path}`} onPush={(target) => { if (canRetry) actions.pushRepository(operation.computer, operation.repositoryPath, target) }}><RotateCw />Retry</RepositoryPushButton>
              : <Button variant="outline" size="xs" aria-label={`Retry push for ${path}`} disabled><RotateCw />Retry</Button>}
          />
        })}
      </div>
      <div className="min-h-0 flex-auto overflow-y-auto overscroll-contain px-2 pb-2">
        {source.computers.length ? <ListCard className="border-0">
          <ol aria-label="Computers" className="divide-y">
            {source.computers.map((computer) => {
              const { configuration } = computer
  const target = computerTarget(computer)
              const availability = computerAvailability(computer, source)
              const pending = confirmation?.computer === target ? confirmation : null
              const pendingPrompt = pending ? interruptionPrompt(computer, pending.action) : null
              const pendingSecrets = !computer.device ? source.secrets.filter((secret) => secret.state === "restart-required" && secret.computers.includes(configuration.name)).map(({ name }) => name) : []
              const activity = source.activities.find((item) => item.category === "computer" && item.computer === target && item.status === "running")
              const review = computer.state === "failed" || computer.attention?.level === "error"
              // A failed Start leaves the computer "Stopped": show the failure instead of a neutral row.
              const lifecycle = lifecycleOutcome(computer)
              const detail = computer.attention?.message ?? (computer.state === "failed" ? computer.stateDetail : computer.freshness === "stale" ? computer.device ? "Offline · last known status" : "Last known status" : lifecycle?.text)
              return <ComputerListItem key={configuration.id} aria-label={configuration.name} aria-busy={availability.busy || undefined}>
                <ComputerListRow
                  name={configuration.name}
                  kindBadge={computer.device ? <DeviceBadge device={computer.device} /> : undefined}
                  iconState={lifecycle?.error ? "error" : computerIconState(computer)}
                  tone={computer.freshness === "stale" ? "warning" : lifecycle?.error ? "error" : computerRowTone(computer)}
                  icon={availability.busy ? <span className="relative shrink-0">
                    <ListRowIcon><Monitor className="size-3.5" /></ListRowIcon>
                    <span className="absolute -top-1 -right-1 grid size-3.5 place-items-center rounded-full bg-background"><Loader2 className="size-2.5 animate-spin" aria-hidden="true" /></span>
                  </span> : undefined}
                  detail={<span className="flex min-w-0 flex-wrap items-center gap-x-1.5 gap-y-0.5">
                    <span className="truncate" title={availability.busy ? activity?.title ?? computer.stateDetail : detail}>
                      {availability.busy ? <span className="font-medium text-warning">{computer.lifecycleAction ? (computer.lifecycleAction === "restart" ? "Restarting…" : computer.lifecycleAction === "stop" ? "Stopping…" : "Starting…") : activity?.title ?? (computer.state === "starting" ? computer.stateDetail : "Working…")}</span> : <><ComputerStateLabel state={computer.state} />{detail && <span> · {detail}</span>}</>}
                    </span>
                    {pendingSecrets.length > 0 && <SecretChangesLabel computer={configuration.name} state={computer.state} secrets={pendingSecrets} />}
                  </span>}
                  detailClassName="overflow-visible whitespace-normal"
                  actions={<>
                    {!availability.busy && (!repair || computer.device) && (review
                      ? <ComputerAction label={`See logs for ${configuration.name}`} onClick={() => actions.openSilo({ computer: target, computerSection: "logs" })}><Terminal /></ComputerAction>
                      : computer.freshness === "stale" ? <ComputerAction label={`Retry ${configuration.name} status`} onClick={actions.refresh}><RotateCw /></ComputerAction>
                        : availability.canOpen ? <>
                          <ComputerAction label={`Open ${configuration.name} in ${source.preferences.terminal}`} onClick={() => actions.openTerminal(target)}><Terminal /></ComputerAction>
                          <ComputerAction label={`Open ${configuration.name} in ${source.preferences.editor}`} onClick={() => openFolders(configuration.id)}><Code /></ComputerAction>
                        </> : availability.canStart ? <ComputerAction label={`Start ${configuration.name}`} onClick={() => guardedActions.startComputer(target)}><Play /></ComputerAction>
                          : <ComputerAction label="Open Silo" onClick={() => actions.openSilo({ computer: target })}><SiloMark /></ComputerAction>)}
                    <ComputerActions computer={computer} source={source} actions={guardedActions} onFolders={() => openFolders(configuration.id)} onConfirm={(action) => setConfirmation({ computer: target, action })} />
                  </>}
                />
                <RepositoryPushes computer={computer} source={source} actions={actions} />
                {startPrompt?.target === target && <ListRowDetails label={startPrompt.prompt.title} className="gap-2 pl-0">
                  <p className="text-[11px] font-medium">{startPrompt.prompt.title}</p>
                  <p className="text-[11px] text-muted-foreground">{startPrompt.prompt.description}</p>
                  <div className="flex justify-end gap-1.5">
                    <Button variant="ghost" size="xs" onClick={() => setStartPrompt(null)}>Cancel</Button>
                    <Button size="xs" disabled={!availability.canStart} onClick={() => {
                      setStartPrompt(null)
                      guarded.confirm(computer, "start")
                    }}>Start anyway</Button>
                  </div>
                </ListRowDetails>}
                {pending && <ListRowDetails label={`${pending.action === "stop" ? "Stop" : "Restart"} ${configuration.name}?`} className="gap-2 pl-0">
                  <p className="text-[11px] text-muted-foreground">{pendingPrompt?.title} {pendingPrompt?.description}</p>
                  <div className="flex justify-end gap-1.5">
                    <Button variant="ghost" size="xs" onClick={() => setConfirmation(null)}>Cancel</Button>
                    <Button variant="destructive" size="xs" disabled={pending.action === "stop" ? !availability.canStop : !availability.canRestart} onClick={() => {
                      if (pending.action === "stop" ? !availability.canStop : !availability.canRestart) return
                      setConfirmation(null)
                      if (pending.action === "stop") guardedActions.stopComputer(target)
                      else guardedActions.restartComputer(target)
                    }}>{pending.action === "stop" ? <Square /> : <RotateCw />}{pending.action === "stop" ? "Stop" : "Restart"}</Button>
                  </div>
                </ListRowDetails>}
              </ComputerListItem>
            })}
          </ol>
        </ListCard> : <div className="grid justify-items-center gap-1.5 py-8 text-center">
          <ListRowIcon><Monitor className="size-3.5" /></ListRowIcon>
          <p className="text-[13px] font-medium">No computers yet</p>
          <p className="text-[11px] text-muted-foreground">Open Silo to create your first computer.</p>
        </div>}
      </div>
      <footer className="flex shrink-0 items-center justify-between border-t px-2 py-2">
        {quitPending && stoppedByQuit.length
          ? <QuitConfirmation names={stoppedByQuit} onCancel={() => { setQuitPending(false); focusContent() }} onQuit={() => { setQuitPending(false); actions.quit() }} />
          : <>
            <Button variant="ghost" size="sm" className="gap-2" onClick={() => actions.openSilo()}><SiloMark data-icon="inline-start" /><span>Open Silo</span></Button>
            <ComputerAction label="Quit Silo" onClick={requestQuit}><Power /></ComputerAction>
          </>}
      </footer>
    </div>
  )
}
