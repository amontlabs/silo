import { useEffect, useRef } from "react"

import { listenForNotices, type Notice } from "@/desktop/notices"
import { showOperationFailure, showOperationSuccess } from "@/lib/operation-toast"

/** How long a backend notice stays on screen: the system notification already told the person. */
export const BACKEND_FAILURE_TOAST_MS = 20_000

/**
 * Show the in-app toast for a backend-originated notice (an unexpected computer change, a
 * startup or update failure, an agent's request). The backend already sent the system
 * notification, so nothing here mirrors it back (`native: false`). The notice key is the toast
 * id: a repeat replaces the earlier toast in place, and a tagged computer ID lets deleting that
 * computer clear it. A request from an agent gets a Review button that calls `onReview`.
 */
export function showBackendNotice(notice: Notice, onReview?: (notice: Notice) => void) {
  const common = { description: notice.body || undefined, noticeComputer: notice.computer ?? undefined, native: false }
  if (notice.category === "completions") showOperationSuccess(notice.key, notice.title, { ...common, persist: true })
  else if (notice.category === "approvals") showOperationFailure(notice.key, notice.title, { ...common, tone: "warning", action: onReview ? { label: "Review", onClick: () => onReview(notice) } : undefined })
  else showOperationFailure(notice.key, notice.title, { ...common, tone: notice.category === "changes" ? "warning" : "error", duration: BACKEND_FAILURE_TOAST_MS })
}

/** Subscribe to backend notices while mounted. A no-op outside the desktop app. */
export function useBackendNotices(onReview?: (notice: Notice) => void) {
  const review = useRef(onReview)
  useEffect(() => { review.current = onReview }, [onReview])
  useEffect(() => listenForNotices(notice => showBackendNotice(notice, review.current && (value => review.current?.(value)))), [])
}
