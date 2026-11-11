import { Bot, Check, CircleAlert, RotateCw, Wrench } from "lucide-react"
import { useId } from "react"

import { ListCard, ListRow, ListRowIcon } from "@/components/list-row"
import { Button } from "@/components/ui/button"
import { Spinner } from "@/components/ui/spinner"
import { agentChoiceLabel, agentChoices, agentName, agentSentence, type AgentChoice } from "@/features/agents/model/agents"
import type { AgentSetupView } from "@/features/onboarding/model/onboarding-state"

const stepText = {
  waiting: "Waiting to start…",
  downloading: "Downloading…",
  installing: "Installing…",
  registering: "Setting up Silo's tools…",
  done: "Finishing…",
} as const

/** What the install of the chosen agent is doing, in one plain line. */
export function agentSetupText(setup: AgentSetupView): string {
  const name = agentName[setup.agent]
  switch (setup.status) {
    case "queued": return `Waiting to install ${name} in ${setup.computer}.`
    case "running": return `Installing ${name} in ${setup.computer}: ${setup.detail ?? stepText[setup.step ?? "waiting"]}`
    case "succeeded": return `${name} is ready in ${setup.computer}.`
    case "failed": return `${name} could not be installed in ${setup.computer}${setup.failure ? `: ${setup.failure}` : "."}`
  }
}

/**
 * The agent the person uses. Silo installs it in the first computer when they continue, shows
 * the install's progress, and a failure leaves setup finishable with a Retry.
 */
export function AgentStep({ choice, onChoose, computerName, setup, onRetry }: {
  choice: AgentChoice | null
  onChoose: (choice: AgentChoice) => void
  computerName: string
  setup: AgentSetupView | null
  onRetry?: () => void
}) {
  const group = useId()
  return <section aria-labelledby="agent-title" className="grid gap-4">
    <h2 id="agent-title" className="sr-only" data-visual-heading="hidden">Your agent</h2>
    <p className="text-xs text-muted-foreground">Pick the agent you use. Silo installs it in {computerName} and opens it there with one action. You sign in to it yourself.</p>
    <ListCard divided role="radiogroup" aria-label="Your agent">
      {agentChoices.map(option => {
        const id = `${group}-${option}`
        return <ListRow
          key={option}
          className="row-hover cursor-pointer"
          icon={<ListRowIcon aria-hidden="true">{option === "none" ? <Wrench className="size-3.5" /> : <Bot className="size-3.5" />}</ListRowIcon>}
          title={<label htmlFor={id} className="cursor-pointer">{agentChoiceLabel(option)}</label>}
          detail={agentSentence[option]}
          detailClassName="whitespace-normal"
          onOpen={() => onChoose(option)}
          actions={<input id={id} type="radio" name={group} value={option} checked={choice === option} onChange={() => onChoose(option)} className="size-3.5 accent-primary" />}
        />
      })}
    </ListCard>
    {setup && <ListCard role={setup.status === "failed" ? "alert" : "status"} aria-live="polite" aria-label="Agent install">
      <ListRow
        icon={<ListRowIcon aria-hidden="true">{setup.status === "running" ? <Spinner /> : setup.status === "succeeded" ? <Check className="size-3.5 text-success" /> : setup.status === "failed" ? <CircleAlert className="size-3.5 text-destructive" /> : <Bot className="size-3.5" />}</ListRowIcon>}
        title={agentSetupText(setup)}
        detail={setup.status === "failed" ? "You can still finish setup and install it later from the computer's page." : undefined}
        detailClassName="whitespace-normal"
        actions={setup.status === "failed" && onRetry ? <Button type="button" variant="outline" size="xs" onClick={onRetry}><RotateCw aria-hidden="true" data-icon="inline-start" />Retry</Button> : undefined}
      />
    </ListCard>}
  </section>
}
