/* oxlint-disable react/only-export-components */
import { errorMessage } from "@/lib/error-message"
import { createContext, useCallback, useContext, useEffect, useRef, useState, type ReactNode } from "react"
import { z } from "zod"

import { formatBinaryBytes } from "@/lib/format-bytes"

/**
 * The previous computer storage, kept after an upgrade that converted every computer. Silo deletes it
 * on its own 14 days later. `deleteAt` is that instant; it is null when Silo cannot read its saved
 * date, in which case the backup is never deleted automatically but can still be deleted by hand.
 */
export const preUpgradeBackupSchema = z.object({
  deleteAt: z.string().refine(value => !Number.isNaN(Date.parse(value)), "Expected a date").nullable(),
  /** The migration screen has not yet told the user that the backup exists. */
  noticePending: z.boolean(),
})
export type PreUpgradeBackup = z.infer<typeof preUpgradeBackupSchema>

export interface PreUpgradeBackupBackend {
  /** Null when there is no backup to offer. Does not measure it. */
  read: () => Promise<PreUpgradeBackup | null>
  /** Allocated bytes; null once the backup is gone. Walks the folder, so call it only where it is shown. */
  measure: () => Promise<number | null>
  reveal: () => Promise<void>
  remove: () => Promise<void>
  acknowledge: () => Promise<void>
  subscribe: (refresh: () => void) => Promise<() => void>
}

export type PreUpgradeBackupSize = number | "measuring" | "unavailable"

export interface PreUpgradeBackupState {
  /** False until the first read has finished. */
  loaded: boolean
  backup: PreUpgradeBackup | null
  size: PreUpgradeBackupSize
  /** Why the backup could not be read; the last known backup stays visible. */
  loadError: string | null
  removing: boolean
  /** Reads again. */
  retry: () => void
  /** Rejects with the reason the backup could not be deleted; the backup stays so the user can retry. */
  remove: () => Promise<void>
  reveal: () => Promise<void>
  acknowledge: () => Promise<void>
}

/**
 * Reads the backup when mounted and whenever Silo reports a change. Pass no backend to see none.
 * Measuring walks the folder, so it happens only while `measure` is true: where the size is shown.
 */
export function usePreUpgradeBackup(backend: PreUpgradeBackupBackend | undefined, { measure = true }: { measure?: boolean } = {}): PreUpgradeBackupState {
  const [loaded, setLoaded] = useState(!backend)
  const [backup, setBackup] = useState<PreUpgradeBackup | null>(null)
  const [size, setSize] = useState<PreUpgradeBackupSize>("measuring")
  const [loadError, setLoadError] = useState<string | null>(null)
  const [subscriptionError, setSubscriptionError] = useState<string | null>(null)
  const [connection, setConnection] = useState(0)
  const [removing, setRemoving] = useState(false)
  const removal = useRef<Promise<void> | null>(null)
  const reads = useRef({ sequence: 0 })
  const refresh = useRef<() => Promise<void>>(async () => {})

  useEffect(() => {
    if (!backend) return
    let live = true
    const requests = reads.current
    let unsubscribe: (() => void) | undefined
    refresh.current = async () => {
      if (!live) return
      const mine = ++requests.sequence
      try {
        const next = await backend.read()
        if (!live || mine !== requests.sequence) return
        setBackup(next)
        setLoadError(null)
        setLoaded(true)
      } catch (cause) {
        if (live && mine === requests.sequence) { setLoadError(errorMessage(cause)); setLoaded(true) }
      }
    }
    void backend.subscribe(() => { if (live) void refresh.current() }).then(stop => {
      if (live) { unsubscribe = stop; setSubscriptionError(null) }
      else stop()
    }).catch(() => {
      if (live) setSubscriptionError("Silo could not listen for backup changes. Try again.")
    }).then(() => {
      if (live) void refresh.current()
    })
    return () => { live = false; unsubscribe?.() }
  }, [backend, connection])

  // Every new read of a present backup is measured again: a failed deletion may have removed part of it.
  const present = backup !== null
  useEffect(() => {
    if (!backend || !measure || !present) return
    let live = true
    backend.measure().then(
      bytes => { if (live) setSize(bytes ?? "unavailable") },
      () => { if (live) setSize("unavailable") },
    )
    return () => { live = false }
  }, [backend, measure, present, backup])

  const remove = useCallback(() => {
    if (!backend) return Promise.resolve()
    if (removal.current) return removal.current
    ++reads.current.sequence
    setRemoving(true)
    removal.current = Promise.resolve().then(async () => {
      try {
        await backend.remove()
        ++reads.current.sequence
        setBackup(null)
        setLoadError(null)
      } catch (cause) {
        // A failed deletion may have removed part of it: show what is left.
        void refresh.current()
        throw cause
      } finally {
        removal.current = null
        setRemoving(false)
      }
    })
    return removal.current
  }, [backend])

  return {
    loaded,
    backup,
    size,
    loadError: loadError ?? subscriptionError,
    removing,
    retry: () => {
      if (subscriptionError) setConnection(value => value + 1)
      else void refresh.current()
    },
    remove,
    reveal: async () => { await backend?.reveal() },
    acknowledge: async () => { await backend?.acknowledge() },
  }
}

const Backend = createContext<PreUpgradeBackupBackend | undefined>(undefined)

export function PreUpgradeBackupProvider({ backend, children }: { backend: PreUpgradeBackupBackend; children: ReactNode }) {
  return <Backend value={backend}>{children}</Backend>
}

/** The backend of the enclosing provider; none in windows and previews that have no previous storage. */
export function usePreUpgradeBackupBackend() {
  return useContext(Backend)
}

/** Allocated bytes in binary units, as the Storage tab shows them. */
export function formatBackupSize(size: PreUpgradeBackupSize) {
  return size === "measuring" ? "Calculating size…" : size === "unavailable" ? "Size unavailable" : formatBinaryBytes(size)
}

/** The local calendar date of the instant Silo deletes the backup: a date, not a countdown. */
export function formatDeleteDate(deleteAt: string) {
  return new Date(deleteAt).toLocaleDateString("en", { year: "numeric", month: "long", day: "numeric" })
}

/** "deleted on October 15, 2026", or that it will not be deleted by itself. */
export function deletionSummary(backup: PreUpgradeBackup) {
  return backup.deleteAt ? `deleted on ${formatDeleteDate(backup.deleteAt)}` : "not deleted automatically"
}

/** The same fact as a sentence for the migration screen. */
export function automaticDeletionSentence(backup: PreUpgradeBackup) {
  return backup.deleteAt ? `Silo deletes it automatically on ${formatDeleteDate(backup.deleteAt)}.` : "Silo will not delete it automatically."
}

/** Shared by every "Delete now" confirmation. Deleting is permanent, so it names what goes and what stays. */
export function deleteConfirmation(size: PreUpgradeBackupSize) {
  const freed = typeof size === "number" ? `frees up to ${formatBinaryBytes(size)}` : "frees its disk space"
  return {
    title: "Delete the pre-upgrade backup permanently?",
    description: `This deletes the copy of your computers from before the upgrade and ${freed}. It can't be undone. Your current computers aren't affected.`,
    confirmLabel: "Delete permanently",
  }
}

/** Linux disk images: a file manager can show and copy the folder, but nothing in it can be browsed. */
export const preUpgradeBackupContents = "It holds Linux disk images, so you can copy it but not browse its files."
