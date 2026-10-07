import { CircleAlert } from "lucide-react"
import type { ReactNode } from "react"

import { ListCard, ListRow, ListRowDetails, ListRowIcon } from "@/components/list-row"
import { LogDisclosure } from "@/components/log-disclosure"

export function SetupNotice({ title, detail, recovery, technicalDetails, action }: {
  title: string
  detail: string
  recovery?: string
  technicalDetails?: string
  action?: ReactNode
}) {
  return (
    <ListCard role="alert">
      <ListRow
        className="grid grid-cols-[auto_minmax(0,1fr)] gap-y-2 sm:flex"
        icon={<ListRowIcon className="bg-destructive/10 text-destructive" aria-hidden="true"><CircleAlert className="size-3.5" /></ListRowIcon>}
        title={<h3>{title}</h3>}
        detail={detail}
        detailClassName="whitespace-normal break-words select-text"
        actions={action && <div className="col-start-2 shrink-0">{action}</div>}
      />
      {(recovery || technicalDetails) && (
        <ListRowDetails label={`${title} details`}>
          {recovery && <p className="text-caption leading-4 text-muted-foreground select-text">{recovery}</p>}
          {technicalDetails && <LogDisclosure title="Technical details" output={technicalDetails} />}
        </ListRowDetails>
      )}
    </ListCard>
  )
}
