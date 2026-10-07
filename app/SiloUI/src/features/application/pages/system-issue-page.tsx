import { CircleAlert, RotateCw } from "lucide-react"

import { ListCard, ListRow, ListRowDetails, ListRowIcon } from "@/components/list-row"
import { Button } from "@/components/ui/button"
import { PageContainer, PageHeader } from "@/components/page"
import type { ActiveRuntimeRepairPresentation, ApplicationActions } from "@/features/application/model/application-source"

export function SystemIssuePage({ issue, actions }: { issue: ActiveRuntimeRepairPresentation; actions: ApplicationActions }) {
  return (
    <PageContainer className="grid gap-4">
      <PageHeader title="System issue" />
      <ListCard role="alert" aria-live="polite">
        <ListRow
          className="grid grid-cols-[auto_minmax(0,1fr)] gap-y-2 row-hover sm:flex"
          icon={<ListRowIcon aria-hidden="true" className="bg-destructive/10 text-destructive"><CircleAlert className="size-3.5" /></ListRowIcon>}
          title={<h3>Silo runtime is unavailable</h3>}
          detail={issue.reason}
          detailClassName="whitespace-normal"
          actions={
            <div className="col-start-2 shrink-0">
              <Button variant="outline" size="xs" onClick={actions.retryRuntimeChecks} disabled={issue.checking}>
                <RotateCw aria-hidden="true" className={issue.checking ? "animate-spin motion-reduce:animate-none" : undefined} />
                {issue.checking ? "Checking…" : "Retry checks"}
              </Button>
            </div>
          }
        />
        <ListRowDetails label="Recovery instructions">
          <p className="text-caption text-muted-foreground whitespace-pre-line">{issue.recovery ?? "Retry checks. If the runtime is still unavailable, quit and reopen Silo."}</p>
        </ListRowDetails>
      </ListCard>
    </PageContainer>
  )
}
