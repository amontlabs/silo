import type { ApplicationComputer } from "@/features/application/model/application-source"
import { computerTarget } from "@/features/application/model/connections"
import { baseName, type FileTransferActions, type TransferProgress } from "@/features/application/model/file-transfer"

/** Host files offered by the fixture upload picker; previews and tests only. */
export const fixtureUploadPaths = ["/Users/ada/Documents/report.pdf", "/Users/ada/Documents/notes.txt"]

/**
 * A deterministic transfer boundary. Uploading a name that a computer's top-level workspace
 * already lists asks for a conflict policy first; every transfer then completes in two steps.
 */
export function fixtureFileTransfers(computers: ApplicationComputer[]): FileTransferActions {
  const listeners = new Set<(progress: TransferProgress) => void>()
  const emit = (progress: TransferProgress) => listeners.forEach(listener => listener(progress))
  return {
    chooseUploadFiles: async () => [...fixtureUploadPaths],
    upload: async ({ id, computer, directory, paths, conflict }) => {
      const owner = computers.find(item => computerTarget(item) === computer)
      if (!owner || owner.state !== "running") throw "Start this computer to transfer files."
      const names = paths.map(baseName)
      const existing = directory === "/workspace" ? names.filter(name => owner.files.some(file => file.name === name)) : []
      if (existing.length > 0 && conflict === "ask") return { status: "conflict", names: existing }
      const stored = names.map(name => existing.includes(name) && conflict === "keepBoth" ? name.replace(/(\.[^.]+)?$/, " (1)$1") : name)
      for (const done of [0, 500]) emit({ id, computer, direction: "upload", state: "transferring", name: stored[0] ?? "", fileIndex: 0, fileCount: stored.length, bytesDone: done, bytesTotal: 1000 })
      return { status: "done", names: stored }
    },
    download: async ({ id, computer, path }) => {
      if (!computers.some(item => computerTarget(item) === computer && item.state === "running")) throw "Start this computer to transfer files."
      emit({ id, computer, direction: "download", state: "transferring", name: baseName(path), fileIndex: 0, fileCount: 1, bytesDone: 500, bytesTotal: 1000 })
      return { status: "done", path: `/Users/ada/Downloads/${baseName(path)}` }
    },
    cancel: async () => {},
    onProgress: async handler => {
      listeners.add(handler)
      return () => { listeners.delete(handler) }
    },
  }
}
