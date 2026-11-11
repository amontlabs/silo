import { useCallback, useEffect, useRef, useState } from "react"

import type { GuestComponent, NetworkMode, NetworkPolicy, NetworkPolicyState, NetworkTiming } from "@/desktop/production-schemas"
import type { ApplicationActions } from "@/features/application/model/application-source"
import { errorMessage } from "@/lib/error-message"

import publicSuffixes from "../../../../src-tauri/engine/src/network_policy/public_suffixes.json"

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

/** The engine's own lists, so a host is refused here for the same reasons as when it is saved. */
const RESERVED_TOP_LEVEL = new Set(["internal", "local", "localhost", "arpa", "invalid", "test", "onion", "lan", "home", "localdomain"])
const PUBLIC_SUFFIXES = new Set(publicSuffixes.suffixes)

/** Why the engine would refuse a host pattern, or nothing when it would take it. Mirrors `validate_host`. */
export function validateHost(host: string): string | undefined {
  const refuse = (why: string) => `"${host}" is not a host Silo can allow: ${why}.`
  const name = host.startsWith("*.") ? host.slice(2) : host
  if (!host || host.length > 253) return refuse("a host name has 1 to 253 characters")
  if (name.includes("*")) return refuse("only one leading `*.` is allowed")
  if (host.includes(":")) return refuse("give a name without a port; port 443 is allowed")
  if (!/^[a-z0-9.-]+$/.test(name)) return refuse("use lowercase letters, digits, hyphens and dots only")
  const labels = name.split(".")
  if (labels.some(label => !label || label.length > 63 || label.startsWith("-") || label.endsWith("-"))) {
    return refuse("each part between dots has 1 to 63 characters and does not start or end with a hyphen")
  }
  if (labels.length < 2) return refuse("a top-level name alone would allow a whole domain")
  const top = labels[labels.length - 1]!
  if (/^\d+$/.test(top)) return refuse("addresses are not allowed, only names")
  if (RESERVED_TOP_LEVEL.has(top)) return refuse("that name stays on this device or its network")
  if (host.startsWith("*.") && PUBLIC_SUFFIXES.has(name)) return refuse("a name under which anyone can get a host would allow all of them")
  return undefined
}

/** The code the engine gives a save refused because the policy changed since the person read it. */
const CHANGED_CODE = "changed"

/** Whether an error is the engine refusing a save because the policy changed while the person was looking at it. */
export function isStaleSave(error: unknown): boolean {
  return typeof error === "object" && error !== null && "code" in error && error.code === CHANGED_CODE
}

const STALE_MESSAGE = "The network policy of this computer changed while this was being decided. Read it again and retry."

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
  const [draft, setDraftState] = useState<NetworkPolicy>()
  const [baseline, setBaseline] = useState<NetworkPolicy>()
  const [loadError, setLoadError] = useState<string | null>(null)
  const [saveError, setSaveError] = useState<string | null>(null)
  const [isStale, setIsStale] = useState(false)
  const [saving, setSaving] = useState(false)
  const requests = useRef(0)
  // The draft and the policy it started from, readable between renders.
  const current = useRef<{ draft?: NetworkPolicy; baseline?: NetworkPolicy }>({})

  const setDraft = useCallback((next: NetworkPolicy) => {
    current.current.draft = next
    setDraftState(next)
  }, [])

  const rebase = useCallback((policy: NetworkPolicy) => {
    current.current = { draft: policy, baseline: policy }
    setBaseline(policy)
    setDraftState(policy)
    setIsStale(false)
  }, [])

  const adopt = useCallback((next: NetworkPolicyState, discard: boolean) => {
    const { draft: edited, baseline: started } = current.current
    const editing = Boolean(edited && started && !samePolicy(edited, started))
    setState(next)
    if (discard || !editing) rebase(next.policy)
    else if (started && !samePolicy(next.policy, started)) {
      setIsStale(true)
      setSaveError(STALE_MESSAGE)
    }
  }, [rebase])

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

  const submit = useCallback(async (policy: NetworkPolicy, options?: { acceptPresets?: boolean }) => {
    if (!save || !state) return
    setSaving(true)
    setSaveError(null)
    try {
      const { baseline } = current.current
      const next = await (options ? save(target, policy, baseline, options) : save(target, policy, baseline))
      requests.current += 1
      adopt(next, true)
    } catch (cause) {
      setIsStale(isStaleSave(cause))
      setSaveError(errorMessage(cause))
    } finally {
      setSaving(false)
    }
  }, [save, state, target, adopt])

  const dirty = Boolean(draft && baseline && !samePolicy(draft, baseline))
  return {
    state, draft, setDraft, dirty, isStale, saving, loadError, saveError,
    loading: enabled && !state && !loadError,
    submit,
    reload: (discard = false) => { setSaveError(null); setIsStale(false); return load(discard) },
    discard: () => { setSaveError(null); if (state) rebase(state.policy) },
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
