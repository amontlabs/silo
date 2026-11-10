import { agentInstallProgressSchema, agentsListSchema, sshNameSchema } from "./production-schemas"
import type { AgentId, AgentInfo, AgentInstallProgress, SshName } from "./production-schemas"
import type { ProductionBridge } from "./production-types"

export const AGENT_INSTALL_PROGRESS_EVENT = "silo://agent-install-progress"

/** The agents of a computer, from `agents.list`. */
export async function listAgents(native: ProductionBridge, computer: string): Promise<AgentInfo[]> {
  return agentsListSchema.parse(await native.invoke("list_agents", { computer })).agents
}

/**
 * Installs an agent and resolves with the computer's agents. `computerId` is the engine's id
 * for the computer: progress events carry it, and only this install's are passed on.
 */
export async function installAgent(
  native: ProductionBridge,
  computer: string,
  computerId: string,
  agent: AgentId,
  onProgress?: (progress: AgentInstallProgress) => void,
): Promise<AgentInfo[]> {
  const stop = onProgress
    ? await native.listen(AGENT_INSTALL_PROGRESS_EVENT, event => {
      const parsed = agentInstallProgressSchema.safeParse(event?.payload)
      if (parsed.success && parsed.data.computerId === computerId && parsed.data.agent === agent) onProgress(parsed.data)
    }).catch(() => undefined)
    : undefined
  try {
    return agentsListSchema.parse(await native.invoke("install_agent", { computer, agent })).agents
  } finally {
    stop?.()
  }
}

/** The SSH host name for a computer, from `guest.sshAlias.ensure`. */
export async function ensureSshName(native: ProductionBridge, computer: string): Promise<SshName> {
  return sshNameSchema.parse(await native.invoke("ensure_ssh_name", { computer }))
}
