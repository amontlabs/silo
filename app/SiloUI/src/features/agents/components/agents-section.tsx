import { useState } from "react"
import { Bot, RotateCw } from "lucide-react"

import { SectionHeading } from "@/components/page"
import { InlineAlert } from "@/components/inline-alert"
import { ListCard, ListRow, ListRowIcon, ListRowSkeleton } from "@/components/list-row"
import { Button } from "@/components/ui/button"
import { Spinner } from "@/components/ui/spinner"
import { DisabledReason } from "@/features/application/components/disabled-reason"
import type { ApplicationActions } from "@/features/application/model/application-source"
import { errorMessage } from "@/lib/error-message"

import { agentActionLabel, agentName, agentStateText, installStepText, type AgentId, type AgentInfo, type AgentInstallProgress } from "../model/agents"
import { setComputerAgents } from "../model/use-computer-agents"
import { SshNameLine } from "./ssh-name-line"

type Install = { status: "running"; progress?: AgentInstallProgress } | { status: "failed"; message: string }

/**
 * The agents of one computer on its page: each with its state, an Install or Update button,
 * the install's progress while it runs and a plain error when it fails.
 */
export function AgentsSection({ target, computerName, running, offline = false, agents, error, loading, onReload, actions }: {
  target: string
  computerName: string
  /** Whether the computer is running: installing needs it, and a stopped computer's agents are not known. */
  running: boolean
  offline?: boolean
  agents: readonly AgentInfo[] | undefined
  error: string | null
  loading: boolean
  onReload: () => void
  actions: Pick<ApplicationActions, "installAgent" | "ensureSshName">
}) {
  const [installs, setInstalls] = useState<Partial<Record<AgentId, Install>>>({})
  const busy = Object.values(installs).some(install => install?.status === "running")
  const { installAgent, ensureSshName } = actions

  async function install(agent: AgentId) {
    if (!installAgent) return
    setInstalls(current => ({ ...current, [agent]: { status: "running" } }))
    try {
      const result = await installAgent(target, agent, progress => setInstalls(current => current[agent]?.status === "running" ? { ...current, [agent]: { status: "running", progress } } : current))
      setComputerAgents(target, result)
      setInstalls(({ [agent]: _done, ...rest }) => rest)
    } catch (cause) {
      setInstalls(current => ({ ...current, [agent]: { status: "failed", message: errorMessage(cause) } }))
    }
  }

  const reason = offline ? "The device is offline." : running ? undefined : "Start the computer to install an agent."
  return <section className="grid gap-1.5" aria-label="Agents">
    <div className="flex min-h-6 items-center justify-between gap-2"><SectionHeading>Agents</SectionHeading></div>
    {error && <InlineAlert className="flex items-center justify-between gap-3">
      <span>{error}</span>
      <Button type="button" size="sm" variant="ghost" onClick={onReload}>Retry</Button>
    </InlineAlert>}
    <ListCard divided>
      {!agents && !error && <ListRowSkeleton label={loading ? "Loading agents" : "Agents"} />}
      {agents?.map(agent => {
        const current = installs[agent.id]
        const installing = current?.status === "running" || agent.state === "installing"
        const action = agentActionLabel(agent.state)
        const detail = current?.status === "running" ? (current.progress?.detail ?? installStepText[current.progress?.step ?? "waiting"])
          : current?.status === "failed" ? `Could not install ${agentName[agent.id]}: ${current.message}`
            : agentStateText(agent)
        const label = current?.status === "failed" ? "Try again" : action
        return <ListRow
          key={agent.id}
          role="group"
          aria-label={agentName[agent.id]}
          icon={<ListRowIcon aria-hidden="true">{installing ? <Spinner /> : <Bot className="size-3.5" />}</ListRowIcon>}
          title={agentName[agent.id]}
          detail={<span role={current?.status === "failed" ? "alert" : installing ? "status" : undefined}>{detail}</span>}
          detailClassName={current?.status === "failed" ? "whitespace-normal break-words text-destructive" : "whitespace-normal"}
          actions={!installing && label && installAgent
            ? <DisabledReason reason={reason}>
              <Button type="button" variant="outline" size="xs" aria-label={`${label === "Try again" ? "Install" : label} ${agentName[agent.id]} in ${computerName}`} disabled={Boolean(reason) || busy} onClick={() => void install(agent.id)}>
                {label === "Try again" ? <RotateCw aria-hidden="true" data-icon="inline-start" /> : null}{label}
              </Button>
            </DisabledReason>
            : undefined}
        />
      })}
      {ensureSshName && <SshNameLine key={target} target={target} running={running && !offline} ensure={ensureSshName} />}
    </ListCard>
  </section>
}
