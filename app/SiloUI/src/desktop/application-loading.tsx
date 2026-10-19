import { Monitor, Power } from "lucide-react"
import { useRef, type ReactNode } from "react"
import type { SetupComputerConfiguration } from "@/contracts/silo"
import { SiloMark } from "@/components/silo-mark"
import { Button } from "@/components/ui/button"
import { ListCard, ListRowIcon } from "@/components/list-row"
import { PageContainer } from "@/components/page"
import { desktopCommand } from "@/desktop/commands"
import { useStatusPanelSize } from "@/desktop/use-status-panel-size"
import { ApplicationShell } from "@/features/application/components/application-shell"
import { ApplicationCommandMenu } from "@/features/application/components/application-command-menu"
import { ComputerConfigurationList } from "@/features/computers/components/computer-configuration-list"
import { ComputerListItem, ComputerListRow } from "@/features/computers/components/computer-list"
import { useSettings } from "@/features/preferences/settings-store"
import { cn } from "@/lib/utils"

function Skeleton({ className }: { className: string }) {
  const { settings } = useSettings()
  return <span aria-hidden="true" className={cn("inline-block shrink-0 rounded bg-muted align-middle", !settings.reduceMotion && "animate-pulse motion-reduce:animate-none", className)} />
}
function LoadingControls() {
  return <span className="flex gap-0.5" aria-hidden="true">
    <Skeleton className="size-6" /><Skeleton className="size-6" /><Skeleton className="size-6" />
  </span>
}
const unavailable = () => {}

/**
 * The status panel's frame while it has no application state (loading or failed).
 * It sizes the native panel like the loaded view and keeps Open Silo and Quit usable.
 */
function StatusPanelFrame({ busy = false, children }: { busy?: boolean; children: ReactNode }) {
  const content = useRef<HTMLDivElement>(null)
  useStatusPanelSize(content)
  return <div ref={content} role="dialog" aria-label="Silo" aria-busy={busy || undefined} className="silo-window status-panel flex flex-col overflow-hidden rounded-xl border border-border bg-popover text-popover-foreground">
    <div className="status-panel-page flex shrink-0 flex-col overflow-hidden">
      {children}
      <footer className="flex shrink-0 items-center justify-between border-t px-2 py-2">
        <Button variant="ghost" size="sm" className="gap-2" onClick={() => { void desktopCommand("open_main") }}><SiloMark data-icon="inline-start" /><span>Open Silo</span></Button>
        <Button variant="ghost" size="icon-xs" aria-label="Quit Silo" onClick={() => { void desktopCommand("quit_app") }}><Power /></Button>
      </footer>
    </div>
  </div>
}

/** A panel-sized failure for the status window, which must never show the full-window error. */
export function StatusPanelUnavailable({ message, retry, checking = false }: { message: string; retry?: () => void; checking?: boolean }) {
  return <StatusPanelFrame>
    <div role="alert" className="grid justify-items-center gap-1.5 px-4 py-6 text-center">
      <p className="text-ui font-medium">Silo could not load</p>
      <p className="whitespace-pre-wrap text-caption text-muted-foreground select-text">{message}</p>
      {retry && <Button type="button" variant="outline" size="xs" className="mt-1" disabled={checking} onClick={retry}>{checking ? "Checking…" : "Retry"}</Button>}
    </div>
  </StatusPanelFrame>
}

export function ApplicationLoading({ configurations, statusPanel = false }: { configurations: SetupComputerConfiguration[]; statusPanel?: boolean }) {
  const { settings } = useSettings()
  const detail = <span className="flex h-4 items-center"><Skeleton className="h-2.5 w-20" /></span>
  if (statusPanel) return <StatusPanelFrame busy>
    <span role="status" className="sr-only">Loading computer state</span>
    <div className="shrink-0 px-2 pt-2" />
    <div className="min-h-0 overflow-y-auto overscroll-contain px-2 pb-2">{configurations.length ? <ListCard className="border-0"><ol aria-label="Computers" className="divide-y">
      {configurations.map((configuration) => <ComputerListItem key={configuration.id}><ComputerListRow name={configuration.name} detail={detail} actions={<LoadingControls />} /></ComputerListItem>)}
    </ol></ListCard> : <div className="grid justify-items-center gap-1.5 py-8 text-center"><ListRowIcon><Monitor className="size-3.5" /></ListRowIcon><p className="text-ui font-medium">Loading computers…</p></div>}</div>
  </StatusPanelFrame>
  return <ApplicationShell activeTab="computers" computerSection="overview" settingsSection="general"
    systemIssueStatus={null} computerAttention={{ errors: 0, warnings: 0 }} navigationDisabled
    onTabChange={unavailable} onComputerSectionChange={unavailable} onSettingsSectionChange={unavailable}
    canGoBack={false} canGoForward={false} onGoBack={unavailable} onGoForward={unavailable}
    reduceMotion={settings.reduceMotion} commandMenu={<ApplicationCommandMenu commands={[]} disabled />}>
    <PageContainer aria-busy="true" className="flex h-full min-h-0 flex-col">
      <span role="status" className="sr-only">Loading computer state</span>
      <div className="min-h-0 flex-1">
        <ComputerConfigurationList configurations={configurations} onConfigurationsChange={unavailable} interactionDisabled
          getRowPresentation={() => ({ detail, actions: <LoadingControls />, busy: true })} />
      </div>
    </PageContainer>
  </ApplicationShell>
}
