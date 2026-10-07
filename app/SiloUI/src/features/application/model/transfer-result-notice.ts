import { errorMessage } from "@/lib/error-message"
import { useCallback, useEffect, useRef, useState } from "react"

import type { BackupOperationKind } from "@/features/application/model/backup-source"

/**
 * An export or import result the user was not shown as it happened: one an upgrade produced (an
 * operation interrupted before it, settled before or after the storage migration), or the notice
 * that an unreadable export or import record was set aside. Every other result present when Silo
 * opens belongs to an earlier session and stays silent; this one is shown until it is acknowledged,
 * either on the screen shown after an upgrade or as the export and import notification.
 */
export interface TransferResultNotice {
  /** The operation id of the result. Acknowledging names it, so no other result is ever marked seen. */
  id: string
  operation: BackupOperationKind
  outcome: "success" | "failed" | "cancelled"
  title: string
  message: string
  detail?: string
}

export interface TransferResultNoticeBackend {
  /** Null when there is no unseen result. */
  read: () => Promise<TransferResultNotice | null>
  /** Marks the result with this id seen. It stays until the export and import page dismisses it. */
  acknowledge: (id: string) => Promise<void>
  /** Calls `refresh` when the result may have changed, for example once recovery has finished. */
  subscribe: (refresh: () => void) => Promise<() => void>
}

export interface TransferResultNoticeState {
  /** The unseen result, once it is known. */
  notice: TransferResultNotice | null
  /** Rejects when the result could not be marked seen; the application then shows it itself. */
  acknowledge: () => Promise<void>
}

/**
 * Reads the unseen result while `enabled`, and again whenever Silo reports a change: recovery of an
 * operation that waited for the upgrade finishes after the screen opens. Without a backend there
 * is none. A failed read is logged and shows nothing, so it never blocks Silo from opening.
 */
export function useTransferResultNotice(backend: TransferResultNoticeBackend | undefined, enabled: boolean): TransferResultNoticeState {
  const [notice, setNotice] = useState<TransferResultNotice | null>(null)
  // Once the user acknowledged, the result stays on the screen that showed it instead of vanishing
  // when Silo reports that the result changed.
  const acknowledging = useRef(false)

  useEffect(() => {
    if (!backend || !enabled) return
    let live = true
    let sequence = 0
    let unsubscribe: (() => void) | undefined
    const refresh = async () => {
      if (!live) return
      const mine = ++sequence
      try {
        const next = await backend.read()
        if (live && mine === sequence && !acknowledging.current) setNotice(next)
      } catch (cause) {
        console.error("Silo export and import result:", errorMessage(cause))
      }
    }
    void backend.subscribe(() => { if (live) void refresh() }).then(stop => {
      if (live) unsubscribe = stop
      else stop()
    }).catch((cause: unknown) => console.error("Silo export and import result:", errorMessage(cause))).then(() => {
      if (live) void refresh()
    })
    return () => { live = false; unsubscribe?.() }
  }, [backend, enabled])

  const acknowledge = useCallback(async () => {
    if (!backend || !notice) return
    acknowledging.current = true
    try {
      await backend.acknowledge(notice.id)
    } catch (cause) {
      acknowledging.current = false
      throw cause
    }
  }, [backend, notice])

  return { notice, acknowledge }
}
