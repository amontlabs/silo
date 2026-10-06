import { useState, type ReactNode } from "react"
import { CircleCheck, HardDrive, TriangleAlert } from "lucide-react"

import { ConfirmPopover } from "@/components/confirm-popover"
import { ListCard, ListRow, ListRowIcon } from "@/components/list-row"
import { SiloWindow } from "@/components/silo-window"
import { Button } from "@/components/ui/button"
import { useTransferResultNotice, type TransferResultNotice, type TransferResultNoticeBackend } from "@/features/application/model/transfer-result-notice"
import {
  automaticDeletionSentence,
  deleteConfirmation,
  formatBackupSize,
  preUpgradeBackupContents,
  usePreUpgradeBackup,
  type PreUpgradeBackupBackend,
  type PreUpgradeBackupState,
} from "@/features/storage/pre-upgrade-backup"

function message(cause: unknown) {
  return cause instanceof Error ? cause.message : String(cause)
}

/**
 * Shown once after a migration that converted every computer, before the application: Silo kept the
 * previous storage as a backup, how big it is, and the date it deletes it. The application opens at
 * once when there is nothing to report or the backup cannot be read; this never blocks Silo.
 *
 * It also tells the user the outcome of an export or import the upgrade interrupted, which the
 * export and import notifications would otherwise treat as an old result. Choosing Open Silo
 * acknowledges that result, so the application does not show it again. When this screen does not
 * appear, the application shows the result itself.
 */
export function MigrationBackupGate({ backend, transferResult, children }: { backend: PreUpgradeBackupBackend; transferResult?: TransferResultNoticeBackend; children: ReactNode }) {
  // Decided once, from the first read: a refresh or a deletion later must not swap the screen.
  const [decision, setDecision] = useState<"notice" | "open" | null>(null)
  // Only the notice shows the size, so only the notice walks the backup.
  const backup = usePreUpgradeBackup(backend, { measure: decision === "notice" })
  // Only the notice shows the result, so only the notice reads it: a launch without the notice opens
  // the application, which reads it itself.
  const transfer = useTransferResultNotice(transferResult, decision === "notice")
  if (decision === null && backup.loaded) setDecision(backup.backup?.noticePending ? "notice" : "open")
  if (decision === "open") return children
  if (decision === null) return <SiloWindow title="Silo" label="Silo"><span role="status" className="sr-only">Opening Silo…</span></SiloWindow>
  return <BackupNotice state={backup} result={transfer.notice} onContinue={async () => {
    // The notice is shown once; failing to record that only shows it again at the next launch.
    void backup.acknowledge().catch((cause: unknown) => console.error("Silo pre-upgrade backup:", message(cause)))
    // The result is acknowledged before the application opens, so it does not show the same result
    // again. If that fails the result stays unseen and the application shows it: never lost.
    await transfer.acknowledge().catch((cause: unknown) => console.error("Silo export and import result:", message(cause)))
    setDecision("open")
  }} />
}

function BackupNotice({ state, result, onContinue }: { state: PreUpgradeBackupState; result: TransferResultNotice | null; onContinue: () => Promise<void> }) {
  const { backup } = state
  const [error, setError] = useState<string | null>(null)
  const [opening, setOpening] = useState(false)
  async function remove() {
    setError(null)
    try { await state.remove() } catch (cause) { setError(message(cause)) }
  }
  return <SiloWindow title="Silo" label="Silo migration complete">
    <main className="mx-auto flex min-h-0 w-full max-w-3xl flex-1 flex-col gap-4 overflow-y-auto px-6 py-8">
      <div className="flex items-start gap-3">
        <CircleCheck aria-hidden="true" className="mt-0.5 size-5 text-success" />
        <div>
          <h1 className="text-lg font-semibold">Your computers were updated</h1>
          <p className="mt-1 text-sm text-muted-foreground">Silo kept a pre-upgrade backup of their previous storage, in case something looks wrong.</p>
        </div>
      </div>
      {backup ? <>
        <ListCard>
          <ListRow
            icon={<ListRowIcon aria-hidden="true"><HardDrive className="size-3.5" /></ListRowIcon>}
            title={<h2>Pre-upgrade backup</h2>}
            detail={formatBackupSize(state.size)}
            actions={<ConfirmPopover align="end" tone="destructive" {...deleteConfirmation(state.size)} onConfirm={remove}>
              <Button type="button" variant="outline" size="xs" disabled={state.removing}>Delete now</Button>
            </ConfirmPopover>}
          />
        </ListCard>
        <p className="text-sm">{automaticDeletionSentence(backup)}</p>
        <p className="text-xs text-muted-foreground">{preUpgradeBackupContents} Going back to an earlier version of Silo isn't supported, so keep it only to check or copy something from before the upgrade. To show it or delete it later, open Settings, General, Storage.</p>
      </> : <p role="status" className="text-sm">The pre-upgrade backup was deleted.</p>}
      {result && <TransferResult result={result} />}
      {error && <p role="alert" className="rounded-md border border-destructive/30 bg-destructive/[.06] p-3 text-sm text-destructive">{error}</p>}
      <div><Button size="sm" disabled={opening} onClick={() => { setOpening(true); void onContinue() }}>Open Silo</Button></div>
    </main>
  </SiloWindow>
}

/** What became of the export or import the upgrade interrupted, in the words the export and import page uses. */
function TransferResult({ result }: { result: TransferResultNotice }) {
  const Icon = result.outcome === "success" ? CircleCheck : TriangleAlert
  const tone = result.outcome === "failed" ? "border-warning/30 bg-warning/[.07]" : "bg-muted/30"
  return <section aria-labelledby="transfer-result-title" className={`flex items-start gap-2.5 rounded-md border p-3 text-sm ${tone}`}>
    <Icon aria-hidden="true" className={`mt-0.5 size-4 shrink-0 ${result.outcome === "success" ? "text-success" : "text-warning"}`} />
    <div className="min-w-0">
      <h2 id="transfer-result-title" className="font-medium">{result.title}</h2>
      <p className="mt-0.5 text-muted-foreground">{[result.message, result.detail].filter(Boolean).join(" ")}</p>
    </div>
  </section>
}
