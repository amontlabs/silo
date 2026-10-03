import { useCallback, useEffect, useRef, useState } from "react"
import { invoke } from "@tauri-apps/api/core"
import { listen } from "@tauri-apps/api/event"
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow"
import { CircleAlert, Upload } from "lucide-react"

import { Button } from "@/components/ui/button"
import { Progress } from "@/components/ui/progress"
import { DOWNLOADS_FOLDER, summarizeNames, transferProgressEvent, transferProgressSchema, uploadOutcomeSchema, uploadSelectionSchema, viewerDragEvent, viewerDropEvent, type UploadSelection } from "@/features/application/model/file-transfer"

type DropState =
  | { kind: "idle" }
  | { kind: "uploading"; id: string; name: string; done: number; total: number }
  | { kind: "done"; names: string[] }
  | { kind: "failed"; message: string }

const BUSY = "Another file transfer is still running."
let sequence = 0

/**
 * Files dropped on the viewer window go to the computer's Downloads folder. The native side
 * receives the drop, over the toolbar or over the computer's display, and tells this window
 * only a one-time token for the files and their names; the computer's page never sees them.
 */
export function useViewerFileDrop(computer: string) {
  const [state, setState] = useState<DropState>({ kind: "idle" })
  const [rejection, setRejection] = useState<string | null>(null)
  const [hovering, setHovering] = useState(false)
  const running = useRef<string | null>(null)
  const clear = useRef<number | undefined>(undefined)
  const clearRejection = useRef<number | undefined>(undefined)

  const upload = useCallback(async (selection: UploadSelection) => {
    if (selection.names.length === 0) return
    if (running.current) {
      setRejection(BUSY)
      window.clearTimeout(clearRejection.current)
      clearRejection.current = window.setTimeout(() => setRejection(null), 6000)
      return
    }
    window.clearTimeout(clear.current)
    setRejection(null)
    const id = `viewer-drop-${Date.now().toString(36)}-${++sequence}`
    running.current = id
    setState({ kind: "uploading", id, name: selection.names.length === 1 ? selection.names[0] : `${selection.names.length} files`, done: 0, total: 0 })
    let stop: (() => void) | undefined
    try {
      stop = await listen(transferProgressEvent, event => {
        const progress = transferProgressSchema.safeParse(event.payload)
        if (!progress.success || progress.data.id !== id || progress.data.state !== "transferring") return
        const { name, fileCount, fileIndex, bytesDone, bytesTotal } = progress.data
        setState({ kind: "uploading", id, name: fileCount > 1 ? `${name} · ${fileIndex + 1} of ${fileCount}` : name, done: bytesDone, total: bytesTotal })
      })
      const outcome = uploadOutcomeSchema.parse(await invoke("upload_files", { transferId: id, computer, directory: DOWNLOADS_FOLDER, selection: selection.token, conflict: "keepBoth" }))
      if (outcome.status === "done") {
        setState({ kind: "done", names: outcome.names })
        clear.current = window.setTimeout(() => setState({ kind: "idle" }), 6000)
      } else setState({ kind: "idle" })
    } catch (cause) {
      setState({ kind: "failed", message: typeof cause === "string" ? cause : cause instanceof Error ? cause.message : "The upload failed." })
    } finally {
      stop?.()
      running.current = null
    }
  }, [computer])

  useEffect(() => {
    let disposed = false
    const stops: Array<() => void> = []
    // Events are listened for on this window only: a listener on every target
    // would also receive the drops aimed at another open viewer.
    const subscribe = (event: string, handler: (payload: unknown) => void) => {
      try {
        void getCurrentWebviewWindow().listen(event, message => handler(message.payload))
          .then(stop => { if (disposed) stop(); else stops.push(stop) })
          .catch(() => {})
      } catch { /* A window without native events offers no drop target. */ }
    }
    subscribe(viewerDragEvent, payload => setHovering(payload === true))
    subscribe(viewerDropEvent, payload => {
      setHovering(false)
      const selection = uploadSelectionSchema.safeParse(payload)
      if (selection.success) void upload(selection.data)
    })
    return () => {
      disposed = true
      stops.forEach(stop => stop())
      window.clearTimeout(clear.current)
      window.clearTimeout(clearRejection.current)
    }
  }, [upload])

  const cancel = useCallback(() => {
    if (running.current) void invoke("cancel_transfer", { transferId: running.current }).catch(() => {})
  }, [])
  return { state, rejection, hovering, cancel, dismiss: () => setState({ kind: "idle" }), dismissRejection: () => setRejection(null) }
}

/** Upload status for the viewer toolbar. */
export function ViewerTransferStatus({ drop }: { drop: ReturnType<typeof useViewerFileDrop> }) {
  const { state, rejection, hovering, cancel, dismiss, dismissRejection } = drop
  const notice = rejection && <div role="alert" className="flex min-w-0 items-center gap-1 text-xs text-destructive">
    <CircleAlert aria-hidden="true" className="size-3.5 shrink-0" /><span className="truncate" title={rejection}>{rejection}</span>
    <Button size="xs" variant="ghost" onClick={dismissRejection}>Dismiss</Button>
  </div>
  if (state.kind === "uploading") {
    return <>
      <div role="status" className="flex min-w-0 items-center gap-2 text-xs">
        <Upload aria-hidden="true" className="size-3.5 shrink-0" />
        <span className="truncate" title={state.name}>Uploading {state.name}</span>
        <Progress className="w-20 shrink-0" aria-label="Upload progress" value={state.total > 0 ? state.done / state.total * 100 : null} />
        <Button size="xs" variant="ghost" onClick={cancel}>Cancel</Button>
      </div>
      {notice}
    </>
  }
  if (state.kind === "failed") {
    return <>
      <div role="alert" className="flex min-w-0 items-center gap-1 text-xs text-destructive">
        <CircleAlert aria-hidden="true" className="size-3.5 shrink-0" /><span className="truncate" title={state.message}>{state.message}</span>
        <Button size="xs" variant="ghost" onClick={dismiss}>Dismiss</Button>
      </div>
      {notice}
    </>
  }
  if (state.kind === "done") return <><span role="status" className="truncate text-xs text-muted-foreground">Uploaded {summarizeNames(state.names)} to Downloads</span>{notice}</>
  if (hovering) return <span role="status" className="flex items-center gap-1 text-xs"><Upload aria-hidden="true" className="size-3.5" />Drop to upload to Downloads</span>
  return notice || null
}
