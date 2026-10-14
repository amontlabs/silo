export interface BackupArchive {
  name: string
  archivePath: string
  completedLabel: string
  size: string
  destination: string
  computers: string[]
  /** The checkpoint an export packages; absent for an export of the current state. */
  checkpointName?: string
}

export type BackupOperationKind = "backup" | "restore"
export type BackupPhaseTone = "waiting" | "running" | "succeeded" | "failed"

export interface BackupPhase {
  title: string
  detail: string
  tone: BackupPhaseTone
}

export type BackupOperation = {
  operation: BackupOperationKind
  archive: BackupArchive
  runningNames: string[]
  targetName?: string
} & (
  | { kind: "running"; progress: number; indeterminate?: boolean; canCancel?: boolean; phases: BackupPhase[] }
  | { kind: "result"; outcome: "success" | "failed" | "cancelled"; title: string; message: string; detail?: string }
)

export interface BackupState {
  /** Changes when an authoritative replacement should discard the open review. */
  snapshotId: string
  operationId?: string
  availability: "available" | "unavailable"
  availabilityMessage?: string
  requiredSpaceGB?: number
  availableSpaceGB?: number
  unsupportedStorage?: { computer: string; label: string }
  destination?: string
  archives: BackupArchive[]
  operation: BackupOperation | null
  /**
   * `operation` is a result the user was not shown as it happened: one an upgrade produced, or the notice
   * that an unreadable export or import record was set aside. Unlike any other result present when Silo
   * opens, it is not stale: it is shown, and stays until it is dismissed.
   */
  resultUnseen?: boolean
}

/** An export that completed and passed verification. */
export interface VerifiedExport {
  /** The backend operation id, equal to `BackupState.operationId` while its result is shown. */
  operationId: string
  archive: BackupArchive
}

/**
 * Why `exportAndVerify` produced no verified export:
 * - `busy`: another export or import was running; nothing started.
 * - `rejected`: Silo refused to start it (the message says why); nothing started.
 * - `failed` / `cancelled`: this export ended without a verified file.
 * - `unavailable`: its result can no longer be observed (the window closed, or
 *   the result was replaced before it was read). Do not assume it succeeded.
 */
export type ExportIncompleteReason = "busy" | "rejected" | "failed" | "cancelled" | "unavailable"

export class ExportIncompleteError extends Error {
  readonly reason: ExportIncompleteReason
  /** Present once Silo accepted the export. */
  readonly operationId?: string

  constructor(reason: ExportIncompleteReason, message: string, operationId?: string) {
    super(message)
    this.name = "ExportIncompleteError"
    this.reason = reason
    this.operationId = operationId
  }
}

export interface BackupActions {
  chooseDestination: () => Promise<string | null>
  /**
   * Picks an export file and checks it. Aborting `signal` stops the check (a large file
   * can take minutes); the caller must then ignore the result.
   */
  chooseArchive: (onSelected?: (archivePath: string) => void, signal?: AbortSignal) => Promise<{ archive: BackupArchive; valid: boolean; reason?: string; warnings?: string[] } | null>
  inspectArchive: (selection: BackupArchive, signal?: AbortSignal) => Promise<{ archive: BackupArchive; valid: boolean; reason?: string; warnings?: string[] }>
  /** Starts an export and returns immediately; the result appears in `BackupState.operation`. */
  startBackup: (destination: string, computers: string[], checkpointId?: string) => void
  /**
   * Starts an export to a folder from `chooseDestination` and settles only with
   * that export's own result: resolves once it completed and its file was
   * verified, and rejects with an `ExportIncompleteError` otherwise. Progress and
   * the result are also shown through `BackupState.operation`, like `startBackup`.
   * "Export, then delete" deletes only after this resolves.
   */
  exportAndVerify: (destination: string, computers: string[], checkpointId?: string) => Promise<VerifiedExport>
  startRestore: (archive: BackupArchive, newName: string, sourceName?: string) => void
  cancelOperation: () => void
  dismissOperation: () => void
  /** Reveal a completed export in Finder (macOS) or the file manager (Linux). */
  revealArchive: (archive: BackupArchive) => Promise<void>
}

export interface BackupController {
  state: BackupState
  actions: BackupActions
}
