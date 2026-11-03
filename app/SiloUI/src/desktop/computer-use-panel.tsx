import { useId } from "react"
import { CircleAlert, Monitor } from "lucide-react"
import { Button } from "@/components/ui/button"
import { ListCard, ListRow, ListRowIcon } from "@/components/list-row"
import { ComputerUseApprovalSwitch, approvalStatus, useComputerUseApproval, type ComputerUseApprovalController } from "./computer-use-approval"

const APPROVAL_HINT = "Agents such as Claude Code and Codex use this computer's desktop without asking first. Not a security boundary."

function Problem({ message, title, tone = "error", actionLabel, onAction, actionDisabled, actionTitle, onDismiss }: {
  message: string; title?: string | null; tone?: "error" | "warning"; actionLabel?: string; onAction?: () => void; actionDisabled?: boolean; actionTitle?: string; onDismiss?: () => void
}) {
  return <div role={tone === "error" ? "alert" : "note"} className={tone === "error" ? "flex items-center gap-1.5 px-2 py-1.5 text-caption text-destructive" : "flex items-center gap-1.5 px-2 py-1.5 text-caption text-warning"}>
    <CircleAlert aria-hidden="true" className="size-3.5 shrink-0" />
    <span className="min-w-0 flex-1 truncate" title={title ?? message}>{message}</span>
    {onAction && <Button type="button" size="xs" variant="outline" disabled={actionDisabled} title={actionTitle} onClick={onAction}>{actionLabel}</Button>}
    {onDismiss && <Button type="button" size="xs" variant="ghost" aria-label="Dismiss error" onClick={onDismiss}>Dismiss</Button>}
  </div>
}

/** Presentational: the approval switch of one computer's computer use, and a line only when something needs attention. */
export function ComputerUsePanel({ approval }: { approval: ComputerUseApprovalController }) {
  const { computerUse, running, busy, error, loadError } = approval
  const labelId = useId()
  if (!computerUse) return loadError ? <ListCard><Problem message={loadError} actionLabel="Try again" onAction={approval.refresh} /></ListCard> : null
  const status = approvalStatus(computerUse, running)
  const setupFailed = computerUse.state === "failed"
  return <ListCard divided>
    <ListRow
      icon={<ListRowIcon aria-hidden="true"><Monitor className="size-3.5" /></ListRowIcon>}
      title={<span id={labelId}>Allow without asking</span>}
      detail={<span title={APPROVAL_HINT}>{APPROVAL_HINT}{computerUse.driverVersion ? ` Cua Driver ${computerUse.driverVersion}.` : ""}</span>}
      actions={<ComputerUseApprovalSwitch approval={approval} labelledBy={labelId} />}
    />
    {setupFailed && <Problem message="Computer use setup failed." title={computerUse.reason} actionLabel="Try again" onAction={approval.setup}
      actionDisabled={busy || !running} actionTitle={running ? undefined : "Start the computer to try again."} />}
    {status.note && <Problem message={status.note} title={[status.note, status.detail].filter(Boolean).join(" ")} tone="warning" />}
    {error && <Problem message={error} onDismiss={approval.dismissError} />}
    {loadError && <Problem message={loadError} actionLabel="Try again" onAction={approval.refresh} />}
  </ListCard>
}

/** Reads and changes one computer's computer use. Renders nothing for pre-v4 computers or without a bridge.
 * Mount with `key={computer}`: its reads belong to one computer. */
export function ComputerUseSection({ computer, pollMs = 5000, active = true }: { computer: string; pollMs?: number; active?: boolean }) {
  const approval = useComputerUseApproval(computer, pollMs, active)
  if (!approval || (!approval.computerUse && !approval.loadError)) return null
  if (!approval.computerUse && approval.state) return null
  return <section aria-label="Computer use" className="grid gap-1.5">
    <h3 className="flex min-h-6 items-center text-xs font-medium">Computer use</h3>
    <ComputerUsePanel approval={approval} />
  </section>
}
