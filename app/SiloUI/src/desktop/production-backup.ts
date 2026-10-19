import { z } from "zod"

import { ExportIncompleteError, type BackupArchive, type BackupController, type BackupOperation, type BackupState, type VerifiedExport } from "@/features/application/model/backup-source"

import type { ProductionContext } from "./production-context"
import { backupFailure, backupResultKey, errorMessage, shareStructure } from "./production-helpers"
import { archiveInspectionShape } from "./production-schemas"
import type { ProductionSnapshot } from "./production-types"

/** What export and import need from the rest of the source. */
export interface BackupDependencies {
  /** The state subscribers see, which includes frontend overlays. */
  view: () => ProductionSnapshot
  /** Marks state published outside a read, so reads that started earlier are dropped. */
  bumpRefreshSequence: () => void
  /** The number of the latest state read that started. */
  readSequence: () => number
  refresh: () => Promise<void>
  reportActionFailure: (key: string, title: string, message: string) => void
  reportUnavailable: (message: string) => void
}

/** Export and import: the operation this window started, the results it dismissed, and the waiters for an export's own result. */
export function createBackupControls({ native, snapshot, publish, disposed, view, bumpRefreshSequence, readSequence, refresh, reportActionFailure, reportUnavailable }: ProductionContext & BackupDependencies) {
  let pendingBackupOperation = false
  let localBackupOperation: BackupOperation | null = null
  const dismissedBackupResults = new Set<string>()
  // Exports awaiting their own result (E-59). Each sees every backend backup
  // state read, with the read's sequence, or null when the source is disposed.
  const exportWaiters = new Set<(state: BackupState | null, sequence: number) => void>()

  /** The backend's export and import state with this window's own operation and dismissed results applied. */
  function derive(backup: BackupState): BackupState {
    let operation = backup.operation
    if (operation?.kind === "result" && dismissedBackupResults.has(backupResultKey(backup, operation))) operation = null
    if (localBackupOperation && operation !== localBackupOperation) {
      if (!operation) operation = localBackupOperation
      else {
        localBackupOperation = null
        if (operation.kind === "running") dismissedBackupResults.clear()
      }
    }
    // The marker belongs to the result the runtime reported, not to one dismissed here or replaced by a local operation.
    const { resultUnseen, ...reported } = backup
    return { ...reported, operation, ...(resultUnseen && operation === backup.operation && { resultUnseen }) }
  }

  function showPendingBackup(operation: "backup" | "restore", archive: BackupArchive, targetName?: string) {
    bumpRefreshSequence()
    // A new operation replaces the previous result, here and in the runtime (E-49),
    // so it does not come back after a relaunch.
    const previous = view().backup.operation
    if (previous?.kind === "result" && !(localBackupOperation && shareStructure(previous, localBackupOperation) === previous)) {
      dismissedBackupResults.add(backupResultKey(view().backup, previous))
      void native.invoke("dismiss_backup_operation", { expectedOperation: previous, expectedOperationId: view().backup.operationId ?? null }).catch(() => {})
    }
    localBackupOperation = { operation, archive, targetName, runningNames: [], kind: "running", progress: 0, indeterminate: true, canCancel: false,
      phases: [{ title: operation === "backup" ? "Preparing export" : "Checking export file", detail: operation === "backup" ? "Preparing the selected computers." : "Verifying the export file before importing it.", tone: "running" }],
    }
    publish({ ...snapshot(), backup: { ...snapshot().backup, operation: localBackupOperation } })
  }

  /** Checks an export file under a request id so aborting `signal` stops the check (E-27).
   * The check changes no state, so no refresh follows it. */
  async function inspectBackupArchive(archivePath: string, signal?: AbortSignal) {
    signal?.throwIfAborted()
    const requestId = crypto.randomUUID()
    const cancel = () => { void native.invoke("cancel_backup_inspection", { requestId }).catch(() => undefined) }
    signal?.addEventListener("abort", cancel, { once: true })
    try { return archiveInspectionShape.parse(await native.invoke("inspect_backup_archive", { archivePath, requestId })) }
    finally { signal?.removeEventListener("abort", cancel) }
  }

  /** Settles with the result the backend reports under `operationId`. A state read
   * that began after the export started and shows another id means the result is gone. */
  function waitForExport(operationId: string): Promise<VerifiedExport> {
    // Reads are numbered as they start: one numbered after this began knows the export.
    const startedAfter = readSequence()
    return new Promise((resolve, reject) => {
      const waiter = (state: BackupState | null, sequence: number) => {
        const operation = state?.operation
        if (state?.operationId === operationId && operation?.kind !== "result") return
        if (state && state.operationId !== operationId && sequence <= startedAfter) return
        exportWaiters.delete(waiter)
        if (state?.operationId === operationId && operation?.kind === "result") {
          if (operation.operation === "backup" && operation.outcome === "success") resolve({ operationId, archive: operation.archive })
          else reject(new ExportIncompleteError(operation.outcome === "cancelled" ? "cancelled" : "failed", operation.message, operationId))
          return
        }
        reject(new ExportIncompleteError("unavailable", state
          ? "Silo no longer reports this export's result. Check the export folder before relying on it."
          : "Silo stopped tracking this export before it finished.", operationId))
      }
      if (disposed()) waiter(null, readSequence())
      else exportWaiters.add(waiter)
    })
  }

  const backupActions: BackupController["actions"] = {
    async chooseDestination() {
      const selected = await native.invoke<string | null>("choose_backup_destination")
      if (selected) await refresh()
      return selected
    },
    async chooseArchive(onSelected, signal) {
      const archivePath = await native.invoke<string | null>("choose_backup_archive")
      if (!archivePath || signal?.aborted) return null
      onSelected?.(archivePath)
      return inspectBackupArchive(archivePath, signal)
    },
    inspectArchive: (archive, signal) => inspectBackupArchive(archive.archivePath, signal),
    startBackup(destination, computers, checkpointId) {
      if (pendingBackupOperation || view().backup.operation?.kind === "running") return
      backupActions.exportAndVerify(destination, computers, checkpointId).catch(() => undefined)
    },
    async exportAndVerify(destination, computers, checkpointId) {
      if (pendingBackupOperation || view().backup.operation?.kind === "running") throw new ExportIncompleteError("busy", "Another export or import is running.")
      pendingBackupOperation = true
      const archive: BackupArchive = { name: "Export file", archivePath: "", completedLabel: "Not completed", size: "Unknown", destination, computers }
      showPendingBackup("backup", archive)
      let operationId: string
      try { operationId = z.string().min(1).parse(await native.invoke("start_backup", { destination, computers, ...(checkpointId && { checkpointId }) })) }
      catch (cause) {
        localBackupOperation = backupFailure("backup", archive, errorMessage(cause))
        publish({ ...snapshot(), backup: { ...snapshot().backup, operation: localBackupOperation } })
        throw new ExportIncompleteError("rejected", errorMessage(cause))
      }
      finally { pendingBackupOperation = false }
      const completion = waitForExport(operationId)
      if (localBackupOperation?.kind === "running") dismissedBackupResults.clear()
      void refresh()
      return completion
    },
    startRestore(archive, newName, sourceName) {
      if (pendingBackupOperation || view().backup.operation?.kind === "running") return
      pendingBackupOperation = true
      showPendingBackup("restore", archive, newName)
      void native.invoke("start_restore", { archivePath: archive.archivePath, newName, ...(sourceName && { sourceName }) }).then(() => { if (localBackupOperation?.kind === "running") dismissedBackupResults.clear(); return refresh() }).catch((cause) => {
        localBackupOperation = backupFailure("restore", archive, errorMessage(cause), newName)
        publish({ ...snapshot(), backup: { ...snapshot().backup, operation: localBackupOperation } })
      }).finally(() => { pendingBackupOperation = false })
    },
    cancelOperation() { void native.invoke("cancel_backup_operation").then(() => refresh()).catch((cause) => reportUnavailable(`Export or import cancellation failed: ${errorMessage(cause)} The operation may still be running.`)) },
    async revealArchive(archive) {
      await native.invoke("reveal_backup_archive", { archivePath: archive.archivePath })
    },
    dismissOperation() {
      const operation = view().backup.operation
      if (operation?.kind !== "result") return
      // A rejected start is reported only here; the runtime has nothing to dismiss.
      // The view reuses an equal earlier object, so compare by content.
      if (localBackupOperation && shareStructure(operation, localBackupOperation) === operation) {
        localBackupOperation = null
        publish({ ...snapshot(), backup: { ...snapshot().backup, operation: snapshot().backup.operation === operation ? null : snapshot().backup.operation } })
        return
      }
      const key = backupResultKey(view().backup, operation)
      dismissedBackupResults.add(key)
      publish({ ...snapshot() })
      const restore = () => { dismissedBackupResults.delete(key); publish({ ...snapshot() }) }
      void native.invoke("dismiss_backup_operation", { expectedOperation: operation, expectedOperationId: view().backup.operationId ?? null })
        .then((value) => {
          if (z.boolean().parse(value)) return
          // The runtime still holds a result (it changed, or is still being resolved): show it.
          restore()
          void refresh()
        })
        .catch((cause: unknown) => {
          restore()
          reportActionFailure("backup-dismiss", "Could not dismiss the result", errorMessage(cause))
        })
    },
  }

  /** Hands a backend backup state read to the exports waiting for their result. */
  function notifyRead(state: BackupState, sequence: number) {
    exportWaiters.forEach(waiter => waiter(state, sequence))
  }

  /** Settles every waiting export because the source is closing. */
  function close() {
    exportWaiters.forEach(waiter => waiter(null, readSequence()))
  }

  return { actions: backupActions, derive, notifyRead, close }
}
