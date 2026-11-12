import { useEffect, useState } from "react"
import { Boxes, GitMerge, GitPullRequestArrow, Globe, HardDrive, KeyRound, Network, Replace, ShieldQuestion, Terminal, TriangleAlert } from "lucide-react"

import { EmptyState } from "@/components/empty-state"
import { ListCard, ListRow, ListRowIcon, ListRowSkeleton } from "@/components/list-row"
import { PageContainer, PageHeader, SectionHeading } from "@/components/page"
import { Button } from "@/components/ui/button"
import { ApprovalDetail, type ApprovalActions } from "@/features/application/components/approval-detail"
import { ReviewRequestPage, type ReviewRequestActions } from "@/features/application/components/review-request"
import type { Approval, RequestEntry } from "@/desktop/production-schemas"
import { approvalKindLabel, approvalOutcomeText } from "@/features/application/model/approval-copy"
import { bucketSummary, statusLabels, type PendingRow, type PendingState } from "@/features/application/model/use-pending-changes"
import { useApprovalJob } from "@/features/application/model/use-approval-job"
import { formatAbsoluteTime, formatRelativeTime } from "@/lib/relative-time"
import { cn } from "@/lib/utils"
import { sanitizeAgentText } from "@/lib/visible-text"

const badgeClass = "inline-flex shrink-0 items-center gap-1 rounded-full bg-muted px-1.5 py-0.5 text-caption font-medium text-muted-foreground"

const iconClass = "size-3.5"

/** The icon of what an approval is about. */
function KindIcon({ kind }: { kind: string }) {
  switch (kind) {
    case "merge": return <GitMerge aria-hidden="true" className={iconClass} />
    case "replace_main": return <Replace aria-hidden="true" className={iconClass} />
    case "github": return <GitPullRequestArrow aria-hidden="true" className={iconClass} />
    case "secret": return <KeyRound aria-hidden="true" className={iconClass} />
    case "network": return <Network aria-hidden="true" className={iconClass} />
    case "port": return <Globe aria-hidden="true" className={iconClass} />
    case "ssh": return <Terminal aria-hidden="true" className={iconClass} />
    case "container": return <Boxes aria-hidden="true" className={iconClass} />
    case "resources": return <HardDrive aria-hidden="true" className={iconClass} />
    default: return <ShieldQuestion aria-hidden="true" className={iconClass} />
  }
}

function Ago({ at }: { at: string }) {
  return <time dateTime={at} title={formatAbsoluteTime(at)}>{formatRelativeTime(at)}</time>
}

function ApprovalRow({ approval, request, onOpen }: { approval: Approval; request?: RequestEntry; onOpen: () => void }) {
  const computerName = sanitizeAgentText(approval.computerName || "A computer")
  const summary = sanitizeAgentText(approval.summary || approvalKindLabel(approval.kind))
  return (
    <ListRow
      icon={<ListRowIcon><KindIcon kind={approval.kind} /></ListRowIcon>}
      onOpen={onOpen}
      title={<>
        <button type="button" className="truncate text-left outline-none focus-visible:underline" onClick={onOpen}>{computerName}</button>
        <span className="shrink-0 font-normal text-muted-foreground">asks</span>
        {request?.deviceName && <span className={badgeClass}>{request.deviceName}</span>}
      </>}
      detail={<>{summary} · <Ago at={approval.createdAt} /></>}
      actions={<span className={badgeClass}>{approvalKindLabel(approval.kind)}</span>}
    />
  )
}

function RequestRow({ entry, onOpen }: { entry: RequestEntry; onOpen: () => void }) {
  const { request } = entry
  const name = entry.forkName ?? request.forkId
  const target = entry.targetName ?? request.target
  return (
    <ListRow
      icon={<ListRowIcon><GitMerge aria-hidden="true" className="size-3.5" /></ListRowIcon>}
      onOpen={onOpen}
      title={<>
        <button type="button" className="truncate text-left outline-none focus-visible:underline" onClick={onOpen}>{name}</button>
        <span className="shrink-0 font-normal text-muted-foreground">into {target}</span>
        <span className={badgeClass}>{entry.deviceName ?? "This device"}</span>
      </>}
      detail={<>{bucketSummary(request.counts)} · <Ago at={request.createdAt} /></>}
      actions={<span className="shrink-0 text-caption text-muted-foreground">{statusLabels[request.status]}</span>}
    />
  )
}

function AnsweredRow({ approval, readJob, onOpen }: { approval: Approval; readJob: ApprovalActions["readApprovalJob"]; onOpen: () => void }) {
  const job = useApprovalJob(readJob, approval)
  const outcome = approvalOutcomeText(approval, job)
  return (
    <ListRow
      icon={<ListRowIcon><KindIcon kind={approval.kind} /></ListRowIcon>}
      onOpen={onOpen}
      title={<button type="button" className="truncate text-left outline-none focus-visible:underline" onClick={onOpen}>{approval.computerName || "A computer"}</button>}
      detail={<>{approval.summary || approvalKindLabel(approval.kind)} · <Ago at={approval.decidedAt ?? approval.createdAt} /></>}
      actions={<span className={cn("max-w-56 truncate text-caption", outcome.tone === "failed" ? "text-destructive" : "text-muted-foreground")} title={outcome.text}>{outcome.text}</span>}
    />
  )
}

type Opened =
  | { kind: "approval"; id: string }
  | { kind: "request"; requestId: string; deviceId?: string; deviceName?: string; targetName?: string }

/**
 * Pending: everything waiting for the person in one list, newest first. A computer's request and
 * a fork's changes to review sit side by side; a merge a computer asked for is one row. A row
 * opens its detail; approvals answered lately stay below so their outcome can be read.
 */
export function PendingChangesPage({ pending, actions, reviewActions, openApproval, listRequest, onOpenRequest }: {
  pending: PendingState
  actions: ApprovalActions
  /** What the review screen runs; the desktop's by default. */
  reviewActions?: ReviewRequestActions
  /** An approval to open, from a notification or the menu bar. */
  openApproval?: { id: string; token: number }
  /** Changes each time the person asks for the list (the sidebar's Pending, clicked on this page). */
  listRequest?: number
  /** Told when a review request is opened, naming the device when it is not this one. */
  onOpenRequest?: (requestId: string, deviceId?: string) => void
}) {
  const [opened, setOpened] = useState<Opened | null>(null)
  const [handledToken, setHandledToken] = useState(0)
  const [handledListRequest, setHandledListRequest] = useState(listRequest)
  const { waiting, answered, unreachable, error, loaded, loading, refresh } = pending

  useEffect(() => {
    // oxlint-disable-next-line react/set-state-in-effect
    if (openApproval && openApproval.token !== handledToken) { setHandledToken(openApproval.token); setOpened({ kind: "approval", id: openApproval.id }) }
  }, [openApproval, handledToken])

  useEffect(() => {
    if (listRequest === handledListRequest) return
    // oxlint-disable-next-line react/set-state-in-effect
    setHandledListRequest(listRequest)
    setOpened(null)
  }, [listRequest, handledListRequest])

  const approvalById = (id: string) =>
    waiting.flatMap(row => row.kind === "approval" && row.approval.id === id ? [row] : [])[0]
    ?? answered.flatMap(approval => approval.id === id ? [{ kind: "approval" as const, key: id, approval, request: undefined, createdAt: 0 }] : [])[0]

  const back = () => { setOpened(null); refresh() }
  if (opened?.kind === "request") {
    return <ReviewRequestPage {...opened} actions={reviewActions} onBack={back} onChanged={refresh} />
  }
  if (opened?.kind === "approval") {
    const row = approvalById(opened.id)
    if (row) {
      return <ApprovalDetail
        key={row.approval.id}
        approval={row.approval}
        requestDeviceId={row.request?.deviceId}
        requestDeviceName={row.request?.deviceName}
        targetName={row.request?.targetName}
        actions={actions}
        reviewActions={reviewActions}
        onBack={back}
        onChanged={refresh}
      />
    }
    if (loaded && !loading) {
      return (
        <PageContainer className="grid gap-3">
          <p role="status" className="text-sm text-muted-foreground">This request is no longer waiting, and it is not among the ones answered lately.</p>
          <div><Button variant="outline" size="sm" onClick={back}>Back</Button></div>
        </PageContainer>
      )
    }
  }

  const openRow = (row: PendingRow) => {
    if (row.kind === "approval") { setOpened({ kind: "approval", id: row.approval.id }); return }
    const { entry } = row
    setOpened({ kind: "request", requestId: entry.request.id, deviceId: entry.deviceId, deviceName: entry.deviceName, targetName: entry.targetName })
    onOpenRequest?.(entry.request.id, entry.deviceId)
  }
  const empty = loaded && !waiting.length && !unreachable.length
  return (
    <PageContainer className="grid gap-3">
      <PageHeader title="Pending" subtitle="Everything waiting for your answer" />
      {error && (
        <div role="alert" className="flex items-center justify-between gap-2 text-xs text-destructive">
          <span>{error}</span>
          <Button type="button" variant="ghost" size="xs" onClick={refresh}>Retry</Button>
        </div>
      )}
      {loading && !loaded && <ListCard><ListRowSkeleton label="Loading what is waiting for you" /></ListCard>}
      {empty && (
        <EmptyState icon={<GitPullRequestArrow />} title="Nothing is waiting for you" description="When an agent asks for something, or a fork has changes to review, it appears here." />
      )}
      {(waiting.length > 0 || unreachable.length > 0) && (
        <ListCard divided aria-label="Waiting for you">
          {waiting.map(row => row.kind === "approval"
            ? <ApprovalRow key={row.key} approval={row.approval} request={row.request} onOpen={() => openRow(row)} />
            : <RequestRow key={row.key} entry={row.entry} onOpen={() => openRow(row)} />)}
          {unreachable.map(device => (
            <ListRow
              key={`unavailable:${device.deviceId}`}
              icon={<ListRowIcon><TriangleAlert aria-hidden="true" className="size-3.5" /></ListRowIcon>}
              title={<><span className="truncate">{device.deviceName}</span><span className={badgeClass}>Unavailable</span></>}
              detail={device.reason}
            />
          ))}
        </ListCard>
      )}
      {answered.length > 0 && (
        <section className="grid gap-1.5" aria-label="Answered lately">
          <SectionHeading>Answered lately</SectionHeading>
          <ListCard divided aria-label="Answered">
            {answered.map(approval => (
              <AnsweredRow key={approval.id} approval={approval} readJob={actions.readApprovalJob} onOpen={() => setOpened({ kind: "approval", id: approval.id })} />
            ))}
          </ListCard>
        </section>
      )}
    </PageContainer>
  )
}
