import { useCallback, useEffect, useRef, useState } from "react"
import { invoke } from "@tauri-apps/api/core"
import { listen } from "@tauri-apps/api/event"
import { getCurrentWindow } from "@tauri-apps/api/window"
import { CircleAlert, Upload } from "lucide-react"

import { Button } from "@/components/ui/button"
import { Progress } from "@/components/ui/progress"
import { DOWNLOADS_FOLDER, baseName, summarizeNames, transferProgressEvent, transferProgressSchema, uploadOutcomeSchema } from "@/features/application/model/file-transfer"

type DropState =
  | { kind: "idle" }
  | { kind: "uploading"; id: string; name: string; done: number; total: number }
  | { kind: "done"; names: string[] }
  | { kind: "failed"; message: string }

let sequence = 0

/**
 * Files dropped on the viewer window go to the computer's Downloads folder. The shell window
 * receives the drop with the files' paths on this device; the computer's page never sees them.
 */
export function useViewerFileDrop(computer: string) {
  const [state, setState] = useState<DropState>({ kind: "idle" })
  const [hovering, setHovering] = useState(false)
  const running = useRef<string | null>(null)
  const clear = useRef<number | undefined>(undefined)

  const upload = useCallback(async (paths: string[]) => {
    if (paths.length === 0) return
    window.clearTimeout(clear.current)
    if (running.current) {
      setState({ kind: "failed", message: "Another file transfer is still running." })
      return
    }
    const id = `viewer-drop-${Date.now().toString(36)}-${++sequence}`
    running.current = id
    setState({ kind: "uploading", id, name: paths.length === 1 ? baseName(paths[0]) : `${paths.length} files`, done: 0, total: 0 })
    let stop: (() => void) | undefined
    try {
      stop = await listen(transferProgressEvent, event => {
        const progress = transferProgressSchema.safeParse(event.payload)
        if (!progress.success || progress.data.id !== id || progress.data.state !== "transferring") return
        const { name, fileCount, fileIndex, bytesDone, bytesTotal } = progress.data
        setState({ kind: "uploading", id, name: fileCount > 1 ? `${name} · ${fileIndex + 1} of ${fileCount}` : name, done: bytesDone, total: bytesTotal })
      })
      const outcome = uploadOutcomeSchema.parse(await invoke("upload_files", { transferId: id, computer, directory: DOWNLOADS_FOLDER, paths, conflict: "keepBoth" }))
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
    let unlisten: (() => void) | undefined
    try {
      void getCurrentWindow().onDragDropEvent(event => {
        const payload = event.payload
        if (payload.type === "enter" || payload.type === "over") setHovering(true)
        else if (payload.type === "leave") setHovering(false)
        else if (payload.type === "drop") {
          setHovering(false)
          void upload(payload.paths)
        }
      }).then(stop => { if (disposed) stop(); else unlisten = stop }).catch(() => {})
    } catch { /* A window without drag-and-drop events offers no drop target. */ }
    return () => {
      disposed = true
      unlisten?.()
      window.clearTimeout(clear.current)
    }
  }, [upload])

  const cancel = useCallback(() => {
    if (running.current) void invoke("cancel_transfer", { transferId: running.current }).catch(() => {})
  }, [])
  return { state, hovering, cancel, dismiss: () => setState({ kind: "idle" }) }
}

/** Upload status for the viewer toolbar. */
export function ViewerTransferStatus({ drop }: { drop: ReturnType<typeof useViewerFileDrop> }) {
  const { state, hovering, cancel, dismiss } = drop
  if (state.kind === "uploading") {
    return <div role="status" className="flex min-w-0 items-center gap-2 text-xs">
      <Upload aria-hidden="true" className="size-3.5 shrink-0" />
      <span className="truncate" title={state.name}>Uploading {state.name}</span>
      <Progress className="w-20 shrink-0" aria-label="Upload progress" value={state.total > 0 ? state.done / state.total * 100 : null} />
      <Button size="xs" variant="ghost" onClick={cancel}>Cancel</Button>
    </div>
  }
  if (state.kind === "failed") {
    return <div role="alert" className="flex min-w-0 items-center gap-1 text-xs text-destructive">
      <CircleAlert aria-hidden="true" className="size-3.5 shrink-0" /><span className="truncate" title={state.message}>{state.message}</span>
      <Button size="xs" variant="ghost" onClick={dismiss}>Dismiss</Button>
    </div>
  }
  if (state.kind === "done") return <span role="status" className="truncate text-xs text-muted-foreground">Uploaded {summarizeNames(state.names)} to Downloads</span>
  if (hovering) return <span role="status" className="flex items-center gap-1 text-xs"><Upload aria-hidden="true" className="size-3.5" />Drop to upload to Downloads</span>
  return null
}
