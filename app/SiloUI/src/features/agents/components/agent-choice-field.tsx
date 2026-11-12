import { useId } from "react"

import { fieldVariants } from "@/components/ui/field"
import { cn } from "@/lib/utils"

import { agentChoiceLabel, type AgentChoice } from "../model/agents"

const options: readonly AgentChoice[] = ["none", "claude-code", "codex"]

/** The agent a new computer starts with. It is installed once the computer runs, and is optional. */
export function AgentChoiceField({ value, onChange }: { value: AgentChoice; onChange: (choice: AgentChoice) => void }) {
  const id = useId()
  return <label htmlFor={id} className="grid min-w-0 gap-1 text-caption font-medium text-muted-foreground">
    Agent
    <select id={id} aria-label="Agent" className={cn(fieldVariants(), "text-foreground")} value={value} onChange={event => onChange(event.target.value as AgentChoice)}>
      {options.map(option => <option key={option} value={option}>{option === "none" ? "None" : agentChoiceLabel(option)}</option>)}
    </select>
    <span className="font-normal">Silo installs it when the computer is running. You sign in to it yourself.</span>
  </label>
}
