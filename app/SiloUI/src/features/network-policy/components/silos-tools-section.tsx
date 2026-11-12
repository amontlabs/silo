import { TriangleAlert, Wrench } from "lucide-react"
import type { ReactNode } from "react"

import { InlineAlert } from "@/components/inline-alert"
import { ListCard, ListRow, ListRowIcon, ListRowSkeleton } from "@/components/list-row"
import { SectionHeading } from "@/components/page"
import { Button } from "@/components/ui/button"
import { Switch } from "@/components/ui/switch"
import type { ApplicationActions } from "@/features/application/model/application-source"

import { componentProblem, useComputerTools } from "../model/network-policy"

/** When the tools' network rule takes effect; empty when they are off or already in force. */
function whenText(tools: { enabled: boolean; network?: string; restartNeeded: boolean }, running: boolean): { text: string; needsRestart: boolean } | undefined {
  if (!tools.enabled) return undefined
  if (tools.restartNeeded || (tools.network === "nextRestart" && running)) return { text: "Restart needed", needsRestart: true }
  if (tools.network === "nextStart" || tools.network === "nextRestart") return { text: "Takes effect at the next start", needsRestart: false }
  return undefined
}

/** Whether the agent inside a computer can use Silo's tools: the `silo` command and its MCP server. */
export function SilosToolsSection({ target, computerName, running, readOnly, enabled, refreshKey, actions, restart }: {
  target: string
  computerName: string
  running: boolean
  readOnly: boolean
  enabled: boolean
  refreshKey?: string
  actions: Pick<ApplicationActions, "readComputerTools" | "setComputerTools">
  restart?: ReactNode
}) {
  const { tools, error, busy, loading, change, reload } = useComputerTools({ read: actions.readComputerTools, set: actions.setComputerTools, target, enabled, refreshKey })
  const when = tools ? whenText(tools, running) : undefined
  const problems = tools?.enabled ? tools.components.flatMap(component => { const text = componentProblem(component); return text ? [text] : [] }) : []
  return <section className="grid gap-1.5" aria-label="Silo's tools">
    <div className="flex min-h-6 items-center justify-between gap-2"><SectionHeading>Silo's tools</SectionHeading></div>
    {error && <InlineAlert className="flex items-center justify-between gap-3">
      <span>{error}</span>
      <Button type="button" size="sm" variant="ghost" onClick={() => void reload()}>Retry</Button>
    </InlineAlert>}
    <ListCard>
      {!tools && !error && <ListRowSkeleton label={loading ? "Loading Silo's tools" : "Silo's tools"} />}
      {tools && <ListRow
        icon={<ListRowIcon aria-hidden="true"><Wrench className="size-3.5" /></ListRowIcon>}
        title={tools.enabled ? "On" : "Off"}
        detail={<>
          When on, the agent inside this computer can use the silo command and its MCP server.
          {when && <> <span className={when.needsRestart ? "text-warning" : undefined}>{when.text}.</span></>}
        </>}
        detailClassName="whitespace-normal"
        actions={<div className="flex shrink-0 items-center gap-2">
          {when?.needsRestart && restart}
          <Switch checked={tools.enabled} disabled={readOnly || busy} aria-label={`Silo's tools in ${computerName}`} onCheckedChange={on => void change(on)} />
        </div>}
      />}
      {problems.length > 0 && <ul role="status" aria-label="Problems with Silo's tools" className="grid gap-1 border-t border-border px-3 py-2 text-xs">
        {problems.map(problem => <li key={problem} className="flex items-start gap-1.5 text-warning"><TriangleAlert className="mt-0.5 size-3.5 shrink-0" aria-hidden="true" />{problem}</li>)}
      </ul>}
    </ListCard>
  </section>
}
