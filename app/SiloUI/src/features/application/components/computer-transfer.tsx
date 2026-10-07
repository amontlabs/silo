import { isMac } from "@/lib/platform"
import { errorMessage } from "@/lib/error-message"
import { useEffect, useRef, useState, type ReactNode } from "react"

import { type OperationStep, dismissOperationToast, showActionFailure, showOperationFailure, showOperationNotice, showOperationProgress, showOperationSuccess } from "@/lib/operation-toast"
import type { ApplicationSource } from "@/features/application/model/application-source"
import type { BackupController, BackupPhase, VerifiedExport } from "@/features/application/model/backup-source"
import { ImportPopover, type ImportReview } from "@/features/application/components/import-popover"

/** One toast tracks the single in-flight export or import; updating it in place keeps the
 * notification tied to backend truth across navigation. */
const TRANSFER_TOAST_ID = "backup-operation"

const revealLabel = () => (isMac() ? "Show in Finder" : "Show in folder")

const stepState: Record<BackupPhase["tone"], OperationStep["state"]> = { succeeded: "done", running: "current", waiting: "pending", failed: "failed" }

export interface ComputerTransfer {
  /**
   * Pick a folder, then export a computer (or one of its checkpoints) as a background toast.
   * Resolves once that export finished: with the verified export, or null when none was
   * produced (folder picker dismissed, export unavailable, busy, refused, failed or cancelled;
   * the toast explains why). Never rejects. "Export, then delete" deletes only on a result.
   */
  exportComputer: (computerName: string, checkpoint?: { id: string; name: string }) => Promise<VerifiedExport | null>
  /** Pick an export file, validate it, then open the import review popover. */
  beginImport: () => Promise<void>
  /** Wraps the computer list's Add button: the import review popover anchors to it. */
  importPopover: (anchor: ReactNode) => ReactNode
}

/**
 * Drives export and import as background notifications: one toast reflects the global
 * `backup.state.operation`, updated in place. Export starts directly after a folder is
 * chosen; import opens a review popover first, then continues in the toast.
 */
export function useComputerTransfer(backup: BackupController, options: { source: ApplicationSource; openComputer?: (id: string) => void }): ComputerTransfer {
  const backupRef = useRef(backup)
  backupRef.current = backup
  const optionsRef = useRef(options)
  optionsRef.current = options
  // Retry handler for the current operation, replayed from a failure toast's Retry action.
  const retryRef = useRef<(() => void) | undefined>(undefined)
  const reviewDraftRef = useRef<Extract<ImportReview, { kind: "review" }> | null>(null)
  const [review, setReview] = useState<ImportReview | null>(null)
  // A result already present when the app loads is from a previous session: never toast it, unless
  // the backend marks it unseen (an upgrade produced it, or an unreadable record was set aside).
  const seenOperation = useRef(false)
  // The export file check behind the open review; closing the review aborts it (E-27).
  const inspectionRef = useRef<AbortController | null>(null)
  useEffect(() => () => inspectionRef.current?.abort(), [])

  function closeReview() {
    inspectionRef.current?.abort()
    inspectionRef.current = null
    setReview(null)
  }

  async function exportComputer(computerName: string, checkpoint?: { id: string; name: string }): Promise<VerifiedExport | null> {
    const controller = backupRef.current
    if (controller.state.availability === "unavailable") {
      showOperationFailure(TRANSFER_TOAST_ID, "Export is unavailable", { description: controller.state.availabilityMessage ?? "Export is not available in this Silo build. No computer data was changed.", native: false })
      return null
    }
    let destination: string | null
    try { destination = await controller.actions.chooseDestination() }
    catch (error) { showOperationFailure(TRANSFER_TOAST_ID, "Could not choose a folder", { description: `${errorMessage(error)} No export was created.`, native: false }); return null }
    if (!destination) return null
    // Another export or import is running: leave its toast and Retry untouched (E-52).
    if (backupRef.current.state.operation?.kind === "running") return null
    retryRef.current = () => { void exportComputer(computerName, checkpoint) }
    // The toast, driven by the export and import state, reports every outcome; only a verified export resolves.
    return backupRef.current.actions.exportAndVerify(destination, [computerName], checkpoint?.id).catch(() => null)
  }

  async function beginImport() {
    const controller = backupRef.current
    if (controller.state.availability === "unavailable") {
      setReview({ kind: "invalid", reason: controller.state.availabilityMessage ?? "Import is not available in this Silo build." })
      return
    }
    controller.actions.dismissOperation()
    inspectionRef.current?.abort()
    const inspection = new AbortController()
    inspectionRef.current = inspection
    let result
    try { result = await controller.actions.chooseArchive(() => { if (!inspection.signal.aborted) setReview({ kind: "checking" }) }, inspection.signal) }
    catch (error) { if (!inspection.signal.aborted) setReview({ kind: "invalid", reason: errorMessage(error) }); return }
    // A review closed (or replaced) while the file was checked ignores the late result.
    if (inspection.signal.aborted) return
    inspectionRef.current = null
    if (!result) { setReview(null); return }
    if (!result.valid) { setReview({ kind: "invalid", reason: result.reason ?? "This export file could not be validated." }); return }
    const base = result.archive.computers[0] || "computer"
    setReview({ kind: "review", archive: result.archive, sourceName: base, newName: `${base}-imported` })
  }

  // Reflect the authoritative operation as a single toast, keyed by a stable id.
  useEffect(() => {
    const controller = backupRef.current
    const operation = controller.state.operation
    const firstLoad = !seenOperation.current
    seenOperation.current = true
    if (!operation) { dismissOperationToast(TRANSFER_TOAST_ID); return }
    const isExport = operation.operation === "backup"
    // The user was not shown this result as it happened. It appears as an ordinary result notification
    // that stays until dismissed; dismissing it removes it like any other result and is what
    // acknowledges it here. Closing Silo first leaves it unseen, so the next launch shows it again.
    // The window may be hidden at launch, so merely showing the notification acknowledges nothing.
    const unseen = operation.kind === "result" && controller.state.resultUnseen === true

    if (firstLoad && operation.kind !== "running" && !unseen) {
      // Stale result: stay silent, and clear it when the imported computer has since been deleted.
      const target = operation.operation === "restore" && operation.outcome === "success" ? operation.targetName : undefined
      if (target && !optionsRef.current.source.computers.some(({ configuration, device }) => !device && configuration.name === target)) backupRef.current.actions.dismissOperation()
      return
    }

    if (operation.kind === "running") {
      const title = isExport
        ? (operation.archive.checkpointName ? `Exporting checkpoint “${operation.archive.checkpointName}”` : `Exporting ${operation.archive.computers.join(", ") || "computer"}`)
        : `Importing ${operation.targetName ?? "computer"}`
      const phase = operation.phases.find((entry) => entry.tone === "running") ?? operation.phases.at(-1)
      const onCancel = () => backupRef.current.actions.cancelOperation()
      showOperationProgress(TRANSFER_TOAST_ID, {
        title,
        step: phase ? (phase.detail || phase.title) : undefined,
        steps: operation.phases.map((entry) => ({ label: entry.title, state: stepState[entry.tone] })),
        progress: operation.indeterminate ? null : operation.progress / 100,
        // The backend sends canCancel: false once Cancel would no longer be honoured (an import saving its computer).
        cancel: operation.canCancel === false ? undefined : isExport
          ? { onCancel, confirm: { prompt: "Stop exporting? No export file is saved.", confirmLabel: "Stop", keepLabel: "Keep going" } }
          : { onCancel, confirm: { prompt: "Stop importing? No computer is added.", confirmLabel: "Stop", keepLabel: "Keep going" } },
      })
      return
    }

    const dismiss = () => backupRef.current.actions.dismissOperation()

    if (operation.outcome === "success") {
      const archive = operation.archive
      const title = isExport ? (archive.checkpointName ? "Checkpoint exported" : "Exported") : `Imported ${operation.targetName ?? archive.computers[0] ?? "computer"}`
      const action = isExport
        ? { label: revealLabel(), onClick: () => {
            backupRef.current.actions.revealArchive(archive).catch((error) => showActionFailure("Could not reveal the export", errorMessage(error), undefined, { native: false }))
          } }
        : (() => {
            // Resolve the computer when Open is clicked: the application snapshot can
            // list the imported computer only after this toast appears (E-57).
            const name = operation.targetName
            if (!name || !optionsRef.current.openComputer) return undefined
            return { label: "Open", onClick: () => {
              const { source, openComputer } = optionsRef.current
              const match = source.computers.find(({ configuration, device }) => !device && configuration.name === name)
              if (match && openComputer) openComputer(match.configuration.id)
              else showActionFailure(`Could not open ${name}`, "Silo does not list this computer yet. Refresh, then open it from the computer list.", undefined, { native: false })
            } }
          })()
      showOperationSuccess(TRANSFER_TOAST_ID, title, { description: isExport ? `${archive.name} · ${archive.size}` : "Stopped and verified.", action, persist: true, native: false, computer: isExport ? undefined : operation.targetName ?? archive.computers[0], onDismiss: dismiss })
      return
    }

    if (operation.outcome === "cancelled") {
      // A result nobody has seen stays until dismissed; the usual cancellation notice disappears by itself.
      const description = unseen && operation.detail ? <div className="grid gap-1"><p>{operation.message}</p><p className="text-muted-foreground">{operation.detail}</p></div> : operation.message
      showOperationNotice(TRANSFER_TOAST_ID, operation.title, { description, duration: unseen ? Infinity : undefined, onDismiss: dismiss })
      return
    }

    // Failed: persistent, actionable.
    const description = <div className="grid gap-1"><p>{operation.message}</p>{operation.detail && <p className="text-muted-foreground">{operation.detail}</p>}</div>
    const retry = retryRef.current ? { label: "Retry", onClick: () => retryRef.current?.() } : undefined
    showOperationFailure(TRANSFER_TOAST_ID, operation.title, { description, action: retry, onDismiss: dismiss, tone: "error", native: false })
    // oxlint-disable-next-line react-hooks/exhaustive-deps
  }, [backup.state.operation])

  const importPopover = (anchor: ReactNode) => <ImportPopover
    anchor={anchor}
    source={optionsRef.current.source}
    review={review}
    onReview={setReview}
    onClose={closeReview}
    onRetry={() => { closeReview(); void beginImport() }}
    onImport={(archive, newName, sourceName) => {
      reviewDraftRef.current = { kind: "review", archive, sourceName, newName }
      retryRef.current = () => { if (reviewDraftRef.current) setReview(reviewDraftRef.current) }
      backupRef.current.actions.startRestore(archive, newName, sourceName)
      setReview(null)
    }}
  />

  return { exportComputer, beginImport, importPopover }
}
