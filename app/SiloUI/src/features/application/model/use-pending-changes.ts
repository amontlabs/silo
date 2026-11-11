import { useCallback, useEffect, useMemo, useRef, useState } from "react"

import { errorMessage } from "@/lib/error-message"
import type { Approval, RequestEntry, RequestList } from "@/desktop/production-schemas"
import type { ApplicationActions } from "./application-source"

/** The normal polling interval, and the ceiling a failing read backs off to. */
export const PENDING_CHANGES_INTERVAL_MS = 5_000
export const PENDING_CHANGES_MAX_INTERVAL_MS = 60_000
/** Polling while the Pending page is not open: events do the real work, this only catches what they miss. */
export const PENDING_BACKGROUND_INTERVAL_MS = 30_000

/** The wait before the next read: the base interval, doubled after each failed read up to the ceiling. */
export function pendingChangesDelay(failures: number, base = PENDING_CHANGES_INTERVAL_MS): number {
  return Math.min(base * 2 ** failures, PENDING_CHANGES_MAX_INTERVAL_MS)
}

/** How a request's status reads in the UI. */
export const statusLabels: Record<RequestEntry["request"]["status"], string> = {
  open: "Open", keeping: "Merging…", kept: "Merged", "thrown-away": "Fork deleted",
}

/** The engine's change buckets and the names the UI shows for them. */
const bucketLabels: Record<string, string> = { code: "Code", packages: "Packages", settings: "Settings & files", database: "Database", other: "Other" }
export const bucketLabel = (bucket: string) => bucketLabels[bucket] ?? bucket

/** "2 code · 1 packages", in the order the bucket counts arrive; "No changes" when every bucket is empty. */
export function bucketSummary(counts: Record<string, number>): string {
  const parts = Object.entries(counts).filter(([, count]) => count > 0).map(([bucket, count]) => `${count} ${bucketLabel(bucket).toLowerCase()}`)
  return parts.length ? parts.join(" · ") : "No changes"
}

/** One thing waiting for the person: an approval, with its review request when it is a merge, or a review request alone. */
export type PendingRow =
  | { kind: "approval"; key: string; approval: Approval; request?: RequestEntry; createdAt: number }
  | { kind: "request"; key: string; entry: RequestEntry; createdAt: number }

/** How many answered approvals the Pending page keeps in view. */
export const ANSWERED_LIMIT = 10

const time = (iso: string | undefined) => {
  const parsed = iso ? Date.parse(iso) : Number.NaN
  return Number.isNaN(parsed) ? 0 : parsed
}

/**
 * Whether a review request is the one an approval wraps. The request records its approval; one
 * made before that was recorded is found by its id, on this device only. The engine tags this
 * device's own requests with the device's id, so the tag cannot tell them apart.
 */
const wrapsRequest = (approval: Approval, entry: RequestEntry) =>
  entry.request.approvalId
    ? entry.request.approvalId === approval.id
    : entry.request.id === approval.reviewRequestId && (!entry.deviceId || entry.deviceId === "local")

/**
 * Everything waiting for the person, newest first, and the approvals answered lately. A merge
 * approval and the review request it wraps are one row; a request whose approval was answered
 * (denied, say) is a row of its own again, because it still waits for a decision.
 */
export function pendingRows(approvals: readonly Approval[], requests: readonly RequestEntry[]): { waiting: PendingRow[]; answered: Approval[] } {
  const waitingApprovals = approvals.filter(({ state }) => state === "pending")
  const wrapped = new Set<RequestEntry>()
  const rows: PendingRow[] = waitingApprovals.map((approval) => {
    const request = approval.reviewRequestId
      ? requests.find((entry) => wrapsRequest(approval, entry))
      : undefined
    if (request) wrapped.add(request)
    return { kind: "approval", key: `approval:${approval.id}`, approval, request, createdAt: time(approval.createdAt) }
  })
  for (const entry of requests) {
    if (wrapped.has(entry)) continue
    rows.push({ kind: "request", key: `request:${entry.deviceId ?? ""}:${entry.request.id}`, entry, createdAt: time(entry.request.createdAt) })
  }
  rows.sort((a, b) => b.createdAt - a.createdAt)
  const answered = approvals
    .filter(({ state }) => state !== "pending")
    .sort((a, b) => time(b.decidedAt ?? b.createdAt) - time(a.decidedAt ?? a.createdAt))
    .slice(0, ANSWERED_LIMIT)
  return { waiting: rows, answered }
}

export interface PendingState {
  waiting: PendingRow[]
  /** Approvals answered lately, newest first, so their outcome stays in view. */
  answered: Approval[]
  /** Review requests that could not be read from a connected device. */
  unreachable: RequestList["unreachable"]
  /** How many things wait for the person. */
  count: number
  /** The message of the latest failed read; cleared by the next success. */
  error: string | null
  /** False until the first read ends. */
  loaded: boolean
  loading: boolean
  refresh: () => void
}

type PendingSources = Pick<ApplicationActions, "listApprovals" | "listReviewRequests" | "watchApprovals">

/**
 * Reads the approvals and the review requests of every device. Approval events refresh at once;
 * polling is the fallback, quicker while the page is open, backing off while reads fail.
 */
export function usePending({ listApprovals, listReviewRequests, watchApprovals }: PendingSources, active: boolean): PendingState {
  const [approvals, setApprovals] = useState<Approval[] | null>(null)
  const [requests, setRequests] = useState<RequestList | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(Boolean(listApprovals || listReviewRequests))
  const reload = useRef<() => void>(() => {})

  useEffect(() => {
    if (!listApprovals && !listReviewRequests) return
    let disposed = false
    let failures = 0
    let timer: number | undefined
    let reading = false
    let again = false
    const read = async () => {
      if (reading) { again = true; return }
      reading = true
      window.clearTimeout(timer)
      try {
        const [approvalResult, requestResult] = await Promise.allSettled([
          listApprovals?.() ?? Promise.resolve(null),
          listReviewRequests?.({ status: "open", allDevices: true }) ?? Promise.resolve(null),
        ])
        if (disposed) return
        if (approvalResult.status === "fulfilled" && approvalResult.value) setApprovals(approvalResult.value.approvals)
        if (requestResult.status === "fulfilled" && requestResult.value) setRequests(requestResult.value)
        const failed = [approvalResult, requestResult].find((result): result is PromiseRejectedResult => result.status === "rejected")
        if (failed) {
          failures += 1
          setError(errorMessage(failed.reason, { fallback: "Silo could not read what is waiting for you. Retry; if it fails again, relaunch Silo." }))
        } else {
          failures = 0
          setError(null)
        }
      } finally {
        reading = false
        if (!disposed) {
          setLoading(false)
          if (again) { again = false; void read() }
          else timer = window.setTimeout(() => void read(), pendingChangesDelay(failures, active ? PENDING_CHANGES_INTERVAL_MS : PENDING_BACKGROUND_INTERVAL_MS))
        }
      }
    }
    reload.current = () => void read()
    void read()
    const stopWatching = watchApprovals?.(() => void read())
    return () => { disposed = true; window.clearTimeout(timer); stopWatching?.(); reload.current = () => {} }
  }, [active, listApprovals, listReviewRequests, watchApprovals])

  const refresh = useCallback(() => reload.current(), [])
  const rows = useMemo(() => pendingRows(approvals ?? [], requests?.requests ?? []), [approvals, requests])
  return {
    waiting: rows.waiting, answered: rows.answered, unreachable: requests?.unreachable ?? [], count: rows.waiting.length,
    error, loaded: approvals !== null || requests !== null, loading, refresh,
  }
}
