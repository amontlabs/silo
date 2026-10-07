import { useCallback, useEffect, useRef, useState } from "react"
import { Switch } from "@/components/ui/switch"
import { useComputerUseBridge } from "./computer-use-bridge"
import type { ComputerUseApproval, ComputerUseState, LinuxDesktopState } from "./linux-desktop-state"

export interface ComputerUseApprovalController {
  /** The computer's state as last read; null until the first read. */
  state: LinuxDesktopState | null
  /** Absent for a computer from before computer use was built in. */
  computerUse: ComputerUseState | null | undefined
  running: boolean
  busy: boolean
  /** The last change failed. Kept until the next change or `dismissError`. */
  error: string | null
  /** The latest read failed; `state` may be stale. */
  loadError: string | null
  setApproval(mode: ComputerUseApproval): void
  /** Configures the agents installed in the computer again. Only possible while it runs. */
  setup(): void
  refresh(): void
  dismissError(): void
}

/** Reads and changes one computer's computer use, polling only while `active` and the window is visible. Returns null without a bridge.
 * Use with a `computer` that does not change for the life of the caller: its reads belong to one computer. */
export function useComputerUseApproval(computer: string, pollMs = 5000, active = true): ComputerUseApprovalController | null {
  const bridge = useComputerUseBridge()
  const [state, setState] = useState<LinuxDesktopState | null>(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [loadError, setLoadError] = useState<string | null>(null)
  const working = useRef(false)
  const reading = useRef(false)
  const revision = useRef(0)
  const failureDelay = useRef(0)
  const refresh = useCallback(async () => {
    // One read at a time, so a slow one can never be overtaken by a newer one and then overwrite it.
    if (!bridge || working.current || reading.current) return
    reading.current = true
    const current = revision.current
    try {
      const next = await bridge.readState(computer)
      if (current === revision.current) {
        failureDelay.current = 0
        setState(previous => JSON.stringify(previous) === JSON.stringify(next) ? previous : next)
        setLoadError(null)
      }
    } catch (cause) {
      if (current === revision.current) {
        failureDelay.current = Math.min(Math.max(failureDelay.current, pollMs) * 2, 30000)
        setLoadError(cause instanceof Error ? cause.message : String(cause))
      }
    }
    finally { reading.current = false }
  }, [bridge, computer, pollMs])
  useEffect(() => {
    if (!active) return
    let disposed = false
    let timer: number | undefined
    const update = () => {
      window.clearTimeout(timer)
      if (disposed || document.visibilityState === "hidden") return
      void refresh().finally(() => {
        if (disposed || document.visibilityState === "hidden") return
        window.clearTimeout(timer)
        timer = window.setTimeout(update, Math.max(pollMs, failureDelay.current))
      })
    }
    timer = window.setTimeout(update, 0)
    document.addEventListener("visibilitychange", update)
    return () => { disposed = true; window.clearTimeout(timer); document.removeEventListener("visibilitychange", update) }
  }, [refresh, pollMs, active])
  const run = useCallback(async (work: () => Promise<LinuxDesktopState>, optimistic: (state: LinuxDesktopState) => LinuxDesktopState) => {
    if (working.current) return
    working.current = true
    revision.current += 1
    setBusy(true)
    setError(null)
    const previous = state
    setState(current => current ? optimistic(current) : current)
    try {
      setState(await work()); setLoadError(null)
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause))
      // The change may have been stored or even applied before it failed to answer: show what the
      // computer's own state says now, not the snapshot from before the change. Only when that read
      // fails too does the old snapshot stand, with its read error.
      try { setState(await bridge?.readState(computer) ?? previous); setLoadError(null) }
      catch (readCause) { setLoadError(readCause instanceof Error ? readCause.message : String(readCause)); setState(previous) }
    }
    finally { working.current = false; setBusy(false) }
  }, [bridge, state, computer])
  if (!bridge) return null
  return {
    state,
    computerUse: state?.computerUse,
    running: state?.state === "running",
    busy,
    error,
    loadError,
    setApproval: mode => { void run(() => bridge.setApproval(computer, mode), current => ({ ...current, computerUse: current.computerUse ? { ...current.computerUse, approval: mode, approvalApply: "pending", approvalApplyReason: null } : current.computerUse })) },
    setup: () => { void run(() => bridge.setup(computer), current => ({ ...current, computerUse: current.computerUse ? { ...current.computerUse, state: "installing", reason: null } : current.computerUse })) },
    refresh: () => { void refresh() },
    dismissError: () => setError(null),
  }
}

/** Where the approval of a computer's computer use stands, for its switch and warning. */
export function approvalStatus(computerUse: ComputerUseState, running: boolean) {
  const apply = computerUse.approvalApply
  const applied = computerUse.appliedApproval
  const unknown = computerUse.approval === "unknown"
  // After choosing ask, agents may still act without asking when the previous applied mode was auto, only
  // some agents were changed, or the change failed and nothing says ask is in place.
  const mayActWithoutAsking = computerUse.approval === "ask" && (applied === "auto" || apply === "partial" || (apply === "failed" && applied !== "ask"))
  const mayStillAsk = computerUse.approval === "auto" && (apply === "partial" || apply === "failed" || applied === "ask")
  // An older owner does not report `approvalApply` (read as `applied`) but still reports both modes: whatever the
  // result says, a chosen mode that differs from the applied one is not in place yet.
  const differs = !unknown && applied !== "unknown" && applied !== computerUse.approval
  const failed = apply === "failed" || apply === "partial"
  const pending = apply === "pending" || differs
  const note = unknown ? "Silo could not read the approval setting. Agents may be running without asking."
    : failed && mayActWithoutAsking ? "Not applied to every agent. Some may still act without asking."
    : failed && mayStillAsk ? "Not applied to every agent. Some may still ask first."
    : failed ? "Silo could not apply the approval change."
    : pending && mayActWithoutAsking ? "Agents may still act without asking until this is applied."
    : pending && mayStillAsk ? "Agents may still ask first until this is applied."
    : null
  return {
    note,
    /** The warning is about agents acting without asking. */
    risky: unknown || mayActWithoutAsking,
    detail: failed ? computerUse.approvalApplyReason : null,
    applying: apply === "pending" && running,
  }
}

/** The "Allow without asking" switch of one computer, with a quiet "Applying…" beside it. It works while the computer is
 * stopped: the choice is stored and applied at start. Renders nothing until the computer's computer use is known. */
export function ComputerUseApprovalSwitch({ approval, label = "Allow without asking", labelledBy }: { approval: ComputerUseApprovalController | null; label?: string; labelledBy?: string }) {
  const computerUse = approval?.computerUse
  if (!approval || !computerUse) return null
  const status = approvalStatus(computerUse, approval.running)
  return <span className="flex shrink-0 items-center gap-2">
    {status.applying && <span role="status" className="text-caption text-muted-foreground">Applying…</span>}
    <Switch aria-label={labelledBy ? undefined : label} aria-labelledby={labelledBy} checked={computerUse.approval === "auto"} disabled={approval.busy || computerUse.approval === "unknown"}
      onCheckedChange={checked => approval.setApproval(checked ? "auto" : "ask")} />
  </span>
}
