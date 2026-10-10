import { z } from "zod"

/** What to do when an uploaded file's name already exists in the folder. */
export type ConflictPolicy = "ask" | "replace" | "keepBoth"

export const uploadOutcomeSchema = z.discriminatedUnion("status", [
  z.object({ status: z.literal("conflict"), names: z.array(z.string()) }),
  z.object({ status: z.literal("done"), names: z.array(z.string()) }),
  z.object({ status: z.literal("cancelled") }),
])
export type UploadOutcome = z.infer<typeof uploadOutcomeSchema>

export const downloadOutcomeSchema = z.discriminatedUnion("status", [
  z.object({ status: z.literal("done"), path: z.string() }),
  z.object({ status: z.literal("cancelled") }),
])
export type DownloadOutcome = z.infer<typeof downloadOutcomeSchema>

export const transferProgressSchema = z.object({
  id: z.string(),
  computer: z.string(),
  direction: z.enum(["upload", "download"]),
  state: z.enum(["transferring", "done", "failed", "cancelled"]),
  name: z.string(),
  fileIndex: z.number().int().min(0),
  fileCount: z.number().int().min(0),
  bytesDone: z.number().min(0),
  bytesTotal: z.number().min(0),
})
export type TransferProgress = z.infer<typeof transferProgressSchema>

export const transferProgressEvent = "silo://transfer-progress"
export const viewerDragEvent = "silo://viewer-drag"
export const viewerDropEvent = "silo://viewer-drop"

/** Files the user picked or dropped; the native side keeps their paths and hands out this one-time token. */
export const uploadSelectionSchema = z.object({ token: z.string().min(1), names: z.array(z.string()) })
export type UploadSelection = z.infer<typeof uploadSelectionSchema>

export interface UploadRequest {
  /** One transfer at a time; the same id cancels it and labels its progress. */
  id: string
  computer: string
  directory: string
  /** The token of a selection the user made in the native picker or by dropping files. */
  selection: string
  conflict: ConflictPolicy
}

/** The native file transfer boundary. Paths on this device never reach the page: the user's picks stay native. */
export interface FileTransferActions {
  /** `null` when the picker is dismissed. */
  chooseUploadFiles: () => Promise<UploadSelection | null>
  upload: (request: UploadRequest) => Promise<UploadOutcome>
  download: (request: { id: string; computer: string; path: string }) => Promise<DownloadOutcome>
  cancel: (id: string) => Promise<void>
  onProgress: (handler: (progress: TransferProgress) => void) => Promise<() => void>
}

/** The Downloads folder of the account computers work as. */
export const DOWNLOADS_FOLDER = "/home/silo/Downloads"

export function baseName(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).at(-1) ?? path
}

/** "report.txt", "report.txt and 2 more" */
export function summarizeNames(names: string[]): string {
  if (names.length === 0) return "files"
  return names.length === 1 ? `“${names[0]}”` : `“${names[0]}” and ${names.length - 1} more`
}
