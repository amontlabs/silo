import { invoke, isTauri } from "@tauri-apps/api/core"
import { listen } from "@tauri-apps/api/event"
import { z } from "zod"

/**
 * One notice, the shared shape of the backend notification router (`notifications.rs`).
 * `key` is the stable identity: a newer notice with the same key replaces the older one, as
 * a system notification and as an in-app toast.
 */
export const noticeSchema = z.object({
  category: z.enum(["failures", "changes", "completions", "approvals"]),
  key: z.string().min(1),
  title: z.string(),
  body: z.string(),
  computer: z.object({ id: z.string(), name: z.string() }).nullable(),
})

export type Notice = z.infer<typeof noticeSchema>
export type NoticeComputer = NonNullable<Notice["computer"]>

export const NOTICE_EVENT = "silo://notice"

/**
 * Mirror a result to the system. The backend decides whether it is shown (never while the
 * main window is focused, category preferences, OS authorization), so callers never check
 * focus. A no-op outside the desktop app. Never throws: a notification must not turn a
 * finished operation into a failure.
 */
export function deliverNotice(notice: Notice): void {
  if (!isTauri()) return
  invoke("deliver_notice", { notice }).catch((error: unknown) => console.error("Silo notice:", error))
}

/** Drop the system notifications of a computer (the backend also does this on deletion). */
export function clearComputerNotices(computerId: string): void {
  if (!isTauri()) return
  invoke("clear_computer_notices", { computerId }).catch((error: unknown) => console.error("Silo notice:", error))
}

/** Subscribe to backend-originated notices that need an in-app toast. Returns a disposer. */
export function listenForNotices(handler: (notice: Notice) => void): () => void {
  if (!isTauri()) return () => {}
  let stopped = false
  let stop: (() => void) | undefined
  let connecting = false
  const connect = () => {
    if (stopped || stop || connecting) return
    connecting = true
    void listen(NOTICE_EVENT, (event) => {
      if (stopped) return
      const parsed = noticeSchema.safeParse(event.payload)
      if (parsed.success) handler(parsed.data)
      else console.error("Silo notice: ignored malformed notice", parsed.error.message)
    }).then((unlisten) => { if (stopped) unlisten(); else stop = unlisten }, (error: unknown) => console.error("Silo notice:", error))
      .finally(() => { connecting = false })
  }
  window.addEventListener("focus", connect)
  connect()
  return () => {
    if (stopped) return
    stopped = true
    window.removeEventListener("focus", connect)
    stop?.()
  }
}
