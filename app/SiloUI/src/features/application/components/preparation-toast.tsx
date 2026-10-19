import { useEffect, useRef } from "react"

import { usePreparationStatus } from "@/desktop/preparation"
import { dismissOperationToast, showOperationFailure, showOperationProgress } from "@/lib/operation-toast"

const TOAST_ID = "preparation"
/** Work that finishes within this time never flashes a notification. */
export const PREPARATION_SHOW_DELAY_MS = 400
/** The toast stays through the pause between two items (one finishes, the next starts). */
export const PREPARATION_HIDE_DELAY_MS = 1200

/**
 * One non-blocking notification for what this device prepares in the background at launch
 * (the computer image, the computer use tools, ChatGPT for Linux): the current item while it works, a short message
 * with Retry when something fails, and nothing once everything is ready. Renders nothing itself.
 */
export function PreparationToast() {
  const { items, retry } = usePreparationStatus()
  const retryRef = useRef(retry)
  retryRef.current = retry
  const shown = useRef(false)
  const startedAt = useRef<number | null>(null)
  const running = items.filter(item => item.state === "running")
  const failed = items.filter(item => item.state === "failed")
  const key = JSON.stringify(items)

  useEffect(() => {
    const hide = () => {
      if (!shown.current) return
      shown.current = false
      startedAt.current = null
      dismissOperationToast(TOAST_ID)
    }
    if (running.length > 0) {
      const [current] = running
      const show = () => {
        shown.current = true
        startedAt.current ??= Date.now()
        showOperationProgress(TOAST_ID, {
          title: "Preparing Silo",
          step: current.text,
          progress: current.progress,
          startedAt: startedAt.current,
          steps: running.length > 1 ? running.map((item, index) => ({ label: item.text, state: index === 0 ? "current" : "pending" })) : undefined,
        })
      }
      if (shown.current) { show(); return }
      const timer = window.setTimeout(show, PREPARATION_SHOW_DELAY_MS)
      return () => window.clearTimeout(timer)
    }
    if (failed.length > 0) {
      shown.current = true
      startedAt.current = null
      showOperationFailure(TOAST_ID, "Silo could not finish preparing", {
        description: [...new Set(failed.map(item => item.text))].join(" "),
        retry: failed.some(item => item.retryable) ? () => retryRef.current() : undefined,
        native: false,
      })
      return
    }
    const timer = window.setTimeout(hide, PREPARATION_HIDE_DELAY_MS)
    return () => window.clearTimeout(timer)
    // oxlint-disable-next-line react-hooks/exhaustive-deps
  }, [key])

  useEffect(() => () => { if (shown.current) dismissOperationToast(TOAST_ID) }, [])
  return null
}
