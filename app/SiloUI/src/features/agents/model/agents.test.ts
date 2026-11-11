import { describe, expect, it } from "vitest"

import contracts from "@/test/contracts/agents.json"
import { agentsListSchema } from "@/desktop/production-schemas"

import { agentActionLabel, agentChoiceLabel, agentStateText, agentToInstall, openableAgents, openAgentLabel, preferredAgent, type AgentInfo } from "./agents"

const claude = (state: AgentInfo["state"], version?: string): AgentInfo => ({ id: "claude-code", name: "Claude Code", state, version, expectedVersion: "2.1.287" })
const codex = (state: AgentInfo["state"], version?: string): AgentInfo => ({ id: "codex", name: "Codex", state, version, expectedVersion: "0.167.0" })

describe("agents", () => {
  it("installs only a chosen agent, never for 'set it up myself' or before a choice", () => {
    expect(agentToInstall("claude-code")).toBe("claude-code")
    expect(agentToInstall("codex")).toBe("codex")
    expect(agentToInstall("none")).toBeNull()
    expect(agentToInstall(null)).toBeNull()
    expect(agentChoiceLabel("none")).toBe("I'll set it up myself")
  })

  it("opens agents that are installed, whether or not they match this Silo's version", () => {
    const agents = [claude("outdated", "2.0.0"), codex("missing")]
    expect(openableAgents(agents).map(({ id }) => id)).toEqual(["claude-code"])
    expect(openableAgents([claude("installing"), codex("unknown")])).toEqual([])
    expect(openableAgents(undefined)).toEqual([])
  })

  it("prefers the remembered agent when it is installed, else the first installed one", () => {
    const both = [claude("ready", "2.1.287"), codex("ready", "0.167.0")]
    expect(preferredAgent(both, "codex")?.id).toBe("codex")
    expect(preferredAgent(both, null)?.id).toBe("claude-code")
    expect(preferredAgent([claude("ready"), codex("missing")], "codex")?.id).toBe("claude-code")
    expect(preferredAgent([claude("missing"), codex("missing")], "codex")).toBeUndefined()
  })

  it("says each state plainly and offers Install or Update only where one applies", () => {
    expect(agentStateText(claude("ready", "2.1.287"))).toBe("Ready · 2.1.287")
    expect(agentStateText(claude("outdated", "2.0.0"))).toContain("Update available")
    expect(agentStateText(codex("missing"))).toBe("Not installed")
    expect(agentStateText(codex("installing"))).toBe("Installing…")
    expect(agentStateText(codex("unknown"))).toBe("Start the computer to see this")
    expect(agentActionLabel("missing")).toBe("Install")
    expect(agentActionLabel("outdated")).toBe("Update")
    expect(["ready", "installing", "unknown"].map(state => agentActionLabel(state as AgentInfo["state"]))).toEqual([null, null, null])
    expect(openAgentLabel("claude-code", "main")).toBe("Open Claude Code in main")
  })

  it("reads the wire contract and drops an agent it does not know", () => {
    expect(agentsListSchema.parse(contracts.list).agents.map(({ id, state }) => [id, state])).toEqual([["claude-code", "ready"], ["codex", "missing"]])
    const newer = agentsListSchema.parse({ agents: [...contracts.list.agents, { id: "pi", name: "Pi", state: "ready", version: "1", expectedVersion: "1" }, { id: "codex", name: "Codex", state: "brand-new" }] })
    expect(newer.agents.map(({ id, state }) => [id, state])).toEqual([["claude-code", "ready"], ["codex", "missing"], ["codex", "unknown"]])
  })
})
