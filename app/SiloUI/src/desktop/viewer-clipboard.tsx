import { useCallback, useEffect, useRef, useState } from "react"
import { invoke } from "@tauri-apps/api/core"
import { listen } from "@tauri-apps/api/event"
import { CircleAlert, ClipboardCopy, ClipboardPaste } from "lucide-react"
import { Button } from "@/components/ui/button"
import { CLIPBOARD_EVENT, clipboardFeedback, type ClipboardAction, type ClipboardReport } from "./viewer-clipboard-feedback"

const FEEDBACK_MS = 4000

function isReport(value: unknown): value is ClipboardReport {
  return typeof value === "object" && value !== null && "status" in value && "action" in value
}

const isMac = () => typeof navigator !== "undefined" && /Mac/i.test(navigator.platform)

/** Paste and Copy buttons for the computer's clipboard, with brief status feedback. Shortcut-started
 * transfers report through the same status. The backend says when a desktop does not support the
 * clipboard. */
export function ViewerClipboard({ computer, name }: { computer: string; name: string }) {
  const [feedback, setFeedback] = useState<{ text: string; error: boolean } | null>(null)
  const [pending, setPending] = useState(false)
  const timer = useRef<number | undefined>(undefined)
  const show = useCallback((next: { text: string; error: boolean }) => {
    window.clearTimeout(timer.current)
    setFeedback(next)
    timer.current = window.setTimeout(() => setFeedback(null), FEEDBACK_MS)
  }, [])
  useEffect(() => () => window.clearTimeout(timer.current), [])
  useEffect(() => {
    let disposed = false
    let stop: (() => void) | undefined
    void listen<unknown>(CLIPBOARD_EVENT, event => {
      if (isReport(event.payload)) show(clipboardFeedback(event.payload, name))
    }).then(unlisten => { if (disposed) unlisten(); else stop = unlisten }).catch(() => {})
    return () => { disposed = true; stop?.() }
  }, [name, show])
  async function transfer(action: ClipboardAction) {
    setPending(true)
    try {
      const report = await invoke<unknown>("desktop_viewer_clipboard", { computer, action })
      show(isReport(report) ? clipboardFeedback(report, name) : { text: "The clipboard transfer failed", error: true })
    } catch (cause) { show({ text: String(cause), error: true }) }
    finally { setPending(false) }
  }
  const chord = isMac() ? { paste: "Command+V", copy: "Command+C" } : { paste: "Ctrl+Shift+V", copy: "Ctrl+Shift+C" }
  return <div className="flex min-w-0 items-center gap-1">
    {feedback && (feedback.error
      ? <div role="alert" className="flex min-w-0 items-center gap-1 text-xs text-destructive">
        <CircleAlert aria-hidden="true" className="size-3.5 shrink-0" /><span className="truncate" title={feedback.text}>{feedback.text}</span>
      </div>
      : <span role="status" className="truncate text-xs text-muted-foreground" title={feedback.text}>{feedback.text}</span>)}
    <Button variant="ghost" size="icon-xs" aria-label="Paste into computer" title={`Paste into computer (${chord.paste})`} disabled={pending} onClick={() => { void transfer("paste") }}><ClipboardPaste /></Button>
    <Button variant="ghost" size="icon-xs" aria-label="Copy from computer" title={`Copy from computer (${chord.copy})`} disabled={pending} onClick={() => { void transfer("copy") }}><ClipboardCopy /></Button>
  </div>
}
