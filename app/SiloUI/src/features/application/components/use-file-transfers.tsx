import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from "react"
import { AlertDialog } from "radix-ui"

import { Button } from "@/components/ui/button"
import { restoreFocus } from "@/lib/focus"
import { dismissOperationToast, errorMessage, showOperationFailure, showOperationProgress, showOperationSuccess } from "@/lib/operation-toast"
import { baseName, formatBytes, summarizeNames, type ConflictPolicy, type FileTransferActions, type TransferProgress } from "@/features/application/model/file-transfer"

export interface FileTransferControls {
  /** True while a transfer runs: the computer's side allows one at a time. */
  busy: boolean
  /** Choose files on this device and upload them into `directory`. `label` names the computer in notices. */
  upload: (computer: string, directory: string, label: string) => void
  download: (computer: string, path: string, label: string) => void
}

type Choice = ConflictPolicy | null
interface PendingConflict {
  names: string[]
  answer: (choice: Choice) => void
}

let sequence = 0
const nextId = () => `file-transfer-${Date.now().toString(36)}-${++sequence}`

/**
 * Upload and download from the Files page: native file picker and save dialog, progress and
 * cancel in one notification, and a dialog when an uploaded name already exists. Render
 * `dialog` once, in a component that stays mounted while the user navigates (see
 * `FileTransfersProvider`).
 */
export function useFileTransfers(api: FileTransferActions | undefined): { controls: FileTransferControls | undefined; dialog: ReactNode } {
  const [busy, setBusy] = useState(false)
  const [conflict, setConflict] = useState<PendingConflict | null>(null)
  const active = useRef<{ id: string; label: string; verb: string } | null>(null)
  const running = useRef(false)
  const mounted = useRef(true)

  useEffect(() => {
    mounted.current = true
    return () => { mounted.current = false }
  }, [])

  useEffect(() => {
    if (!api) return
    let disposed = false
    let stop: (() => void) | undefined
    const onProgress = (progress: TransferProgress) => {
      const current = active.current
      if (!current || progress.id !== current.id || progress.state !== "transferring") return
      const known = progress.bytesTotal > 0
      showOperationProgress(current.id, {
        title: `${current.verb} ${current.label}`,
        step: [progress.name && `“${progress.name}”`, progress.fileCount > 1 && `${progress.fileIndex + 1} of ${progress.fileCount}`, known && `${formatBytes(progress.bytesDone)} of ${formatBytes(progress.bytesTotal)}`].filter(Boolean).join(" · "),
        progress: known ? progress.bytesDone / progress.bytesTotal : null,
        cancel: { onCancel: () => void api.cancel(current.id).catch(() => {}) },
      })
    }
    api.onProgress(onProgress).then(unlisten => { if (disposed) unlisten(); else stop = unlisten }).catch(() => {})
    return () => { disposed = true; stop?.() }
  }, [api])

  const askConflict = useCallback((names: string[]) => new Promise<Choice>(resolve => {
    setConflict({ names, answer: choice => { setConflict(null); resolve(choice) } })
  }), [])
  const pending = useRef<PendingConflict | null>(null)
  useEffect(() => { pending.current = conflict }, [conflict])
  useEffect(() => () => pending.current?.answer(null), [])

  const run = useCallback(async (verb: string, label: string, work: (id: string) => Promise<void>) => {
    if (!api || running.current) return
    running.current = true
    setBusy(true)
    const id = nextId()
    active.current = { id, label, verb }
    try {
      await work(id)
    } catch (error) {
      showOperationFailure(id, `${verb === "Uploading" ? "Upload" : "Download"} failed`, { description: errorMessage(error), native: false })
    } finally {
      active.current = null
      running.current = false
      if (mounted.current) setBusy(false)
    }
  }, [api])

  const controls = useMemo<FileTransferControls | undefined>(() => api && {
    busy,
    upload: (computer, directory, label) => void run("Uploading", label, async id => {
      const picked = await api.chooseUploadFiles()
      if (!picked || picked.names.length === 0) return
      const { names } = picked
      const start = (policy: ConflictPolicy) => {
        showOperationProgress(id, {
          title: `Uploading ${summarizeNames(names)} to ${label}`,
          progress: null,
          cancel: { onCancel: () => void api.cancel(id).catch(() => {}) },
          computer,
        })
        return api.upload({ id, computer, directory, selection: picked.token, conflict: policy })
      }
      let outcome = await start("ask")
      if (outcome.status === "conflict") {
        dismissOperationToast(id)
        const choice = await askConflict(outcome.names)
        if (!choice) return
        outcome = await start(choice)
      }
      if (outcome.status === "done") showOperationSuccess(id, `Uploaded ${summarizeNames(outcome.names)} to ${label}`, { description: directory, native: false })
      else dismissOperationToast(id)
    }),
    download: (computer, path, label) => void run("Downloading", label, async id => {
      showOperationProgress(id, {
        title: `Downloading “${baseName(path)}” from ${label}`,
        progress: null,
        cancel: { onCancel: () => void api.cancel(id).catch(() => {}) },
        computer,
      })
      const outcome = await api.download({ id, computer, path })
      if (outcome.status === "done") showOperationSuccess(id, `Downloaded “${baseName(path)}”`, { description: outcome.path, native: false })
      else dismissOperationToast(id)
    }),
  }, [api, busy, run, askConflict])

  return { controls, dialog: <ConflictDialog pending={conflict} /> }
}

const FileTransfersContext = createContext<FileTransferControls | undefined>(undefined)

/**
 * Owns file transfers for the whole application, so progress, cancel and the replace-or-keep
 * question survive moving between pages.
 */
export function FileTransfersProvider({ api, children }: { api: FileTransferActions | undefined; children: ReactNode }) {
  const { controls, dialog } = useFileTransfers(api)
  return <FileTransfersContext.Provider value={controls}>
    {children}
    {dialog}
  </FileTransfersContext.Provider>
}

/** The upload and download controls, or `undefined` where the application cannot transfer files. */
export function useFileTransferControls(): FileTransferControls | undefined {
  return useContext(FileTransfersContext)
}

function ConflictDialog({ pending }: { pending: PendingConflict | null }) {
  const keep = useRef<HTMLButtonElement>(null)
  const previousFocus = useRef<HTMLElement | null>(null)
  const names = pending?.names ?? []
  const subject = names.length === 1 ? `“${names[0]}” already exists` : `${names.length} files already exist`
  return <AlertDialog.Root open={pending !== null} onOpenChange={open => { if (!open) pending?.answer(null) }}>
    <AlertDialog.Portal>
      <AlertDialog.Overlay className="fixed inset-0 z-50 bg-black/20" />
      <AlertDialog.Content onOpenAutoFocus={event => {
        previousFocus.current = document.activeElement instanceof HTMLElement ? document.activeElement : null
        event.preventDefault()
        keep.current?.focus()
      }} onCloseAutoFocus={event => {
        event.preventDefault()
        restoreFocus(previousFocus.current)
      }} className="fixed top-1/2 left-1/2 z-50 grid w-[calc(100%-2rem)] max-w-sm -translate-x-1/2 -translate-y-1/2 gap-2 rounded-xl border border-border bg-popover p-4 text-xs text-popover-foreground shadow-2xl outline-none">
        <AlertDialog.Title className="text-[13px] font-medium">{subject} in this folder</AlertDialog.Title>
        <AlertDialog.Description className="text-muted-foreground">
          {names.length > 1 && <span className="mb-1 block max-h-24 overflow-y-auto break-all">{names.map(name => `“${name}”`).join(", ")}</span>}
          Replace {names.length === 1 ? "it" : "them"} with the files you chose, or keep both and add a number to the new {names.length === 1 ? "name" : "names"}.
        </AlertDialog.Description>
        <div className="mt-1 flex justify-end gap-2">
          <AlertDialog.Cancel asChild><Button type="button" variant="ghost" size="sm">Cancel</Button></AlertDialog.Cancel>
          <AlertDialog.Action asChild><Button type="button" variant="outline" size="sm" onClick={() => pending?.answer("replace")}>Replace</Button></AlertDialog.Action>
          <AlertDialog.Action asChild><Button ref={keep} type="button" size="sm" onClick={() => pending?.answer("keepBoth")}>Keep both</Button></AlertDialog.Action>
        </div>
      </AlertDialog.Content>
    </AlertDialog.Portal>
  </AlertDialog.Root>
}
