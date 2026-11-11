import { createContext, useContext, useEffect } from "react"

import { useSettingsSelector } from "@/features/preferences/settings-store"
import { showActionFailure } from "@/lib/operation-toast"

import type { ApplicationActions, ApplicationSource } from "@/features/application/model/application-source"
import { computerTarget } from "@/features/application/model/connections"

import { agentName, agentToInstall, type AgentChoice, type AgentId } from "./agents"
import { setComputerAgents } from "./use-computer-agents"

/** Whether the app can install an agent, so a create form offers the choice. */
export const AgentInstallSupport = createContext(false)

export function useAgentInstallSupport() {
  return useContext(AgentInstallSupport)
}

/** The agent remembered in Settings, "none" until one was chosen: what a create form preselects. */
export function useRememberedAgentChoice(): AgentChoice {
  return useSettingsSelector(view => view.settings.agent) ?? "none"
}

/**
 * Agents to install in computers that are being created, by computer name. A new computer
 * cannot take an agent until it is running (a macOS computer is set up first and stopped), so
 * the choice made in the create form waits here until the computer runs.
 */
const planned = new Map<string, AgentId>()

export function planAgentInstall(computerName: string, choice: AgentChoice | null | undefined) {
  const agent = agentToInstall(choice)
  if (agent) planned.set(computerName, agent)
  else planned.delete(computerName)
}

export function clearPlannedAgentInstalls() {
  planned.clear()
}

/** Installs each planned agent in its computer once the computer runs. */
export function usePlannedAgentInstalls(source: ApplicationSource | null | undefined, install: ApplicationActions["installAgent"]) {
  useEffect(() => {
    if (!install || planned.size === 0 || !source) return
    for (const computer of source.computers) {
      const name = computer.configuration.name
      const agent = planned.get(name)
      if (!agent || computer.device || computer.state !== "running" || computer.freshness === "stale" || computer.lifecycleAction) continue
      if (computer.macos && computer.macos.setupComplete === false) continue
      planned.delete(name)
      const target = computerTarget(computer)
      install(target, agent).then(
        agents => setComputerAgents(target, agents),
        (error: unknown) => showActionFailure(`Could not install ${agentName[agent]} in ${name}`, error, undefined, { native: false }),
      )
    }
  }, [source, install])
}
