import { useCallback, useEffect, useRef, useState } from "react"

import type { GuestComponent, NetworkMode, NetworkPolicy, NetworkPolicyState, NetworkTiming } from "@/desktop/production-schemas"
import type { ApplicationActions } from "@/features/application/model/application-source"
import { errorMessage } from "@/lib/error-message"

export const modeChoices: readonly { mode: NetworkMode; label: string; description: string }[] = [
  { mode: "open", label: "Open", description: "Can reach any public address." },
  { mode: "allowlist", label: "Allowlist", description: "Only the hosts you list." },
  { mode: "closed", label: "Closed", description: "Nothing out." },
]

export function modeLabel(mode: NetworkMode): string {
  return modeChoices.find(choice => choice.mode === mode)?.label ?? mode
}

const sameSet = (a: readonly string[], b: readonly string[]) => a.length === b.length && [...a].sort().every((value, index) => value === [...b].sort()[index])

/** Whether two policies are the same one; the order of hosts and presets does not matter. */
export function samePolicy(a: NetworkPolicy, b: NetworkPolicy): boolean {
  return a.mode === b.mode && sameSet(a.hosts, b.hosts) && sameSet(a.presets, b.presets)
}

/** When a saved change takes effect, in a sentence; nothing when it already has. */
export function timingText(timing: NetworkTiming): string | undefined {
  switch (timing) {
    case "applied": return undefined
    case "nextStart": return "This applies when the computer next starts."
    case "nextRestart": return "This applies when the computer restarts. Until then it keeps its previous rules."
  }
}

/** Why Silo itself allows a host, for a host the engine added on its own; hosts from the list or a preset have no such reason. */
export function addedByReason(from: string): string | undefined {
  if (from === "github") return "For GitHub access"
  if (from.startsWith("secret:")) return `For the secret ${from.slice("secret:".length)}`
  return undefined
}

export function hostCount(count: number): string {
  return `${count} more ${count === 1 ? "host" : "hosts"}`
}

/** Whether an error says the policy changed while the person was looking at it, so their draft is out of date. */
export function isStaleSave(message: string): boolean {
  return /read it again/i.test(message)
}

type Read = NonNullable<ApplicationActions["readNetworkPolicy"]>
type Save = NonNullable<ApplicationActions["saveNetworkPolicy"]>

/**
 * One computer's network policy: what the engine says it is, and a draft the person edits.
 * Saving sends the policy the draft started from, so a change made meanwhile is refused.
 */
export function useNetworkPolicy({ read, save, target, enabled, refreshKey }: {
  read: Read | undefined
  save: Save | undefined
  target: string
  enabled: boolean
  /** A change reads the policy again, for example a computer starting or stopping. */
  refreshKey?: string
}) {
  const [state, setState] = useState<NetworkPolicyState>()
  const [draft, setDraft] = useState<NetworkPolicy>()
  const [loadError, setLoadError] = useState<string | null>(null)
  const [saveError, setSaveError] = useState<string | null>(null)
  const [saving, setSaving] = useState(false)
  const requests = useRef(0)
  const base = useRef<NetworkPolicy | undefined>(undefined)

  const adopt = useCallback((next: NetworkPolicyState, discard: boolean) => {
    const previous = base.current
    base.current = next.policy
    setState(next)
    setDraft(current => discard || !current || !previous || samePolicy(current, previous) ? next.policy : current)
  }, [])

  const load = useCallback(async (discard = false) => {
    if (!read) return
    const request = ++requests.current
    try {
      const next = await read(target)
      if (request !== requests.current) return
      adopt(next, discard)
      setLoadError(null)
    } catch (cause) {
      if (request === requests.current) setLoadError(errorMessage(cause))
    }
  }, [read, target, adopt])

  useEffect(() => {
    if (!enabled) return
    // oxlint-disable-next-line react/set-state-in-effect
    void load()
    return () => { requests.current += 1 }
  }, [enabled, load, refreshKey])

  const submit = useCallback(async (policy: NetworkPolicy) => {
    if (!save || !state) return
    setSaving(true)
    setSaveError(null)
    try {
      const next = await save(target, policy, state.policy)
      requests.current += 1
      adopt(next, true)
    } catch (cause) {
      setSaveError(errorMessage(cause))
    } finally {
      setSaving(false)
    }
  }, [save, state, target, adopt])

  const dirty = Boolean(state && draft && !samePolicy(state.policy, draft))
  return {
    state, draft, setDraft, dirty, saving, loadError, saveError,
    loading: enabled && !state && !loadError,
    submit,
    reload: (discard = false) => { setSaveError(null); return load(discard) },
    discard: () => { setSaveError(null); if (state) setDraft(state.policy) },
  }
}

type ReadTools = NonNullable<ApplicationActions["readComputerTools"]>
type SetTools = NonNullable<ApplicationActions["setComputerTools"]>

/** Whether the agent inside a computer has Silo's tools, and the switch that changes it. */
export function useComputerTools({ read, set, target, enabled, refreshKey }: {
  read: ReadTools | undefined
  set: SetTools | undefined
  target: string
  enabled: boolean
  refreshKey?: string
}) {
  const [tools, setTools] = useState<Awaited<ReturnType<ReadTools>>>()
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const requests = useRef(0)
  const load = useCallback(async () => {
    if (!read) return
    const request = ++requests.current
    try {
      const next = await read(target)
      if (request !== requests.current) return
      setTools(next)
      setError(null)
    } catch (cause) {
      if (request === requests.current) setError(errorMessage(cause))
    }
  }, [read, target])
  useEffect(() => {
    if (!enabled) return
    // oxlint-disable-next-line react/set-state-in-effect
    void load()
    return () => { requests.current += 1 }
  }, [enabled, load, refreshKey])
  const change = useCallback(async (on: boolean) => {
    if (!set) return
    setBusy(true)
    setError(null)
    try {
      const next = await set(target, on)
      requests.current += 1
      setTools(next)
    } catch (cause) {
      setError(errorMessage(cause))
    } finally {
      setBusy(false)
    }
  }, [set, target])
  return { tools, error, busy, loading: enabled && !tools && !error, change, reload: load }
}

const componentNames: Record<string, string> = {
  silo: "the silo command",
  "silo-mcp": "the MCP server for your agents",
}

/** A piece of Silo's tools that is not in place, in plain words; nothing for one that is. */
export function componentProblem(component: GuestComponent): string | undefined {
  const name = componentNames[component.id] ?? component.id
  const reason = component.detail ? ` ${component.detail}` : ""
  switch (component.state) {
    case "failed": return `Could not set up ${name}.${reason}`
    case "unavailable": return `${name.charAt(0).toUpperCase()}${name.slice(1)} is not available in this version of Silo.`
    case "waiting": return `${name.charAt(0).toUpperCase()}${name.slice(1)} is waiting.${reason}`
    default: return undefined
  }
}
