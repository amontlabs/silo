import { useId } from "react"

import { fieldVariants } from "@/components/ui/field"
import { cn } from "@/lib/utils"

import { modeChoices } from "../model/network-policy"
import type { NetworkChoice } from "../model/planned-network"

/** The network a new computer starts with. Optional: the default is what every new computer gets. */
export function NetworkModeField({ value, onChange }: { value: NetworkChoice; onChange: (choice: NetworkChoice) => void }) {
  const id = useId()
  const described = modeChoices.find(choice => choice.mode === value)?.description
  return <label htmlFor={id} className="grid min-w-0 gap-1 text-caption font-medium text-muted-foreground">
    Network
    <select id={id} aria-label="Network" className={cn(fieldVariants(), "text-foreground")} value={value} onChange={event => onChange(event.target.value as NetworkChoice)}>
      <option value="default">Default</option>
      {modeChoices.map(choice => <option key={choice.mode} value={choice.mode}>{choice.label}</option>)}
    </select>
    <span className="font-normal">{described ?? "New computers get the default network."} You can change it later on the computer's page.</span>
  </label>
}
