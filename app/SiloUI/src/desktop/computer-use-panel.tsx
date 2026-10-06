import { useId } from "react"
import { CircleAlert, Monitor } from "lucide-react"
import { Button } from "@/components/ui/button"
import { Progress } from "@/components/ui/progress"
import { ListCard, ListRow, ListRowIcon } from "@/components/list-row"
import { deviceOfComputer, useChatGptApp, useComputerUseBridge, type ChatGptAppStore } from "./computer-use-bridge"
import { ComputerUseApprovalSwitch, approvalStatus, useComputerUseApproval, type ComputerUseApprovalController } from "./computer-use-approval"
import type { ChatGptAppStatus } from "./linux-desktop-state"

function megabytes(bytes: number) {
  return `${Math.max(0, Math.round(bytes / 1_000_000)).toLocaleString("en-US")} MB`
}

function ErrorLine({ message, actionLabel, onAction, onDismiss, busy }: { message: string; actionLabel?: string; onAction?: () => void; onDismiss?: () => void; busy?: boolean }) {
  return <div role="alert" className="flex items-start gap-1.5 text-xs text-destructive">
    <CircleAlert aria-hidden="true" className="mt-0.5 size-3.5 shrink-0" />
    <span className="min-w-0 flex-1 break-words">{message}</span>
    {onAction && <Button type="button" size="xs" variant="outline" disabled={busy} onClick={onAction}>{actionLabel}</Button>}
    {onDismiss && <Button type="button" size="xs" variant="ghost" aria-label="Dismiss error" onClick={onDismiss}>Dismiss</Button>}
  </div>
}

/** What Silo tells the user about the ChatGPT app. It downloads it by itself: nothing to accept. */
export const CHATGPT_DOWNLOAD_NOTE = "Silo downloads ChatGPT for Linux from OpenAI so agents in your computers can use the Linux desktop."

/** Progress and result of preparing the ChatGPT app, read-only: a Retry button appears only with `onRetry`. */
export function ChatGptAppProgress({ status, busy = false, onRetry }: { status: ChatGptAppStatus; busy?: boolean; onRetry?: () => void }) {
  switch (status.state) {
    case "downloading": {
      const total = status.totalBytes ?? 0
      const percent = total > 0 ? Math.min(100, Math.round(status.receivedBytes / total * 100)) : null
      return <div className="grid gap-1.5 text-xs" role="status">
        <p>Downloading ChatGPT for Linux · {total > 0 ? `${megabytes(status.receivedBytes)} of ${megabytes(total)}` : megabytes(status.receivedBytes)}</p>
        <Progress aria-label="Download progress" value={percent} />
      </div>
    }
    case "verifying": return <p role="status" className="text-xs">Verifying the download…</p>
    case "extracting": return <p role="status" className="text-xs">Unpacking ChatGPT for Linux…</p>
    case "failed": return <div role="alert" className="flex items-start gap-1.5 text-xs text-destructive">
      <CircleAlert aria-hidden="true" className="mt-0.5 size-3.5 shrink-0" />
      <span className="min-w-0 flex-1 break-words">{status.reason}</span>
      {onRetry && <Button type="button" size="xs" variant="outline" disabled={busy} onClick={onRetry}>Retry</Button>}
    </div>
    case "idle": return <p role="status" className="text-xs text-muted-foreground">ChatGPT for Linux will download shortly.</p>
    default: return null
  }
}

/** The progress of the ChatGPT download of a computer's device. `retry` adds Retry to a failure, which acts on that device's download.
 * `fallbackReason` is shown with Retry until the status itself is read. */
export function ChatGptAppStatusView({ store, retry = false, fallbackReason, active = true }: { store: ChatGptAppStore | undefined; retry?: boolean; fallbackReason?: string | null; active?: boolean }) {
  const { status, busy, error, loadError, subscriptionError } = useChatGptApp(store, active)
  const recovery = subscriptionError ?? loadError
  const refresh = recovery ? <ErrorLine message={recovery} actionLabel="Refresh status" onAction={store ? () => { void store.refresh() } : undefined} /> : null
  const onRetry = retry && store ? () => { void store.retry() } : undefined
  const retryError = error ? <ErrorLine message={error} onDismiss={() => store?.dismissError()} /> : null
  if (!status) {
    const fallback = fallbackReason ? <ErrorLine message={fallbackReason} actionLabel={onRetry ? "Retry" : undefined} onAction={onRetry} busy={busy} /> : null
    return <>{refresh ?? fallback}{retryError}</>
  }
  if (status.state === "ready" || status.state === "unknown") return <>{fallbackReason && <ErrorLine message={fallbackReason} />}{refresh}{retryError}</>
  return <div className="grid gap-1.5">
    <ChatGptAppProgress status={status} busy={busy} onRetry={onRetry} />
    {refresh}
    {retryError}
  </div>
}

const APPROVAL_HINT = "Agents such as Claude Code and Codex use this computer's desktop without asking first. Not a security boundary."

function Problem({ message, title, tone = "error", actionLabel, onAction, actionDisabled, actionTitle, onDismiss }: {
  message: string; title?: string | null; tone?: "error" | "warning"; actionLabel?: string; onAction?: () => void; actionDisabled?: boolean; actionTitle?: string; onDismiss?: () => void
}) {
  return <div role={tone === "error" ? "alert" : "note"} className={tone === "error" ? "flex items-center gap-1.5 px-2 py-1.5 text-[11px] text-destructive" : "flex items-center gap-1.5 px-2 py-1.5 text-[11px] text-warning"}>
    <CircleAlert aria-hidden="true" className="size-3.5 shrink-0" />
    <span className="min-w-0 flex-1 truncate" title={title ?? message}>{message}</span>
    {onAction && <Button type="button" size="xs" variant="outline" disabled={actionDisabled} title={actionTitle} onClick={onAction}>{actionLabel}</Button>}
    {onDismiss && <Button type="button" size="xs" variant="ghost" aria-label="Dismiss error" onClick={onDismiss}>Dismiss</Button>}
  </div>
}

/** Presentational: the approval switch of one computer's computer use, and a line only when something needs attention. */
export function ComputerUsePanel({ approval, chatGpt }: {
  approval: ComputerUseApprovalController
  /** The ChatGPT download of this computer's device: how to retry it, and whether a retry is under way. */
  chatGpt?: { retry: () => void; busy: boolean; error?: string | null }
}) {
  const { computerUse, running, busy, error, loadError } = approval
  const labelId = useId()
  if (!computerUse) return loadError ? <ListCard><Problem message={loadError} actionLabel="Try again" onAction={approval.refresh} /></ListCard> : null
  const status = approvalStatus(computerUse, running)
  const downloadFailed = computerUse.state === "failed" && computerUse.cause === "app-download"
  const setupFailed = computerUse.state === "failed" && !downloadFailed
  return <ListCard divided>
    <ListRow
      icon={<ListRowIcon aria-hidden="true"><Monitor className="size-3.5" /></ListRowIcon>}
      title={<span id={labelId}>Allow without asking</span>}
      detail={<span title={APPROVAL_HINT}>{APPROVAL_HINT}</span>}
      actions={<ComputerUseApprovalSwitch approval={approval} labelledBy={labelId} />}
    />
    {downloadFailed && <Problem message="ChatGPT download failed." title={computerUse.reason} actionLabel="Retry" onAction={chatGpt?.retry} actionDisabled={!chatGpt || chatGpt.busy} />}
    {chatGpt?.error && downloadFailed && <Problem message={chatGpt.error} />}
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
  const bridge = useComputerUseBridge()
  const approval = useComputerUseApproval(computer, pollMs, active)
  // Only a failed download needs the device's ChatGPT status, for its Retry.
  const downloadFailed = approval?.computerUse?.state === "failed" && approval.computerUse.cause === "app-download"
  const store = downloadFailed ? bridge?.chatGptFor(deviceOfComputer(computer)) : undefined
  const chatGpt = useChatGptApp(store, active)
  if (!approval || (!approval.computerUse && !approval.loadError)) return null
  if (!approval.computerUse && approval.state) return null
  return <section aria-label="Computer use" className="grid gap-1.5">
    <h3 className="flex min-h-6 items-center text-xs font-medium">Computer use</h3>
    <ComputerUsePanel approval={approval} chatGpt={store ? { retry: () => { void store.retry() }, busy: chatGpt.busy, error: chatGpt.error ?? chatGpt.subscriptionError ?? chatGpt.loadError } : undefined} />
  </section>
}
