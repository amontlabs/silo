import { useState } from "react"
import { RefreshCw, ScrollText } from "lucide-react"

import { EmptyState } from "@/components/empty-state"
import { Button } from "@/components/ui/button"
import type { AuditCallerKind, AuditEntry } from "@/desktop/production-schemas"
import type { ApplicationActions, ApplicationComputer } from "@/features/application/model/application-source"
import { auditAction, auditOutcome, auditTarget, auditWho } from "@/features/application/model/audit-copy"
import { useAuditLog } from "@/features/application/model/use-audit-log"
import { formatLocalActivityTime } from "@/lib/format-date"
import { cn } from "@/lib/utils"

const selectClass = "h-7 rounded-md border bg-transparent px-2 text-xs outline-none focus-visible:ring-2 focus-visible:ring-ring"

const callers: ReadonlyArray<{ value: AuditCallerKind | ""; label: string }> = [
  { value: "", label: "Anyone" },
  { value: "owner", label: "You" },
  { value: "device", label: "Other devices" },
  { value: "computer", label: "Computers" },
]

const unopenableReason = "This request is no longer in the list. Silo keeps only the ones waiting and the ones answered lately."

function Row({ entry, computers, onOpenApproval, openableApprovals }: { entry: AuditEntry; computers: readonly ApplicationComputer[]; onOpenApproval?: (id: string) => void; openableApprovals?: ReadonlySet<string> }) {
  const when = entry.lastAt ?? entry.at
  const target = auditTarget(entry, computers)
  return (
    <tr className="border-b border-border align-top select-text">
      <td className="px-3 py-1.5 whitespace-nowrap text-muted-foreground" title={entry.count > 1 ? `First ${entry.at}, latest ${when}` : entry.at}>
        <time dateTime={when}>{formatLocalActivityTime(when)}</time>
      </td>
      <td className="px-3 py-1.5">{auditWho(entry, computers)}</td>
      <td className="px-3 py-1.5">
        <span>{auditAction(entry.method)}</span>
        {entry.count > 1 && <span className="ml-1.5 text-caption text-muted-foreground" title={`${entry.count} times in a row`}>×{entry.count}</span>}
      </td>
      <td className="px-3 py-1.5 text-muted-foreground">{target ?? "—"}</td>
      <td className={cn("px-3 py-1.5", (entry.outcome === "error" || entry.outcome === "refused") && "text-destructive")}>
        {auditOutcome(entry.outcome)}
        {entry.errorCode && <span className="ml-1 text-caption text-muted-foreground">({entry.errorCode})</span>}
      </td>
      <td className="px-3 py-1.5 text-right">
        {entry.approvalId && (onOpenApproval
          ? openableApprovals && !openableApprovals.has(entry.approvalId)
            ? <span title={unopenableReason}><Button variant="link" size="xs" disabled>Open request</Button></span>
            : <Button variant="link" size="xs" onClick={() => onOpenApproval(entry.approvalId!)}>Open request</Button>
          : <span className="text-caption text-muted-foreground">Request {entry.approvalId}</span>)}
      </td>
    </tr>
  )
}

/**
 * Who did what: the engine's record of the calls that changed something and of the ones it
 * refused or held for an answer, newest first, as a plain table. Filters by computer and by who;
 * older rows load from the cursor. A row opens its request when `openableApprovals` (the ones
 * the Pending page can show) has it, or always when that is not known.
 */
export function AuditLog({ read, computers, active, onOpenApproval, openableApprovals }: {
  read: ApplicationActions["listAudit"]
  computers: readonly ApplicationComputer[]
  active: boolean
  onOpenApproval?: (approvalId: string) => void
  openableApprovals?: ReadonlySet<string>
}) {
  const [computerId, setComputerId] = useState("")
  const [callerKind, setCallerKind] = useState<AuditCallerKind | "">("")
  const log = useAuditLog(read, { computerId: computerId || undefined, callerKind: callerKind || undefined }, active)
  if (!read) return <EmptyState icon={<ScrollText />} title="Not available" description="This version of Silo cannot read who did what." />
  return (
    <div className="flex h-full min-h-0 min-w-0 flex-col gap-3">
      <div className="flex shrink-0 flex-wrap items-center gap-2">
        <label className="flex items-center gap-1.5 text-xs text-muted-foreground">Computer
          <select aria-label="Computer" className={selectClass} value={computerId} onChange={event => setComputerId(event.target.value)}>
            <option value="">All computers</option>
            {computers.map(({ configuration }) => <option key={configuration.id} value={configuration.id}>{configuration.name}</option>)}
          </select>
        </label>
        <label className="flex items-center gap-1.5 text-xs text-muted-foreground">Who
          <select aria-label="Who" className={selectClass} value={callerKind} onChange={event => setCallerKind(event.target.value as AuditCallerKind | "")}>
            {callers.map(({ value, label }) => <option key={value} value={value}>{label}</option>)}
          </select>
        </label>
        <Button size="icon-xs" variant="outline" aria-label="Refresh" title="Refresh" disabled={log.loading} onClick={log.refresh}><RefreshCw aria-hidden="true" className={log.loading ? "motion-safe:animate-spin motion-reduce:animate-none" : undefined} /></Button>
      </div>
      {log.error && <p role="alert" className="text-xs text-destructive">{log.error}</p>}
      {log.entries.length === 0 && !log.loading && !log.error
        ? <EmptyState icon={<ScrollText />} title="Nothing recorded yet" description="Calls that change something, and the ones Silo refuses or holds for you, appear here." />
        : (
          <div className="min-h-0 flex-1 overflow-auto overscroll-contain rounded-md border border-border">
            <table className="w-full text-xs" aria-label="Who did what" aria-busy={log.loading || undefined}>
              <thead className="sticky top-0 bg-background text-left text-muted-foreground">
                <tr className="border-b border-border">
                  <th scope="col" className="px-3 py-1.5 font-medium">Time</th>
                  <th scope="col" className="px-3 py-1.5 font-medium">Who</th>
                  <th scope="col" className="px-3 py-1.5 font-medium">What</th>
                  <th scope="col" className="px-3 py-1.5 font-medium">Computer</th>
                  <th scope="col" className="px-3 py-1.5 font-medium">Outcome</th>
                  <th scope="col" className="px-3 py-1.5 font-medium"><span className="sr-only">Request</span></th>
                </tr>
              </thead>
              <tbody>
                {log.entries.map(entry => <Row key={entry.id} entry={entry} computers={computers} onOpenApproval={onOpenApproval} openableApprovals={openableApprovals} />)}
              </tbody>
            </table>
            {log.hasMore && (
              <div className="flex justify-center p-2">
                <Button size="xs" variant="outline" disabled={log.loading} onClick={log.loadMore}>Load more</Button>
              </div>
            )}
          </div>
        )}
    </div>
  )
}
