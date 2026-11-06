import { GitPullRequestArrow, TriangleAlert } from "lucide-react"

import { EmptyState } from "@/components/empty-state"
import { ListCard, ListRow, ListRowIcon, ListRowSkeleton } from "@/components/list-row"
import { PageContainer, PageHeader } from "@/components/page"
import { Button } from "@/components/ui/button"
import type { RequestEntry } from "@/desktop/production-schemas"
import { bucketSummary, usePendingChanges } from "@/features/application/model/use-pending-changes"
import type { ApplicationActions } from "@/features/application/model/application-source"
import { formatLocalTimestamp } from "@/lib/format-date"

const statusLabels: Record<RequestEntry["request"]["status"], string> = {
  open: "Open", keeping: "Keeping…", kept: "Kept", "thrown-away": "Thrown away",
}

const badgeClass = "inline-flex shrink-0 items-center gap-1 rounded-full bg-muted px-1.5 py-0.5 text-caption font-medium text-muted-foreground"

function RequestRow({ entry, onOpen }: { entry: RequestEntry; onOpen: () => void }) {
  const { request } = entry
  const name = entry.forkName ?? request.forkId
  const target = entry.targetName ?? request.target
  return (
    <ListRow
      icon={<ListRowIcon><GitPullRequestArrow aria-hidden="true" className="size-3.5" /></ListRowIcon>}
      onOpen={onOpen}
      title={<>
        <button type="button" className="truncate text-left outline-none focus-visible:underline" onClick={onOpen}>{name}</button>
        <span className="shrink-0 font-normal text-muted-foreground">into {target}</span>
        <span className={badgeClass}>{entry.deviceName ?? "This device"}</span>
      </>}
      detail={`${bucketSummary(request.counts)} · ${formatLocalTimestamp(Date.parse(request.createdAt))}`}
      actions={<span className="shrink-0 text-caption text-muted-foreground">{statusLabels[request.status]}</span>}
    />
  )
}

/**
 * Pending changes: the review requests of this device and every connected device in one list.
 * Selecting a row calls `onOpenRequest`, which opens the review screen.
 */
export function PendingChangesPage({ listReviewRequests, active = true, onOpenRequest }: {
  listReviewRequests: ApplicationActions["listReviewRequests"]
  active?: boolean
  /** Opens the review screen for a request, naming the device when it is not this one. */
  onOpenRequest: (requestId: string, deviceId?: string) => void
}) {
  const { list, error, loading, refresh } = usePendingChanges(listReviewRequests, active)
  const requests = list?.requests ?? []
  const unreachable = list?.unreachable ?? []
  return (
    <PageContainer className="grid gap-2">
      <PageHeader title="Pending changes" subtitle="Changes in forks, waiting for review" />
      {error && (
        <div role="alert" className="flex items-center justify-between gap-2 text-xs text-destructive">
          <span>{error}</span>
          <Button type="button" variant="ghost" size="xs" onClick={refresh}>Retry</Button>
        </div>
      )}
      {loading && !list && <ListCard><ListRowSkeleton label="Loading pending changes" /></ListCard>}
      {list && !requests.length && !unreachable.length && (
        <EmptyState icon={<GitPullRequestArrow />} title="No pending changes" description="When a fork has changes to review, they appear here." />
      )}
      {(requests.length > 0 || unreachable.length > 0) && (
        <ListCard divided aria-label="Pending changes">
          {requests.map(entry => (
            <RequestRow key={`${entry.deviceId ?? ""}:${entry.request.id}`} entry={entry} onOpen={() => onOpenRequest(entry.request.id, entry.deviceId)} />
          ))}
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
    </PageContainer>
  )
}
