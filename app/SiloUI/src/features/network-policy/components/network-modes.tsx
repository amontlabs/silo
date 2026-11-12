import { ChevronRight, ShieldCheck } from "lucide-react"
import { useEffect, useMemo, useState } from "react"

import { ListCard, ListRow, ListRowIcon } from "@/components/list-row"
import { SectionHeading } from "@/components/page"
import { ComputerBadge } from "@/features/application/components/application-ui"
import type { ApplicationActions, ApplicationComputer } from "@/features/application/model/application-source"
import { computerTarget } from "@/features/application/model/connections"
import type { NetworkMode } from "@/desktop/production-schemas"

import { modeChoices, modeLabel } from "../model/network-policy"

type Modes = ReadonlyMap<string, NetworkMode | "unknown">

/** The network mode of each computer of this device, read while `active`. A computer whose policy cannot be read shows as unknown. */
function useNetworkModes(computers: readonly ApplicationComputer[], read: ApplicationActions["readNetworkPolicy"], active: boolean): Modes {
  const [modes, setModes] = useState<Modes>(new Map())
  const targets = computers.filter(computer => !computer.device).map(computerTarget)
  const key = targets.join("\n")
  useEffect(() => {
    if (!read || !active || !key) return
    let current = true
    const wanted = key.split("\n")
    Promise.all(wanted.map(target => read(target).then(state => [target, state.policy.mode] as const, () => [target, "unknown"] as const)))
      .then(entries => { if (current) setModes(new Map(entries)) })
    return () => { current = false }
  }, [read, active, key])
  return useMemo(() => modes, [modes])
}

/** Each computer's network mode, with a way to its Network section; the table below it is the computers' published ports. */
export function NetworkModes({ computers, read, active, onOpenComputer }: {
  computers: readonly ApplicationComputer[]
  read: ApplicationActions["readNetworkPolicy"]
  active: boolean
  onOpenComputer?: (computerId: string) => void
}) {
  const modes = useNetworkModes(computers, read, active)
  const local = computers.filter(computer => !computer.device)
  if (!read || local.length === 0) return null
  return <section className="grid gap-1.5" aria-label="Network access">
    <SectionHeading>Network access</SectionHeading>
    <ListCard divided>
      {local.map(computer => {
        const mode = modes.get(computerTarget(computer))
        const description = mode && mode !== "unknown" ? modeChoices.find(choice => choice.mode === mode)?.description : undefined
        return <ListRow
          key={computerTarget(computer)}
          icon={<ListRowIcon aria-hidden="true"><ShieldCheck className="size-3.5" /></ListRowIcon>}
          title={<ComputerBadge name={computer.configuration.name} state={computer.state} device={computer.device} />}
          detail={mode === undefined ? "Reading…" : mode === "unknown" ? "Could not read the network policy." : `${modeLabel(mode)} · ${description}`}
          detailClassName="whitespace-normal"
          actions={onOpenComputer && <button type="button" aria-label={`Open the network of ${computer.configuration.name}`} onClick={() => onOpenComputer(computer.configuration.id)}
            className="inline-flex shrink-0 items-center gap-0.5 rounded-sm text-caption text-muted-foreground hover:text-foreground focus-ring">Network section<ChevronRight className="size-3" aria-hidden="true" /></button>}
        />
      })}
    </ListCard>
  </section>
}
