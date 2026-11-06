import { useCallback, useEffect, useRef, useState } from "react"

import { errorMessage } from "@/lib/error-message"
import type { RequestEntry, RequestList } from "@/desktop/production-schemas"
import type { ApplicationActions } from "./application-source"

/** The normal polling interval, and the ceiling a failing read backs off to. */
export const PENDING_CHANGES_INTERVAL_MS = 5_000
export const PENDING_CHANGES_MAX_INTERVAL_MS = 60_000

/** The wait before the next read: the normal interval, doubled after each failed read up to the ceiling. */
export function pendingChangesDelay(failures: number): number {
  return Math.min(PENDING_CHANGES_INTERVAL_MS * 2 ** failures, PENDING_CHANGES_MAX_INTERVAL_MS)
}

/** How a request's status reads in the UI. */
export const statusLabels: Record<RequestEntry["request"]["status"], string> = {
  open: "Open", keeping: "Keeping…", kept: "Kept", "thrown-away": "Thrown away",
}

/** The engine's change buckets and the names the UI shows for them. */
const bucketLabels: Record<string, string> = { code: "Code", packages: "Packages", settings: "Settings & files", database: "Database", other: "Other" }
export const bucketLabel = (bucket: string) => bucketLabels[bucket] ?? bucket

/** "2 code · 1 packages", in the order the bucket counts arrive; "No changes" when every bucket is empty. */
export function bucketSummary(counts: Record<string, number>): string {
  const parts = Object.entries(counts).filter(([, count]) => count > 0).map(([bucket, count]) => `${count} ${bucketLabel(bucket).toLowerCase()}`)
  return parts.length ? parts.join(" · ") : "No changes"
}

export interface PendingChangesState {
  /** The last list read, kept while a later read fails. */
  list: RequestList | null
  /** The message of the latest failed read; cleared by the next success. */
  error: string | null
  loading: boolean
  refresh: () => void
}

/** Reads the review requests of every device while `active`, backing off while reads fail. */
export function usePendingChanges(load: ApplicationActions["listReviewRequests"], active = true): PendingChangesState {
  const [list, setList] = useState<RequestList | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(Boolean(load))
  const reload = useRef<() => void>(() => {})

  useEffect(() => {
    if (!active || !load) return
    let disposed = false
    let failures = 0
    let timer: number | undefined
    let reading = false
    const read = async () => {
      if (reading) return
      reading = true
      window.clearTimeout(timer)
      try {
        const next = await load({ status: "open", allDevices: true })
        if (disposed) return
        failures = 0
        setList(next)
        setError(null)
      } catch (cause) {
        if (disposed) return
        failures += 1
        setError(errorMessage(cause, { fallback: "Silo could not read the pending changes. Retry; if it fails again, relaunch Silo." }))
      } finally {
        reading = false
        if (!disposed) {
          setLoading(false)
          timer = window.setTimeout(() => void read(), pendingChangesDelay(failures))
        }
      }
    }
    reload.current = () => void read()
    void read()
    return () => { disposed = true; window.clearTimeout(timer); reload.current = () => {} }
  }, [active, load])

  const refresh = useCallback(() => reload.current(), [])
  return { list, error, loading, refresh }
}
