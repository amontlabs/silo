import { useEffect, useId, useState } from "react"

import { FormPopover } from "@/components/confirm-popover"
import { PageContainer, PageHeader, SectionHeading } from "@/components/page"
import { Button } from "@/components/ui/button"
import type { Approval, DeviceResources } from "@/desktop/production-schemas"
import { ReviewRequestPage, type ReviewRequestActions } from "@/features/application/components/review-request"
import type { ApplicationActions } from "@/features/application/model/application-source"
import { approvalKindLabel, approvalOutcomeText } from "@/features/application/model/approval-copy"
import { useApprovalJob } from "@/features/application/model/use-approval-job"
import { errorMessage } from "@/lib/error-message"
import { formatAbsoluteTime, formatRelativeTime } from "@/lib/relative-time"
import { cn } from "@/lib/utils"
import { sanitizeAgentText } from "@/lib/visible-text"

export type ApprovalActions = Pick<ApplicationActions, "decideApproval" | "readApprovalJob" | "readDeviceResources">

const gib = (bytes: number) => `${(bytes / 2 ** 30).toFixed(1)} GiB`

/** What an approval is, in the engine's words, and the agent's own note under a label that says whose words they are. */
export function ApprovalFacts({ approval }: { approval: Approval }) {
  const summary = sanitizeAgentText(approval.summary || approvalKindLabel(approval.kind), { preserveNewlines: true })
  const note = approval.note ? sanitizeAgentText(approval.note, { preserveNewlines: true }) : undefined
  return (
    <div className="grid gap-3">
      <section className="grid gap-1" aria-label="What is asked">
        <SectionHeading>What is asked</SectionHeading>
        <p className="text-sm whitespace-pre-line">{summary}</p>
      </section>
      {note && (
        <section className="grid gap-1" aria-label="The agent's note">
          <SectionHeading>The agent's own words</SectionHeading>
          <blockquote className="border-l-2 pl-3 text-sm whitespace-pre-wrap text-muted-foreground">{note}</blockquote>
          <p className="text-caption text-muted-foreground">The agent wrote this note. Silo has not checked it.</p>
        </section>
      )}
    </div>
  )
}

/** Deny, with a note the agent will read. */
export function DenyButton({ disabled, onDeny }: { disabled?: boolean; onDeny: (note?: string) => void | Promise<void> }) {
  const [note, setNote] = useState("")
  const field = useId()
  return (
    <FormPopover
      title="Deny this request?"
      description="The agent is told you said no."
      confirmLabel="Deny"
      tone="destructive"
      onSubmit={() => { const text = note.trim(); setNote(""); return onDeny(text || undefined) }}
      fields={
        <label htmlFor={field} className="grid gap-1">
          <span>Note for the agent (optional)</span>
          <textarea id={field} value={note} maxLength={500} rows={3} onChange={event => setNote(event.target.value)}
            className="w-full resize-none rounded-md border bg-transparent px-2 py-1 text-xs outline-none focus-visible:ring-2 focus-visible:ring-ring" />
        </label>
      }
    >
      <Button variant="outline" size="sm" disabled={disabled}>Deny</Button>
    </FormPopover>
  )
}

/** The device's memory and disk, next to a request for more of them. */
function ResourcesNow({ read }: { read: NonNullable<ApprovalActions["readDeviceResources"]> }) {
  const [resources, setResources] = useState<DeviceResources>()
  useEffect(() => {
    let current = true
    read().then(next => { if (current) setResources(next) }, () => {})
    return () => { current = false }
  }, [read])
  if (!resources) return null
  return (
    <section className="grid gap-1" aria-label="This device now">
      <SectionHeading>This device now</SectionHeading>
      <p className="text-sm">
        {resources.memoryAvailableBytes !== undefined && <>{gib(resources.memoryAvailableBytes)} of {gib(resources.memoryTotalBytes)} memory available. </>}
        {gib(resources.diskFreeBytes)} of disk free. {resources.runningComputers} {resources.runningComputers === 1 ? "computer" : "computers"} running.
      </p>
      {resources.message && <p className="text-caption text-muted-foreground">{resources.message}</p>}
    </section>
  )
}

function OutcomeLine({ approval, readJob }: { approval: Approval; readJob: ApprovalActions["readApprovalJob"] }) {
  const job = useApprovalJob(readJob, approval)
  const outcome = approvalOutcomeText(approval, job)
  if (approval.state === "pending") return null
  return (
    <section className="grid gap-1" aria-label="Outcome">
      <SectionHeading>{approval.state === "allowed" ? "You allowed this" : approval.state === "denied" ? "You denied this" : "Closed"}</SectionHeading>
      <p role="status" className={cn("text-sm", outcome.tone === "failed" && "text-destructive")}>{outcome.text}</p>
      {approval.decisionNote && <p className="text-caption text-muted-foreground">Your note: {approval.decisionNote}</p>}
    </section>
  )
}

/**
 * One approval. A merge shows the review request with its diff, and merging it is the answer; any
 * other kind is answered with Allow or Deny. After an answer it shows the outcome while it runs and when it ends.
 */
export function ApprovalDetail({ approval, requestDeviceId, requestDeviceName, targetName, actions, reviewActions, onBack, onChanged }: {
  approval: Approval
  requestDeviceId?: string
  requestDeviceName?: string
  targetName?: string
  actions: ApprovalActions
  reviewActions?: ReviewRequestActions
  onBack: () => void
  /** Told after an answer, so the list reads again. */
  onChanged: () => void
}) {
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const waiting = approval.state === "pending"
  // A merge opened while it waits stays in the review view after it is answered, so the result of merging stays on screen.
  const [reviewing] = useState(waiting && Boolean(approval.reviewRequestId))
  const decide = async (decision: "allow" | "deny", note?: string) => {
    if (!actions.decideApproval) return
    setBusy(true); setError(null)
    try { await actions.decideApproval({ id: approval.id, decision, ...(note ? { note } : {}) }); onChanged() }
    catch (cause) { setError(errorMessage(cause)) }
    finally { setBusy(false) }
  }
  const facts = <div className="grid gap-3">
    <ApprovalFacts approval={approval} />
    {waiting && approval.kind === "resources" && actions.readDeviceResources && <ResourcesNow read={actions.readDeviceResources} />}
    <OutcomeLine approval={approval} readJob={actions.readApprovalJob} />
    {error && <p role="alert" className="text-sm whitespace-pre-wrap text-destructive">{error}</p>}
  </div>

  if (approval.reviewRequestId && reviewing) {
    return (
      <ReviewRequestPage
        requestId={approval.reviewRequestId}
        deviceId={requestDeviceId}
        deviceName={requestDeviceName}
        targetName={targetName}
        actions={reviewActions}
        onBack={onBack}
        onChanged={onChanged}
        intro={facts}
        extraActions={<DenyButton disabled={busy} onDeny={note => decide("deny", note)} />}
      />
    )
  }

  const subtitle = [approvalKindLabel(approval.kind), formatRelativeTime(approval.createdAt), approval.state === "pending" && approval.expiresAt ? `expires ${formatAbsoluteTime(approval.expiresAt)}` : ""].filter(Boolean).join(" · ")
  const cleanComputerName = sanitizeAgentText(approval.computerName || "A computer")
  const headerTitle = (
    <span className="inline-flex items-baseline gap-2">
      <span>{cleanComputerName} asks</span>
      {approval.computerId && <span className="font-mono text-xs font-normal text-muted-foreground">{approval.computerId}</span>}
    </span>
  )
  return (
    <PageContainer className="grid gap-4">
      <PageHeader title={headerTitle} subtitle={subtitle} actions={
        <div className="flex flex-wrap gap-2">
          <Button variant="ghost" size="sm" onClick={onBack}>Back</Button>
          {waiting && <DenyButton disabled={busy} onDeny={note => decide("deny", note)} />}
          {waiting && <Button size="sm" disabled={busy || !actions.decideApproval} onClick={() => void decide("allow")}>Allow</Button>}
        </div>
      } />
      {facts}
    </PageContainer>
  )
}
