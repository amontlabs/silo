import { useRef } from "react"
import { HardDrive } from "lucide-react"

import { ConfirmPopover } from "@/components/confirm-popover"
import { SectionHeading } from "@/components/page"
import { ListCard, ListRow, ListRowIcon } from "@/components/list-row"
import { Button } from "@/components/ui/button"
import { runWithOperationToast, showActionFailure } from "@/lib/operation-toast"
import {
  deleteConfirmation,
  deletionSummary,
  formatBackupSize,
  preUpgradeBackupContents,
  usePreUpgradeBackup,
  usePreUpgradeBackupBackend,
} from "./pre-upgrade-backup"

/**
 * Settings, General: the copy of the previous computer storage that an upgrade keeps. The section
 * exists only while there is such a copy to show, so it disappears once the copy is deleted.
 */
export function StorageSection() {
  const state = usePreUpgradeBackup(usePreUpgradeBackupBackend())
  const attempts = useRef(0)
  const { backup, loadError } = state
  if (!state.loaded || (!backup && !loadError)) return null

  // A retry gets its own id: Sonner closes the failed toast after its Retry action runs, and a
  // toast that reuses that id would be closed with it.
  const deleteNow = (): Promise<void> => runWithOperationToast(`pre-upgrade-backup:delete:${++attempts.current}`, {
    loading: "Deleting the pre-upgrade backup",
    success: "Pre-upgrade backup deleted",
    failure: "Could not delete the pre-upgrade backup",
  }, state.remove, { retry: () => { void deleteNow() } }).then(() => undefined)
  const show = () => state.reveal().catch((cause: unknown) => showActionFailure("Could not show the pre-upgrade backup", cause))

  return <section className="grid gap-2" aria-label="Storage">
    <SectionHeading>Storage</SectionHeading>
    <ListCard>
      <ListRow
        icon={<ListRowIcon aria-hidden="true"><HardDrive className="size-3.5" /></ListRowIcon>}
        title={<h4>Pre-upgrade backup</h4>}
        detailClassName="whitespace-normal"
        detail={backup ? <>
          <span className="block">{formatBackupSize(state.size)} · {deletionSummary(backup)}</span>
          <span className="block">{preUpgradeBackupContents}</span>
          {loadError && <span role="alert" className="block text-destructive">Silo could not refresh it. {loadError}</span>}
        </> : <span role="alert" className="block text-destructive">Silo could not check for it. {loadError}</span>}
        actions={<span className="flex shrink-0 gap-1.5">
          {backup ? <>
            <Button type="button" variant="outline" size="xs" disabled={state.removing} onClick={() => { void show() }}>Show</Button>
            <ConfirmPopover align="end" tone="destructive" {...deleteConfirmation(state.size)} onConfirm={deleteNow}>
              <Button type="button" variant="outline" size="xs" disabled={state.removing}>Delete now</Button>
            </ConfirmPopover>
          </> : null}
          {loadError && <Button type="button" variant="outline" size="xs" onClick={state.retry}>Retry</Button>}
        </span>}
      />
    </ListCard>
  </section>
}
