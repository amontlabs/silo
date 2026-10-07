import { CircleAlert, RotateCw } from "lucide-react"

import { ListCard, ListRow, ListRowDetails, ListRowIcon } from "@/components/list-row"
import { Button } from "@/components/ui/button"
import type { ActiveRuntimeRepairPresentation, ApplicationActions } from "@/features/application/model/application-source"

export function SystemIssuePage({ issue, actions }: { issue: ActiveRuntimeRepairPresentation; actions: ApplicationActions }) {
  return (
    <div className="mx-auto grid w-full max-w-4xl gap-4 px-4 py-5 sm:px-6 sm:py-6">
      <h2 className="text-xs font-medium">System issue</h2>
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
          <p className="text-[11px] text-muted-foreground whitespace-pre-line">{issue.recovery ?? "Retry checks. If the runtime is still unavailable, quit and reopen Silo."}</p>
        </ListRowDetails>
      </ListCard>
    </div>
  )
}
