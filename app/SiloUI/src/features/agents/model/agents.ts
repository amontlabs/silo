import type { AgentId, AgentInfo, AgentInstallStep, AgentState } from "@/desktop/production-schemas"

export type { AgentId, AgentInfo, AgentInstallProgress, AgentInstallStep, AgentState, SshName } from "@/desktop/production-schemas"

/** What the person picked in onboarding and the create form: an agent, or setting it up themselves. */
export type AgentChoice = AgentId | "none"

export const agentChoices: readonly AgentChoice[] = ["claude-code", "codex", "none"]

export const agentName: Record<AgentId, string> = { "claude-code": "Claude Code", codex: "Codex" }

/** One plain sentence each, for the choice in onboarding and the create form. */
export const agentSentence: Record<AgentChoice, string> = {
  "claude-code": "Anthropic's coding agent, installed in your computer and ready to open.",
  codex: "OpenAI's coding agent, installed in your computer and ready to open.",
  none: "Silo installs nothing. You set up the agent you use yourself.",
}

export function agentChoiceLabel(choice: AgentChoice): string {
  return choice === "none" ? "I'll set it up myself" : agentName[choice]
}

/** The agent to install for a remembered choice; none for "set it up myself" and for no choice yet. */
export function agentToInstall(choice: AgentChoice | null | undefined): AgentId | null {
  return choice && choice !== "none" ? choice : null
}

/** Agents that can be started: installed, whether or not they are the version this Silo ships. */
export function openableAgents(agents: readonly AgentInfo[] | undefined): AgentInfo[] {
  return (agents ?? []).filter(({ state }) => state === "ready" || state === "outdated")
}

/** The agent a single "open the agent" action starts: the remembered one if installed, else the first installed. */
export function preferredAgent(agents: readonly AgentInfo[] | undefined, remembered: AgentChoice | null | undefined): AgentInfo | undefined {
  const openable = openableAgents(agents)
  return openable.find(({ id }) => id === remembered) ?? openable[0]
}

export function agentActionLabel(state: AgentState): "Install" | "Update" | null {
  return state === "missing" ? "Install" : state === "outdated" ? "Update" : null
}

export function agentStateText(agent: AgentInfo): string {
  switch (agent.state) {
    case "ready": return agent.version ? `Ready · ${agent.version}` : "Ready"
    case "outdated": return agent.version ? `Update available · ${agent.version} installed, ${agent.expectedVersion} ready to install` : "Update available"
    case "missing": return "Not installed"
    case "installing": return "Installing…"
    case "unknown": return "Start the computer to see this"
  }
}

/** Where an install is, in a few plain words. */
export const installStepText: Record<AgentInstallStep, string> = {
  waiting: "Waiting to start…",
  downloading: "Downloading…",
  installing: "Installing…",
  registering: "Setting up Silo's tools…",
  done: "Finishing…",
}

/** Shown in the label of every open-the-agent action. */
export function openAgentLabel(agent: AgentId, computerName: string): string {
  return `Open ${agentName[agent]} in ${computerName}`
}
