import { useCallback, useEffect, useRef, useState, useSyncExternalStore } from "react"

import type { ApplicationActions, ApplicationSource } from "@/features/application/model/application-source"
import { computerTarget } from "@/features/application/model/connections"
import { computerCan, kindOf } from "@/features/computers/model/computer-capabilities"
import { errorMessage } from "@/lib/error-message"

import type { AgentInfo } from "./agents"

/** How long an answer is trusted before a window asks again. */
const FRESH_MS = 5 * 60_000

type ListAgents = NonNullable<ApplicationActions["listAgents"]>

const cache = new Map<string, { agents: AgentInfo[]; at: number }>()
/** Computers being asked by a running-computers sweep, so two windows' sweeps ask once. */
const asking = new Set<string>()
const listeners = new Set<() => void>()
let version = 0

function publish() {
  version += 1
  for (const listener of listeners) listener()
}

function subscribe(listener: () => void) {
  listeners.add(listener)
  return () => { listeners.delete(listener) }
}

/** Records what a computer's agents are, for every page and menu that shows them. */
export function setComputerAgents(target: string, agents: AgentInfo[]) {
  cache.set(target, { agents, at: Date.now() })
  publish()
}

/** Forgets what is known about a computer's agents (for tests, and when a computer goes away). */
export function clearComputerAgents() {
  cache.clear()
  asking.clear()
  publish()
}

function useCacheVersion() {
  return useSyncExternalStore(subscribe, () => version, () => version)
}

/** The agents of one computer: the last answer at once, refreshed when `enabled` turns on or `refreshKey` changes (a computer starting or stopping). */
export function useComputerAgents(listAgents: ListAgents | undefined, target: string, enabled: boolean, refreshKey?: string) {
  useCacheVersion()
  const [error, setError] = useState<string | null>(null)
  const requests = useRef(0)
  const load = useCallback(async () => {
    if (!listAgents) return
    const request = ++requests.current
    try {
      const agents = await listAgents(target)
      if (request !== requests.current) return
      setComputerAgents(target, agents)
      setError(null)
    } catch (cause) {
      if (request === requests.current) setError(errorMessage(cause))
    }
  }, [listAgents, target])
  useEffect(() => {
    if (!enabled) return
    // oxlint-disable-next-line react/set-state-in-effect
    void load()
    return () => { requests.current += 1 }
  }, [enabled, load, refreshKey])
  const agents = cache.get(target)?.agents
  return { agents, error, loading: enabled && !agents && !error, reload: load }
}

/**
 * The agents of every running computer that can open a terminal, for the palette and the menu
 * bar panel. A computer is asked once while its answer is fresh, and again when it starts.
 */
export function useRunningComputerAgents(source: ApplicationSource | null | undefined, listAgents: ListAgents | undefined) {
  const revision = useCacheVersion()
  const running = (source?.computers ?? [])
    .filter(computer => computer.state === "running" && computer.freshness !== "stale" && computerCan(kindOf(computer), "terminal") && !(computer.device && computer.device.connected === false))
    .map(computer => computerTarget(computer))
  const key = running.join("\n")
  useEffect(() => {
    if (!listAgents) return
    let cancelled = false
    for (const target of key ? key.split("\n") : []) {
      const known = cache.get(target)
      if ((known && Date.now() - known.at < FRESH_MS) || asking.has(target)) continue
      asking.add(target)
      listAgents(target).then(
        agents => { if (!cancelled) setComputerAgents(target, agents) },
        () => { /* An unreadable answer leaves the action out; the computer page shows the reason. */ },
      ).finally(() => asking.delete(target))
    }
    return () => { cancelled = true }
  }, [key, listAgents])
  // A new function whenever any answer changes, so a memo that depends on it recomputes.
  // oxlint-disable-next-line react-hooks/exhaustive-deps
  const agentsOf = useCallback((target: string): AgentInfo[] | undefined => cache.get(target)?.agents, [revision])
  return { agentsOf }
}
