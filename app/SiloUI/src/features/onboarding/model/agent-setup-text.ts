import { agentName, installStepText } from "@/features/agents/model/agents"

import type { AgentSetupView } from "./onboarding-state"

/** What the install of the chosen agent is doing, in one plain line. */
export function agentSetupText(setup: AgentSetupView): string {
  const name = agentName[setup.agent]
  switch (setup.status) {
    case "queued": return `Waiting to install ${name} in ${setup.computer}.`
    case "running": return `Installing ${name} in ${setup.computer}: ${setup.detail ?? installStepText[setup.step ?? "waiting"]}`
    case "succeeded": return `${name} is ready in ${setup.computer}.`
    case "failed": return `${name} could not be installed in ${setup.computer}${setup.failure ? `: ${setup.failure}` : "."}`
  }
}
