import { useEffect, useState, type ReactNode } from "react"
import { ConfirmPopover } from "@/components/confirm-popover"
import { PageContainer, PageHeader, SectionHeading } from "@/components/page"
import { Button } from "@/components/ui/button"
import { formatAbsoluteTime } from "@/lib/relative-time"
import { keepReviewRequest, openReviewRequest, showReviewRequest, throwAwayReviewRequest } from "@/desktop/production-source"
import type { KeepRequestResult, OpenRequestResult, RequestDetail } from "@/desktop/production-schemas"
import { CopyButton } from "@/components/copy-button"
import { statusLabels } from "@/features/application/model/use-pending-changes"
import { errorMessage } from "@/lib/error-message"
import { BucketCounts, CredentialsUsed } from "./review-request-parts"
import { MergeConflicts } from "./merge-conflicts"
import { refusalHint } from "@/features/application/model/merge-copy"
import type { MergeChoice } from "@/desktop/production-source"

export type ReviewRequestActions = {
  show: typeof showReviewRequest
  keep: typeof keepReviewRequest
  throwAway: typeof throwAwayReviewRequest
  open: typeof openReviewRequest
}
const productionActions: ReviewRequestActions = { show: showReviewRequest, keep: keepReviewRequest, throwAway: throwAwayReviewRequest, open: openReviewRequest }

export const staleReason = "The fork has changed since this was requested. Delete the fork; the agent can ask again."
export const fastForwardNote = "Main had not changed since the fork was made, so nothing could conflict. Everything was applied."

/** One review request: what the fork changed, the credentials it used, and Merge, Delete fork and Open fork. */
export function ReviewRequest({ requestId, deviceId, deviceName, targetName, onBack, onClose, onChanged, extraActions, intro, actions = productionActions }: {
  requestId: string
  deviceId?: string
  /** Shown in the header instead of the device id. */
  deviceName?: string
  /** Shown in the header instead of the request's target id. */
  targetName?: string
  /** Called from the Back button. */
  onClose?: () => void
  /** Called after the fork is deleted. */
  onBack?: () => void
  /** Called after a merge or a deletion, so a list showing the request can read again. */
  onChanged?: () => void
  /** Further header buttons; the Pending page adds Deny for a merge a computer asked for. */
  extraActions?: ReactNode
  /** Shown under the header: what a computer's agent asked, for a merge it asked for. */
  intro?: ReactNode
  actions?: ReviewRequestActions
}) {
  const [detail, setDetail] = useState<RequestDetail | null>(null)
  const [loadError, setLoadError] = useState<string | null>(null)
  const [reload, setReload] = useState(0)
  const [busy, setBusy] = useState(false)
  const [kept, setKept] = useState<KeepRequestResult | null>(null)
  const [actionError, setActionError] = useState<string | null>(null)
  const [opened, setOpened] = useState<OpenRequestResult | null>(null)
  const [dataConfirm, setDataConfirm] = useState(false)
  const target = { requestId, deviceId }

  useEffect(() => {
    let current = true
    actions.show({ requestId, deviceId }).then(
      next => { if (current) setDetail(next) },
      error => { if (current) setLoadError(errorMessage(error)) },
    )
    return () => { current = false }
  }, [requestId, deviceId, reload, actions])

  async function run(work: () => Promise<void>) {
    setBusy(true); setActionError(null)
    try { await work() } catch (error) { setActionError(errorMessage(error)) } finally { setBusy(false) }
  }
  const keep = (options: { dataOk?: boolean; choices?: Record<string, MergeChoice> } = {}) => run(async () => {
    setDataConfirm(false)
    const result = await actions.keep({ ...target, ...(options.dataOk ? { dataOk: true } : {}), ...(options.choices ? { choices: options.choices } : {}) })
    setKept(result)
    if (!result.refusal) setReload(value => value + 1)
    onChanged?.()
  })
  const throwAway = () => run(async () => { await actions.throwAway(target); onChanged?.(); onBack?.() })
  const open = () => run(async () => { setOpened(await actions.open(target)) })

  if (loadError) {
    return (
      <PageContainer className="grid gap-3">
        <p role="alert" className="text-sm text-destructive">Couldn’t load this request: {loadError}</p>
        <div><Button variant="outline" size="sm" onClick={() => { setLoadError(null); setReload(value => value + 1) }}>Try again</Button></div>
      </PageContainer>
    )
  }
  if (!detail) return <PageContainer><p role="status" className="text-sm text-muted-foreground">Loading request…</p></PageContainer>

  const refusal = kept?.refusal
  const keptNow = kept && !refusal ? kept : null
  const request = keptNow?.request ?? detail.request
  const targetLabel = targetName ?? detail.targetName ?? request.target
  const resolved = request.status !== "open"
  const stale = detail.stale === true
  const keepDisabledReason = stale ? staleReason : resolved ? `This request is ${statusLabels[request.status].toLowerCase()}.` : keptNow ? "Already merged." : undefined
  const subtitle = [deviceId ? `on ${deviceName ?? deviceId}` : "this device", `created ${formatAbsoluteTime(request.createdAt)}`, statusLabels[request.status]].join(" · ")

  return (
    <PageContainer className="grid gap-4">
      <PageHeader title={`${detail.forkName} → ${targetLabel}`} subtitle={subtitle} actions={
        <div className="flex flex-wrap gap-2">
          {onClose && <Button variant="ghost" size="sm" onClick={onClose}>Back</Button>}
          <Button variant="outline" size="sm" disabled={busy || request.status === "thrown-away"} onClick={open}>Open fork</Button>
          <ConfirmPopover title={`Delete the fork ${detail.forkName}?`} description={`This deletes the fork and everything in it. Nothing is merged into ${targetLabel}.`} confirmLabel="Delete fork" tone="destructive" onConfirm={throwAway}>
            <Button variant="outline" size="sm" disabled={busy || resolved}>Delete fork</Button>
          </ConfirmPopover>
          {extraActions}
          <ConfirmPopover title={`Merge into ${targetLabel}?`} description={`The changes in ${detail.forkName} are brought into ${targetLabel}.`} confirmLabel="Merge" onConfirm={() => keep()}>
            <Button size="sm" disabled={busy || keepDisabledReason !== undefined}>Merge</Button>
          </ConfirmPopover>
        </div>
      } />
      {intro}
      {keepDisabledReason && <p role={stale ? "alert" : undefined} className="text-caption text-muted-foreground">{stale ? `Stale: ${keepDisabledReason}` : keepDisabledReason}</p>}
      {stale && <Button variant="outline" size="sm" className="w-fit" onClick={() => { setDetail(null); setReload(value => value + 1) }}>Reload</Button>}
      {keptNow && (
        <p role="status" className="text-sm">
          Merged into {targetLabel}. Undo with <code>silo undo {targetLabel}</code>.
          {keptNow.report.fastForward && <> {fastForwardNote}</>}
        </p>
      )}
      {refusal && (
        <div role="alert" className="grid gap-2 text-sm">
          <p className="whitespace-pre-wrap">{refusal.message}</p>
          {refusalHint(refusal) && <p className="whitespace-pre-wrap text-muted-foreground">{refusalHint(refusal)}</p>}
          {refusal.reason === "database" && (
            <ConfirmPopover title="Merge with the database change?" description="Database changes are carried into the target without a migration covering them." confirmLabel="Merge anyway" open={dataConfirm} onOpenChange={setDataConfirm} onConfirm={() => keep({ dataOk: true })}>
              <Button variant="outline" size="sm" className="w-fit" disabled={busy}>Merge with data changes</Button>
            </ConfirmPopover>
          )}
        </div>
      )}
      {refusal && refusal.conflicts.length > 0 && (
        <MergeConflicts key={refusal.conflicts.map(({ id, item }) => id || item).join("\n")} conflicts={refusal.conflicts} forkName={detail.forkName} busy={busy} onMerge={choices => void keep({ choices })} />
      )}
      {actionError && <p role="alert" className="whitespace-pre-wrap text-sm text-destructive">{actionError}</p>}
      {opened && (
        <section className="grid gap-1 text-sm" aria-label="Open fork">
          <p>{opened.name} is {opened.started ? "started" : opened.state}.</p>
          {opened.sshAlias && <p>SSH: <code>{opened.sshAlias}</code></p>}
          {opened.urls.map(url => (
            <p key={url} className="flex items-center gap-2">
              <code>{url}</code>
              <CopyButton size="xs" variant="ghost" value={url} labels={{ idle: `Copy ${url}`, copied: "Copied", failed: "Copy failed" }} text={{ idle: "Copy", copied: "Copied", failed: "Copy failed" }} />
            </p>
          ))}
        </section>
      )}
      <BucketCounts counts={detail.counts} />
      <section className="grid gap-1.5" aria-label="Diff">
        <SectionHeading>Diff</SectionHeading>
        <pre className="max-h-96 overflow-auto rounded-md border p-3 font-mono text-xs whitespace-pre">{detail.text || "No changes to show."}</pre>
      </section>
      <CredentialsUsed credentials={detail.credentials} />
    </PageContainer>
  )
}

/** The page slice B renders for `onOpenRequest(requestId, deviceId?)`. */
export function ReviewRequestPage({ requestId, deviceId, deviceName, targetName, onBack, onChanged, extraActions, intro, actions }: {
  requestId: string
  deviceId?: string
  deviceName?: string
  targetName?: string
  /** Returns to the list; also called after the fork is deleted. */
  onBack?: () => void
  onChanged?: () => void
  extraActions?: ReactNode
  intro?: ReactNode
  actions?: ReviewRequestActions
}) {
  return <ReviewRequest key={`${deviceId ?? ""}:${requestId}`} requestId={requestId} deviceId={deviceId} deviceName={deviceName} targetName={targetName} onBack={onBack} onClose={onBack} onChanged={onChanged} extraActions={extraActions} intro={intro} actions={actions} />
}
