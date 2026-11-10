import { useCallback, useEffect, useState } from "react"

import type { AuditCallerKind, AuditEntry } from "@/desktop/production-schemas"
import { errorMessage } from "@/lib/error-message"
import type { ApplicationActions } from "./application-source"

/** Rows read per page. */
export const AUDIT_PAGE_SIZE = 50

export interface AuditFilters { computerId?: string; callerKind?: AuditCallerKind }

export interface AuditLogState {
  entries: AuditEntry[]
  loading: boolean
  /** The message of the latest failed read. */
  error: string | null
  /** Whether older rows can be read. */
  hasMore: boolean
  loadMore: () => void
  refresh: () => void
}

/** Reads the audit log newest first and continues from the cursor of the last page; reading again starts over when the filters change. */
export function useAuditLog(read: ApplicationActions["listAudit"], { computerId, callerKind }: AuditFilters, active: boolean): AuditLogState {
  const [entries, setEntries] = useState<AuditEntry[]>([])
  const [cursor, setCursor] = useState<number | undefined>()
  const [loading, setLoading] = useState(Boolean(read))
  const [error, setError] = useState<string | null>(null)
  const [version, setVersion] = useState(0)
  const [more, setMore] = useState(0)

  useEffect(() => {
    if (!read || !active) return
    let current = true
    // oxlint-disable-next-line react/set-state-in-effect
    setLoading(true)
    read({ computerId, callerKind, limit: AUDIT_PAGE_SIZE }).then(
      page => { if (current) { setEntries(page.entries); setCursor(page.nextBeforeId); setError(null); setLoading(false) } },
      cause => { if (current) { setError(errorMessage(cause, { fallback: "Silo could not read the activity log." })); setLoading(false) } },
    )
    return () => { current = false }
  }, [read, active, computerId, callerKind, version])

  useEffect(() => {
    if (!more || !read || cursor === undefined) return
    let current = true
    // oxlint-disable-next-line react/set-state-in-effect
    setLoading(true)
    read({ computerId, callerKind, beforeId: cursor, limit: AUDIT_PAGE_SIZE }).then(
      page => {
        if (!current) return
        setEntries(existing => {
          const known = new Set(existing.map(({ id }) => id))
          return [...existing, ...page.entries.filter(({ id }) => !known.has(id))]
        })
        setCursor(page.nextBeforeId); setError(null); setLoading(false)
      },
      cause => { if (current) { setError(errorMessage(cause, { fallback: "Silo could not read older activity." })); setLoading(false) } },
    )
    return () => { current = false }
    // Only a new request for older rows reads; the cursor it uses is the one in hand then.
    // oxlint-disable-next-line react-hooks/exhaustive-deps
  }, [more])

  const loadMore = useCallback(() => setMore(value => value + 1), [])
  const refresh = useCallback(() => { setCursor(undefined); setMore(0); setVersion(value => value + 1) }, [])
  return { entries, loading, error, hasMore: cursor !== undefined, loadMore, refresh }
}
