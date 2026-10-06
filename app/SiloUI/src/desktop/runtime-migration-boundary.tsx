import { invoke } from "@tauri-apps/api/core"
import { listen } from "@tauri-apps/api/event"
import { useEffect, useRef, useState, type ReactNode } from "react"
import { AlertCircle, LoaderCircle } from "lucide-react"
import { z } from "zod"
import { SiloWindow } from "@/components/silo-window"
import { Button } from "@/components/ui/button"
import { MigrationBackupGate } from "@/desktop/migration-backup-notice"
import { desktopPreUpgradeBackupBackend } from "@/desktop/pre-upgrade-backup"
import { desktopTransferResultNoticeBackend } from "@/desktop/transfer-result-notice"
import type { TransferResultNoticeBackend } from "@/features/application/model/transfer-result-notice"
import type { PreUpgradeBackupBackend } from "@/features/storage/pre-upgrade-backup"

const migrationStateSchema = z.object({
  status: z.enum(["not-required", "scanning", "running", "failed", "complete"]),
  stage: z.string(),
  logs: z.array(z.string()),
  migratedCount: z.number().int().nonnegative(),
  failedCount: z.number().int().nonnegative(),
  totalCount: z.number().int().nonnegative(),
  canContinue: z.boolean(),
  logPath: z.string().optional(),
  error: z.string().optional(),
})

export type RuntimeMigrationState = z.infer<typeof migrationStateSchema>
export interface RuntimeMigrationBackend {
  read: () => Promise<RuntimeMigrationState>
  retry: () => Promise<RuntimeMigrationState>
  continueAfterFailure: () => Promise<RuntimeMigrationState>
  subscribe: (refresh: () => void) => Promise<() => void>
  /** The backup a finished migration leaves behind. Without it, a finished migration opens Silo at once. */
  preUpgradeBackup?: PreUpgradeBackupBackend
  /** The export or import result an upgrade produced, which the screen about the backup also shows. */
  transferResult?: TransferResultNoticeBackend
}

const nativeBackend: RuntimeMigrationBackend = {
  read: async () => migrationStateSchema.parse(await invoke("read_runtime_migration_state")),
  retry: async () => migrationStateSchema.parse(await invoke("retry_runtime_migration")),
  continueAfterFailure: async () => migrationStateSchema.parse(await invoke("continue_after_migration_failure")),
  subscribe: async (refresh) => listen("silo://application-state-changed", refresh),
  preUpgradeBackup: desktopPreUpgradeBackupBackend,
  transferResult: desktopTransferResultNoticeBackend,
}

function message(cause: unknown) {
  return cause instanceof Error ? cause.message : String(cause)
}

function issueUrl(state: RuntimeMigrationState) {
  const title = "Silo computer migration failed"
  const body = [
    "Silo computer migration failed while upgrading the runtime.",
    `Stage: ${state.stage}`,
    `Migrated: ${state.migratedCount} of ${state.totalCount}; failed: ${state.failedCount}.`,
    "Please describe what happened. Attach logs only after checking them for private data.",
  ].join("\n")
  return `https://github.com/amontlabs/silo/issues/new?title=${encodeURIComponent(title)}&body=${encodeURIComponent(body)}`
}

export function RuntimeMigrationBoundary({ children, backend = nativeBackend }: { children: ReactNode; backend?: RuntimeMigrationBackend }) {
  const [state, setState] = useState<RuntimeMigrationState | null>(null)
  const [readError, setError] = useState<string | null>(null)
  // Without events a running migration would never update, so a failed subscription blocks Silo like a failed read.
  const [subscriptionError, setSubscriptionError] = useState<string | null>(null)
  const error = subscriptionError ?? readError
  // A settled status opens Silo only once the subscription worked, so a failing event channel is never hidden.
  const [subscribed, setSubscribed] = useState(false)
  const [busy, setBusy] = useState(false)
  const [showLogs, setShowLogs] = useState(false)
  const [acknowledged, setAcknowledged] = useState(false)
  // Each attempt subscribes and reads again, so Retry recovers from either failing.
  const [attempt, setAttempt] = useState(0)
  const publicationSequence = useRef({ sequence: 0 })

  useEffect(() => {
    const publication = publicationSequence.current
    let active = true
    let unsubscribe: (() => void) | undefined
    let listening = false
    // Once the migration is settled nothing more can change: stop listening.
    let settled = false
    // A read that started before the subscription completed may have missed an event.
    let recheck = false
    let eventSeen = false
    async function read() {
      const sequence = ++publication.sequence
      const beforeSubscription = !listening
      try {
        const next = await backend.read()
        if (active && sequence === publication.sequence) {
          setState(next)
          setError(null)
          if (next.status === "not-required" || next.status === "complete") {
            settled = true
            unsubscribe?.()
            unsubscribe = undefined
            return
          }
        }
      } catch (cause) {
        if (active && sequence === publication.sequence) setError(message(cause))
        return
      }
      if (!active || settled || sequence !== publication.sequence || !beforeSubscription) return
      if (listening) void read()
      else recheck = true
    }
    // The first read does not wait for the subscription; the subscription then closes the gap.
    void read()
    void backend.subscribe(() => {
      if (!active || settled) return
      eventSeen = true
      void read()
    }).then(stop => {
      if (!active) { stop(); return }
      setSubscribed(true)
      if (settled) { stop(); return }
      unsubscribe = stop
      listening = true
      if (recheck && !eventSeen) { recheck = false; void read() }
    }).catch(cause => { if (active) setSubscriptionError(message(cause)) })
    return () => { active = false; ++publication.sequence; unsubscribe?.() }
  }, [backend, attempt])

  async function run(operation: () => Promise<RuntimeMigrationState>, readAfter = false) {
    const sequence = ++publicationSequence.current.sequence
    setBusy(true)
    setError(null)
    try {
      let next = await operation()
      if (sequence !== publicationSequence.current.sequence) return
      // Retry returns its launch snapshot; conversion can already have finished.
      if (readAfter) next = await backend.read()
      if (sequence === publicationSequence.current.sequence) { setState(next); setError(null) }
    } catch (cause) {
      if (sequence === publicationSequence.current.sequence) setError(message(cause))
    } finally { setBusy(false) }
  }

  const settledState = state?.status === "not-required" || state?.status === "complete"
  if (state?.status === "not-required" && !error && subscribed) return children
  if (state?.status === "complete" && !error && subscribed) return backend.preUpgradeBackup ? <MigrationBackupGate backend={backend.preUpgradeBackup} transferResult={backend.transferResult}>{children}</MigrationBackupGate> : children
  // Most launches need no migration: stay neutral until the first status arrives
  // instead of briefly announcing an update that is not happening.
  if ((!state || settledState) && !error) return <SiloWindow title="Silo" label="Silo">
    <span role="status" className="sr-only">Opening Silo…</span>
  </SiloWindow>
  const failed = state?.status === "failed"
  const issue = state ? issueUrl(state) : null
  return <SiloWindow title="Silo" label="Silo migration">
    <main className="mx-auto flex min-h-0 w-full max-w-3xl flex-1 flex-col gap-4 overflow-y-auto px-6 py-8">
      <div className="flex items-start gap-3">
        {failed || error ? <AlertCircle aria-hidden="true" className="mt-0.5 size-5 text-destructive" /> : <LoaderCircle aria-hidden="true" className="mt-0.5 size-5 animate-spin motion-reduce:animate-none" />}
        <div>
          <h1 className="text-lg font-semibold">{failed ? "Some computers could not be migrated" : error ? "Migration status is unavailable" : "Updating your computers"}</h1>
          <p className="mt-1 text-sm text-muted-foreground">{state?.stage ?? "Checking saved computers…"}</p>
        </div>
      </div>
      {state && <p role="status" className="text-xs text-muted-foreground">{state.migratedCount} of {state.totalCount} migrated{state.failedCount ? ` · ${state.failedCount} failed` : ""}</p>}
      {(state?.error || error) && <p role="alert" className="rounded-md border border-destructive/30 bg-destructive/[.06] p-3 text-sm text-destructive">{error ?? state?.error}</p>}
      {error && !failed && <div><Button size="sm" disabled={busy} onClick={() => { setError(null); setSubscriptionError(null); setSubscribed(false); setAttempt(value => value + 1) }}>Retry</Button></div>}
      {state && <section aria-label="Migration log" className="min-h-0 rounded-md border bg-muted/30">
        <div className="flex items-center justify-between border-b px-3 py-2"><h2 className="text-xs font-medium">Live migration log</h2><Button size="xs" variant="ghost" onClick={() => setShowLogs(value => !value)}>{showLogs ? "Hide logs" : "Show logs"}</Button></div>
        {showLogs && <div className="p-3"><pre className="max-h-64 overflow-auto whitespace-pre-wrap break-words font-mono text-[11px]" aria-live="polite">{state.logs.length ? state.logs.join("\n") : "Waiting for migration output…"}</pre>{state.logPath && <p className="mt-2 break-all text-[11px] text-muted-foreground">Full log: {state.logPath}</p>}</div>}
      </section>}
      {failed && state && <div className="space-y-3 rounded-md border border-warning/30 bg-warning/[.05] p-3 text-xs">
        <p>Review the logs and retry. You can back up your work yourself before continuing. Continuing leaves unmigrated originals in place; affected computers may be unavailable in the new runtime.</p>
        <label className="flex items-start gap-2"><input type="checkbox" className="mt-0.5" checked={acknowledged} onChange={event => setAcknowledged(event.target.checked)} /><span>I understand that failed computers have not been converted and will remain unavailable until recovered.</span></label>
        <div className="flex flex-wrap gap-2">
          <Button size="sm" disabled={busy} onClick={() => void run(backend.retry, true)}>Retry migration</Button>
          {issue && <Button size="sm" variant="outline" asChild><a href={issue} target="_blank" rel="noopener noreferrer">Prepare GitHub issue</a></Button>}
          <Button size="sm" variant="outline" disabled={busy || !acknowledged || !state.canContinue} onClick={() => void run(backend.continueAfterFailure)}>Continue with available computers</Button>
        </div>
      </div>}
    </main>
  </SiloWindow>
}
