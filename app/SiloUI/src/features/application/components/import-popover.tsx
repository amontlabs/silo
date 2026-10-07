import { useId, type ReactNode } from "react"

import { FormPopover } from "@/components/confirm-popover"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Progress } from "@/components/ui/progress"
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select"
import type { ApplicationSource } from "@/features/application/model/application-source"
import type { BackupArchive } from "@/features/application/model/backup-source"
import { validateComputerName } from "@/features/onboarding/model/computer-configuration"

export type ImportReview =
  | { kind: "checking" }
  | { kind: "invalid"; reason: string }
  | { kind: "review"; archive: BackupArchive; sourceName: string; newName: string }

/** Import review popover anchored to the computer list's Add button: shows the export file summary,
 * picks a source computer when several are present, and names the new computer. */
export function ImportPopover({ source, review, anchor, onReview, onImport, onClose, onRetry }: {
  source: ApplicationSource
  review: ImportReview | null
  anchor: ReactNode
  onReview: (review: ImportReview) => void
  onImport: (archive: BackupArchive, newName: string, sourceName: string) => void
  onClose: () => void
  onRetry: () => void
}) {
  const nameErrorID = useId()
  const isReview = review?.kind === "review" ? review : null
  const nameError = isReview ? validateComputerName(isReview.newName) : undefined
  const nameConflict = isReview
    ? source.computers.filter((w) => !w.device).some(({ configuration }) => configuration.name.toLowerCase() === isReview.newName.toLowerCase())
    : false
  const validationError = nameError ?? (isReview && nameConflict ? `A computer named ${isReview.newName} already exists.` : undefined)
  const title = isReview ? `Import ${isReview.archive.name}` : review?.kind === "invalid" ? "This export cannot be imported" : "Checking export"
  const fields = !review ? null : review.kind === "checking"
    ? <Progress value={null} aria-label="Import validation progress" />
    : review.kind === "invalid"
    ? <p className="text-destructive">{review.reason} No computer data changed.</p>
    : <div className="grid gap-2">
        <p className="text-muted-foreground">{review.archive.size} · {review.archive.computers.length === 1 ? review.archive.computers[0] : `${review.archive.computers.length} computers`}. Imported as a new stopped computer; existing computers and the export file stay unchanged.</p>
        {review.archive.computers.length > 1 && <label className="grid gap-1">Computer to import<Select value={review.sourceName} onValueChange={(sourceName) => onReview({ ...review, sourceName, newName: review.newName === `${review.sourceName}-imported` ? `${sourceName}-imported` : review.newName })}><SelectTrigger size="sm" aria-label="Computer to import"><SelectValue /></SelectTrigger><SelectContent>{review.archive.computers.map((name) => <SelectItem key={name} value={name}>{name}</SelectItem>)}</SelectContent></Select></label>}
        <div className="grid gap-1">
          <label htmlFor={`${nameErrorID}-input`}>New computer name</label>
          <Input technical id={`${nameErrorID}-input`} value={review.newName} aria-invalid={Boolean(validationError)} aria-describedby={validationError ? nameErrorID : undefined} onChange={(event) => onReview({ ...review, newName: event.target.value })} />
          {validationError && <span id={nameErrorID} className="text-destructive">{validationError}</span>}
        </div>
      </div>
  return <FormPopover
    open={review !== null}
    onOpenChange={(open) => { if (!open) onClose() }}
    anchor={<span className="inline-flex">{anchor}</span>}
    align="end"
    title={title}
    fields={fields}
    confirmLabel="Import"
    canSubmit={Boolean(isReview) && !nameError && !nameConflict}
    onSubmit={() => { if (isReview) onImport(isReview.archive, isReview.newName, isReview.sourceName) }}
    description={review?.kind === "invalid" ? <Button type="button" variant="outline" size="xs" onClick={onRetry}>Choose another file</Button> : undefined}
  />
}
