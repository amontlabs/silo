import { useEffect, useRef, useState } from "react"

import { ExportIncompleteError, type BackupArchive, type BackupController, type BackupOperation, type BackupOperationKind, type BackupPhase, type VerifiedExport } from "@/features/application/model/backup-source"
import type { ApplicationSource } from "@/features/application/model/application-source"

export const backupFixtureModes = [
  "success", "backup-failed", "invalid-archive", "restore-failed",
  "unsupported-storage", "space-blocked", "stop-failed", "capture-failed", "cancel-backup", "logins-found",
  "restore-conflict", "restore-storage", "cancel-restore",
] as const
export type BackupFixtureMode = (typeof backupFixtureModes)[number]

export function backupFixtureModeFromSearch(search: string): BackupFixtureMode | undefined {
  const requested = new URLSearchParams(search).get("backup-operation")
  return backupFixtureModes.find((mode) => mode === requested)
}

export function initialBackupArchive(source: ApplicationSource): BackupArchive {
  return {
    name: source.backup.lastArchive,
    archivePath: `${source.backup.destination}/${source.backup.lastArchive}`,
    completedLabel: source.backup.completedLabel,
    size: source.backup.compressedSize,
    destination: source.backup.destination,
    computers: source.computers.map(({ configuration }) => configuration.name),
  }
}

const backupPhases: Array<[string, string]> = [
  ["Prepare export", "Saving a checkpoint after flushing computer files."],
  ["Save disk copies", "Saving each managed disk."],
  ["Write and verify export file", "Writing a self-contained export file."],
  ["Finalize export", "Saving the durable result."],
]
const restorePhases: Array<[string, string]> = [
  ["Validate export", "Checking the manifest and checksum."],
  ["Create new computer", "Writing managed disk data."],
  ["Apply Silo settings", "Importing the computer settings."],
  ["Verify new computer", "Checking the new stopped computer."],
]

function phasesFor(operation: BackupOperationKind, step: number): BackupPhase[] {
  return (operation === "backup" ? backupPhases : restorePhases).map(([title, detail], index) => ({
    title,
    detail: index < step ? "Completed." : detail,
    tone: index < step ? "succeeded" as const : index === step ? "running" as const : "waiting" as const,
  }))
}

interface BackupFixtureOptions {
  source: ApplicationSource
  previewMode?: BackupFixtureMode
  onRestoreComplete?: (targetName: string) => void
  /** A result already present when the window opens, and whether the user has not been shown it (see `BackupState.resultUnseen`). */
  initialResult?: { operation: BackupOperation; unseen: boolean }
}

type RunningFixture = { operation: BackupOperationKind; archive: BackupArchive; runningNames: string[]; targetName?: string; step: number }

function resultFor(running: RunningFixture, mode: BackupFixtureMode): Extract<BackupOperation, { kind: "result" }> {
  const common = { operation: running.operation, archive: running.archive, runningNames: running.runningNames, ...(running.targetName && { targetName: running.targetName }) }
  if (running.operation === "backup") {
    if (mode === "stop-failed") return { ...common, kind: "result", outcome: "failed", title: "Export could not start", message: "dev could not be saved. No export file was created.", detail: "No computer data changed." }
    if (mode === "capture-failed") return { ...common, kind: "result", outcome: "failed", title: "Could not save the disk copy", message: "The disk copy failed. No export file was created.", detail: "Earlier exports were not changed." }
    if (mode === "backup-failed") return { ...common, kind: "result", outcome: "failed", title: "Export could not be verified", message: "The destination disconnected while writing. The incomplete temporary file was removed.", detail: "Earlier exports were not changed." }
    return { ...common, kind: "result", outcome: "success", title: "Export ready", message: `${running.archive.name} · ${running.archive.size} · checksum verified`, detail: `Saved in ${running.archive.destination}.` }
  }
  if (mode === "restore-failed") return { ...common, kind: "result", outcome: "failed", title: "Import did not complete", message: `The new disk failed verification. The incomplete ${running.targetName} computer was removed.`, detail: "The export file and existing computers were not changed." }
  return { ...common, kind: "result", outcome: "success", title: `${running.targetName} is ready`, message: "The new computer was imported and verified. It is stopped.", detail: "Disk files and settings were imported; running programs were not." }
}

export function useBackupFixture({ source, previewMode = "success", onRestoreComplete, initialResult }: BackupFixtureOptions): BackupController {
  const snapshotId = `${JSON.stringify(source.backup)}:${previewMode}`
  const [archives, setArchives] = useState<BackupArchive[]>(() => source.backup.lastArchive ? [initialBackupArchive(source)] : [])
  const [running, setRunning] = useState<RunningFixture | null>(null)
  const [result, setResult] = useState<BackupOperation | null>(initialResult?.operation ?? null)
  const [resultUnseen, setResultUnseen] = useState(initialResult?.unseen ?? false)
  // The export awaited through `exportAndVerify`, settled with its fixture result.
  const awaitedExport = useRef<{ operationId: string; resolve: (value: VerifiedExport) => void; reject: (error: Error) => void } | null>(null)

  useEffect(() => {
    if (!running) return
    const timer = window.setTimeout(() => {
      if (running.step < 3) { setRunning({ ...running, step: running.step + 1 }); return }
      const result = resultFor(running, previewMode)
      const awaited = running.operation === "backup" ? awaitedExport.current : null
      if (awaited) {
        awaitedExport.current = null
        if (result.outcome === "success") awaited.resolve({ operationId: awaited.operationId, archive: result.archive })
        else awaited.reject(new ExportIncompleteError("failed", result.message, awaited.operationId))
      }
      if (result.outcome === "success" && running.operation === "backup") setArchives((current) => [running.archive, ...current])
      if (result.outcome === "success" && running.operation === "restore" && running.targetName) onRestoreComplete?.(running.targetName)
      setResult(result)
      setResultUnseen(false)
      setRunning(null)
    }, 900)
    return () => window.clearTimeout(timer)
  }, [running, previewMode, onRestoreComplete])

  function start(operation: BackupOperationKind, archive: BackupArchive, selected: string[], targetName?: string) {
    setResult(null)
    setResultUnseen(false)
    setRunning({ operation, archive, targetName, step: 0, runningNames: source.computers.filter(({ configuration, state }) => selected.includes(configuration.name) && state === "running").map(({ configuration }) => configuration.name) })
  }

  function startBackup(destination: string, computers: string[], checkpointId?: string) {
    const base = computers.length === 1 ? computers[0] : "Silo-Export"
    const name = `${base}${checkpointId ? "-checkpoint" : ""}-${new Date().toISOString().slice(0, 10)}.silo-backup`
    const checkpointName = checkpointId ? source.computers.find(({ configuration }) => configuration.name === computers[0])?.checkpoints?.find(({ id }) => id === checkpointId)?.name : undefined
    start("backup", { name, archivePath: `${destination}/${name}`, completedLabel: "Just now", size: source.backup.compressedSize, destination, computers, ...(checkpointName && { checkpointName }) }, computers)
  }

  return {
    state: {
      snapshotId,
      availability: "available",
      requiredSpaceGB: previewMode === "restore-storage" ? 24 : 32,
      availableSpaceGB: previewMode === "space-blocked" ? 19 : previewMode === "restore-storage" ? 15 : 86,
      unsupportedStorage: previewMode === "unsupported-storage" ? { computer: "dev", label: "Client files" } : undefined,
      archives,
      ...(resultUnseen && !running && result && { resultUnseen: true }),
      operation: running ? { kind: "running", operation: running.operation, archive: running.archive, runningNames: running.runningNames, ...(running.targetName && { targetName: running.targetName }), progress: 18 + running.step * 25, phases: phasesFor(running.operation, running.step) } : result,
    },
    actions: {
      async checkLogins() {
        return previewMode === "logins-found"
          ? { logins: ["a Claude Code login (/root/.claude/.credentials.json)", "a GitHub CLI login (/root/.config/gh/hosts.yml)"], leavesOut: true }
          : { logins: [] }
      },
      async chooseDestination() { return source.backup.destination },
      async chooseArchive() {
        const archive = { ...initialBackupArchive(source), name: "dev.silo-backup", archivePath: "/selected/dev.silo-backup", destination: "Selected file", completedLabel: "Selected export file", size: "12.4 GiB" }
        return previewMode === "invalid-archive" ? { archive, valid: false, reason: "The checksum does not match, or this export file format is newer than this Silo version." } : { archive, valid: true }
      },
      async inspectArchive(selection) {
        const archive = selection
        return previewMode === "invalid-archive" ? { archive, valid: false, reason: "The checksum does not match, or this export file format is newer than this Silo version." } : { archive, valid: true }
      },
      startBackup,
      exportAndVerify(destination, computers, checkpointId) {
        if (running) return Promise.reject(new ExportIncompleteError("busy", "Another export or import is running."))
        startBackup(destination, computers, checkpointId)
        return new Promise<VerifiedExport>((resolve, reject) => { awaitedExport.current = { operationId: `fixture-export-${Date.now()}`, resolve, reject } })
      },
      startRestore: (archive, newName) => start("restore", archive, archive.computers, newName),
      cancelOperation() {
        if (!running) return
        if (running.operation === "backup" && awaitedExport.current) {
          awaitedExport.current.reject(new ExportIncompleteError("cancelled", "The operation was cancelled.", awaitedExport.current.operationId))
          awaitedExport.current = null
        }
        setResult({ operation: running.operation, archive: running.archive, runningNames: running.runningNames, ...(running.targetName && { targetName: running.targetName }), kind: "result", outcome: "cancelled", title: running.operation === "backup" ? "Export cancelled" : "Import cancelled", message: running.operation === "backup" ? "The export was cancelled." : "The import was cancelled.", detail: running.operation === "backup" ? "No export file was saved." : "No computer was added. The export file was not changed." })
        setResultUnseen(false)
        setRunning(null)
      },
      dismissOperation: () => { setResult(null); setResultUnseen(false) },
      async revealArchive() { /* Fixtures have no file manager to reveal; the toast action is exercised in tests. */ },
    },
  }
}

export function useUnavailableBackup(source: ApplicationSource): BackupController {
  return {
    state: { snapshotId: JSON.stringify(source.backup), availability: "unavailable", availabilityMessage: "Export and import are not available in this Silo build. No computer data was changed.", requiredSpaceGB: 0, archives: [], operation: null },
    actions: {
      checkLogins: async () => ({ logins: [] }),
      chooseDestination: async () => null,
      chooseArchive: async () => null,
      inspectArchive: async (selection) => ({ archive: selection, valid: false, reason: "Import validation is unavailable in this Silo build." }),
      startBackup: () => undefined,
      exportAndVerify: async () => { throw new ExportIncompleteError("rejected", "Export is not available in this Silo build.") },
      startRestore: () => undefined,
      cancelOperation: () => undefined,
      dismissOperation: () => undefined,
      revealArchive: async () => undefined,
    },
  }
}
