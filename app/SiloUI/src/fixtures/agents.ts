import type { AgentId, AgentInfo, AgentInstallProgress } from "@/desktop/production-schemas"
import { computerTarget } from "@/features/application/model/connections"
import type { ApplicationActions, ApplicationComputer } from "@/features/application/model/application-source"

/**
 * What the agents of a computer look like in the preview. `mixed`: Claude Code is ready and
 * Codex is not installed. The other modes put both agents in one state; `install-fails` lists
 * both as missing and every install fails after it starts.
 */
export const agentFixtureModes = ["mixed", "ready", "missing", "outdated", "installing", "unknown", "install-fails"] as const
export type AgentFixtureMode = (typeof agentFixtureModes)[number]

export function agentFixtureModeFromSearch(search: string): AgentFixtureMode | undefined {
  const requested = new URLSearchParams(search).get("agents")
  return agentFixtureModes.find(mode => mode === requested)
}

const versions: Record<AgentId, string> = { "claude-code": "2.1.287", codex: "0.167.0" }
const names: Record<AgentId, string> = { "claude-code": "Claude Code", codex: "Codex" }
const ids: AgentId[] = ["claude-code", "codex"]

const initialState = (mode: AgentFixtureMode, agent: AgentId): AgentInfo["state"] => {
  switch (mode) {
    case "mixed": return agent === "claude-code" ? "ready" : "missing"
    case "ready": return "ready"
    case "missing": case "install-fails": return "missing"
    case "outdated": return agent === "claude-code" ? "outdated" : "ready"
    case "installing": return agent === "codex" ? "installing" : "ready"
    case "unknown": return "unknown"
  }
}

export type AgentFixtureActions = Required<Pick<ApplicationActions, "listAgents" | "installAgent" | "ensureSshName">>

/** Agents that answer from memory. A stopped computer's agents are unknown; an install walks through its steps and changes the next list. */
export function createAgentFixture({ mode = "mixed", stepMs = 350 }: { mode?: AgentFixtureMode; stepMs?: number } = {}): AgentFixtureActions & { installs: () => string[]; setComputers: (computers: readonly ApplicationComputer[]) => void } {
  // Every computer runs until the preview says which computers there are.
  let computers: readonly ApplicationComputer[] | undefined
  const isRunning = (target: string) => !computers || computers.some(computer => computerTarget(computer) === target && computer.state === "running")
  const installed = new Map<string, Partial<Record<AgentId, AgentInfo["state"]>>>()
  const started: string[] = []
  const list = (target: string): AgentInfo[] => ids.map(id => {
    const state = isRunning(target) ? installed.get(target)?.[id] ?? initialState(mode, id) : "unknown"
    const version = state === "ready" ? versions[id] : state === "outdated" ? "2.0.0" : undefined
    return { id, name: names[id], state, version, expectedVersion: versions[id] }
  })
  const wait = () => new Promise<void>(resolve => setTimeout(resolve, stepMs))
  return {
    installs: () => started,
    setComputers: next => { computers = next },
    listAgents: async target => list(target),
    installAgent: async (target, agent, onProgress) => {
      started.push(`${target}:${agent}`)
      const progress = (step: AgentInstallProgress["step"], detail?: string) => onProgress?.({ computerId: target, agent, step, detail })
      progress("waiting")
      await wait()
      progress("downloading", `Downloading ${names[agent]} (42%)`)
      await wait()
      if (mode === "install-fails") throw new Error("The download could not be verified. Check the network and try again.")
      progress("installing")
      await wait()
      progress("registering")
      await wait()
      installed.set(target, { ...installed.get(target), [agent]: "ready" })
      return list(target)
    },
    ensureSshName: async target => ({ alias: `silo-${target.replace(/[^a-z0-9]+/gi, "-").toLowerCase()}`, includeInstalled: true, includeLine: "" }),
  }
}
