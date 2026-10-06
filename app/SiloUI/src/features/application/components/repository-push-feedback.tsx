import { useEffect, useState, type ReactNode } from "react"
import { CircleAlert, CircleCheck, Loader2, RotateCw } from "lucide-react"

import { ConfirmBody, ConfirmPopover } from "@/components/confirm-popover"
import { Button } from "@/components/ui/button"
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover"
import { commitLabel, pushTarget, shortCommit } from "@/features/application/model/repository-push"
import type { ApplicationRepository, RepositoryPushOperation, RepositoryPushTarget } from "@/features/application/model/application-source"

export type PushRepository = (computer: string, repositoryPath: string, commitCount: number, target: RepositoryPushTarget) => void

/** Every push names its repository and branch first (owner decision 1); the host then pushes exactly this commit. */
function pushConfirmation(target: RepositoryPushTarget, commitCount: number) {
  return {
    title: `Push to ${target.repository}?`,
    description: `Branch ${target.branch} · ${commitLabel(commitCount)} · ${shortCommit(target.commit)}`,
    confirmLabel: "Push",
  }
}

const UNCONFIRMABLE = "Silo cannot tell where this repository pushes. It needs a GitHub origin; refresh repositories, or update Silo on the device that runs this computer."

/**
 * The push button: asks for confirmation naming the repository, branch and commit, then pushes that
 * target. Disabled when the computer did not report a GitHub destination.
 */
export function RepositoryPushButton({ repository, disabled = false, label, onPush, children }: {
  repository: ApplicationRepository
  disabled?: boolean
  /** Accessible name of the button. */
  label?: string
  onPush: (target: RepositoryPushTarget) => void
  children: ReactNode
}) {
  const target = pushTarget(repository)
  const button = <Button variant="outline" size="xs" disabled={disabled || !target} aria-label={label} title={target ? undefined : UNCONFIRMABLE}>{children}</Button>
  if (!target || disabled) return button
  return <ConfirmPopover {...pushConfirmation(target, repository.ahead)} onConfirm={() => onPush(target)}>{button}</ConfirmPopover>
}

/** Compact in-row state for a push. Results are announced by notifications; only states that need attention or a decision stay. */
export function RepositoryPushFeedback({
  operation,
  computer,
  repositoryPath,
  repository,
  onPush,
  onDismiss,
  showSuccess = false,
  disabled = false,
}: {
  operation: RepositoryPushOperation
  computer: string
  repositoryPath: string
  /** The repository as the computer reports it now; Retry confirms and pushes its current target. */
  repository?: ApplicationRepository
  onPush: (target: RepositoryPushTarget) => void
  onDismiss: (computer: string, repositoryPath: string) => void
  /** Show the success line and clear it after a few seconds. For surfaces without notifications. */
  showSuccess?: boolean
  /** The computer must be available to retry a push. */
  disabled?: boolean
}) {
  useEffect(() => {
    if (!showSuccess || operation.status !== "succeeded") return
    const timer = window.setTimeout(() => onDismiss(computer, repositoryPath), 4_000)
    return () => window.clearTimeout(timer)
  }, [showSuccess, operation.status, onDismiss, repositoryPath, computer])

  if (operation.status === "pushing") {
    return (
      <div className="flex h-6 items-center gap-1.5 text-xs text-muted-foreground" role="status" aria-live="polite" aria-atomic="true">
        <Loader2 className="size-3.5 animate-spin" aria-hidden="true" />
        {operation.message ?? `Pushing ${commitLabel(operation.commitCount)}…`}
      </div>
    )
  }
  if (operation.status === "unknown") {
    return <div className="flex h-6 min-w-0 items-center gap-1.5 text-xs text-muted-foreground" role="status">
      <CircleAlert className="size-3.5 shrink-0" aria-hidden="true" />
      <span className="truncate" title={operation.message}>{operation.message}</span>
      <Button className="shrink-0" variant="outline" size="xs" onClick={() => onDismiss(computer, repositoryPath)}>I’ve checked GitHub</Button>
    </div>
  }
  if (operation.status === "succeeded") {
    if (!showSuccess) return null
    return (
      <div className="flex h-6 items-center gap-1.5 text-xs text-success" role="status" aria-live="polite" aria-atomic="true">
        <CircleCheck className="size-3.5" aria-hidden="true" />
        Pushed {commitLabel(operation.commitCount)}.
      </div>
    )
  }
  return <FailedPush operation={operation} repositoryPath={repositoryPath} repository={repository} onPush={onPush} disabled={disabled} />
}

function FailedPush({ operation, repositoryPath, repository, onPush, disabled }: {
  disabled: boolean
  operation: Extract<RepositoryPushOperation, { status: "failed" }>
  repositoryPath: string
  repository?: ApplicationRepository
  onPush: (target: RepositoryPushTarget) => void
}) {
  const [open, setOpen] = useState(false)
  const [confirming, setConfirming] = useState(false)
  const target = repository ? pushTarget(repository) : null
  function change(next: boolean) {
    setOpen(next)
    if (!next) setConfirming(false)
  }
  return (
    <div className="flex h-6 items-center gap-1.5">
      <Popover open={open} onOpenChange={change}>
        <PopoverTrigger asChild>
          <Button variant="ghost" size="xs" className="text-destructive hover:text-destructive" aria-label={`Push failed for ${repositoryPath}. Show details`}>
            <CircleAlert aria-hidden="true" />
            Push failed
          </Button>
        </PopoverTrigger>
        <PopoverContent align="start" className="grid w-80 max-w-[calc(100vw-2rem)] gap-2 text-xs">
          {confirming && !disabled && target && repository
            ? <ConfirmBody {...pushConfirmation(target, repository.ahead)} onConfirm={() => onPush(target)} onClose={() => change(false)} />
            : <>
              <p className="text-destructive">{operation.message}</p>
              {operation.diagnosticDetails && <pre className="max-h-48 overflow-auto rounded-md bg-muted px-2.5 py-2 font-mono text-[10px] leading-4 whitespace-pre-wrap text-muted-foreground">{operation.diagnosticDetails}</pre>}
              <Button className="justify-self-start" variant="outline" size="xs" disabled={disabled || !target} title={target ? undefined : UNCONFIRMABLE} onClick={() => setConfirming(true)} aria-label={`Retry push for ${repositoryPath}`}>
                <RotateCw aria-hidden="true" />
                Retry
              </Button>
            </>}
        </PopoverContent>
      </Popover>
    </div>
  )
}
