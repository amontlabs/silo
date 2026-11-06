import { useEffect, useState } from "react"
import { ConfirmPopover } from "@/components/confirm-popover"
import { PageContainer, PageHeader, SectionHeading } from "@/components/page"
import { Button } from "@/components/ui/button"
import { formatAbsoluteTime } from "@/lib/relative-time"
import { keepReviewRequest, openReviewRequest, showReviewRequest, throwAwayReviewRequest } from "@/desktop/production-source"
import type { KeepRequestResult, OpenRequestResult, RequestDetail } from "@/desktop/production-schemas"
import { BucketCounts, CredentialsUsed } from "./review-request-parts"

export type ReviewRequestActions = {
  show: typeof showReviewRequest
  keep: typeof keepReviewRequest
  throwAway: typeof throwAwayReviewRequest
  open: typeof openReviewRequest
}
const productionActions: ReviewRequestActions = { show: showReviewRequest, keep: keepReviewRequest, throwAway: throwAwayReviewRequest, open: openReviewRequest }

export const staleReason = "The fork or its target changed since this request was made. Refresh to review the current changes before keeping."
const message = (error: unknown) => error instanceof Error ? error.message : String(error)

/** One review request: what the fork changed, the credentials it used, and Keep, Throw away and Open fork. */
export function ReviewRequest({ requestId, deviceId, deviceName, onBack, onClose, actions = productionActions }: {
  requestId: string
  deviceId?: string
  /** Shown in the header instead of the device id. */
  deviceName?: string
  /** Called from the Back button. */
  onClose?: () => void
  /** Called after the fork is thrown away. */
  onBack?: () => void
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
      error => { if (current) setLoadError(message(error)) },
    )
    return () => { current = false }
  }, [requestId, deviceId, reload, actions])

  async function run(work: () => Promise<void>) {
    setBusy(true); setActionError(null)
    try { await work() } catch (error) { setActionError(message(error)) } finally { setBusy(false) }
  }
  const keep = (dataOk?: boolean) => run(async () => {
    setDataConfirm(false)
    const result = await actions.keep(dataOk ? { ...target, dataOk } : target)
    setKept(result)
  })
  const throwAway = () => run(async () => { await actions.throwAway(target); onBack?.() })
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

  const { request } = detail
  const refusal = kept?.refusal
  const keptNow = kept && !refusal ? kept : null
  const stale = detail.stale === true
  const keepDisabledReason = stale ? staleReason : request.status !== "open" ? `This request is ${request.status}.` : keptNow ? "Already kept." : undefined
  const subtitle = [deviceId ? `on ${deviceName ?? deviceId}` : "this device", `created ${formatAbsoluteTime(request.createdAt)}`, request.status].join(" · ")

  return (
    <PageContainer className="grid gap-4">
      <PageHeader title={`${detail.forkName} → ${request.target}`} subtitle={subtitle} actions={
        <div className="flex flex-wrap gap-2">
          {onClose && <Button variant="ghost" size="sm" onClick={onClose}>Back</Button>}
          <Button variant="outline" size="sm" disabled={busy} onClick={open}>Open fork</Button>
          <ConfirmPopover title={`Throw away ${detail.forkName}?`} description="The fork and its changes are removed. Nothing is kept into the target." confirmLabel="Throw away" tone="destructive" onConfirm={throwAway}>
            <Button variant="outline" size="sm" disabled={busy}>Throw away</Button>
          </ConfirmPopover>
          <Button size="sm" disabled={busy || keepDisabledReason !== undefined} onClick={() => keep()}>Keep</Button>
        </div>
      } />
      {keepDisabledReason && <p role={stale ? "alert" : undefined} className="text-caption text-muted-foreground">{stale ? `Stale: ${keepDisabledReason}` : keepDisabledReason}</p>}
      {stale && <Button variant="outline" size="sm" className="w-fit" onClick={() => { setDetail(null); setReload(value => value + 1) }}>Refresh</Button>}
      {keptNow && (
        <p role="status" className="text-sm">
          Kept into {keptNow.report.target}. Keep id {keptNow.request.keepId ?? keptNow.report.checkpoint ?? "unknown"}. Undo is available from the computer.
        </p>
      )}
      {refusal && (
        <div role="alert" className="grid gap-2 text-sm">
          <p className="whitespace-pre-wrap">{refusal.message}</p>
          {refusal.hint && <p className="whitespace-pre-wrap text-muted-foreground">{refusal.hint}</p>}
          {refusal.reason === "database" && (
            <ConfirmPopover title="Keep with the database change?" description="Database changes are carried into the target without a migration covering them." confirmLabel="Keep anyway" open={dataConfirm} onOpenChange={setDataConfirm} onConfirm={() => keep(true)}>
              <Button variant="outline" size="sm" className="w-fit" disabled={busy}>Keep with data changes</Button>
            </ConfirmPopover>
          )}
        </div>
      )}
      {actionError && <p role="alert" className="whitespace-pre-wrap text-sm text-destructive">{actionError}</p>}
      {opened && (
        <section className="grid gap-1 text-sm" aria-label="Open fork">
          <p>{opened.name} is {opened.started ? "started" : opened.state}.</p>
          {opened.sshAlias && <p>SSH: <code>{opened.sshAlias}</code></p>}
          {opened.urls.map(url => <p key={url}><a className="underline" href={url} target="_blank" rel="noreferrer">{url}</a></p>)}
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
export function ReviewRequestPage({ requestId, deviceId, deviceName, onBack, actions }: {
  requestId: string
  deviceId?: string
  deviceName?: string
  /** Returns to the list; also called after the fork is thrown away. */
  onBack?: () => void
  actions?: ReviewRequestActions
}) {
  return <ReviewRequest key={`${deviceId ?? ""}:${requestId}`} requestId={requestId} deviceId={deviceId} deviceName={deviceName} onBack={onBack} onClose={onBack} actions={actions} />
}
