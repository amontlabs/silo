import { formatDateTime } from '@/lib/format-date'
import { useEffect, useEffectEvent, useId, useLayoutEffect, useRef, useState, type ReactNode } from 'react'
import { formatBinaryBytes as formatBytes } from '@/lib/format-bytes'
import type { WorkspaceStorageState } from '../model/workspace-storage'
import { HardDrive, Database, Folder, Gauge, RefreshCw, History, ChevronDown, Check, CircleAlert, Layers, type LucideIcon } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { errorMessage } from '@/lib/error-message'
import { dismissOperationToast, showOperationFailure, showOperationProgress, showOperationSuccess } from '@/lib/operation-toast'
import { DisabledReason } from '../components/disabled-reason'
import { Tooltip, TooltipContent, TooltipTrigger, TooltipProvider } from '@/components/ui/tooltip'

type ReclaimEntry = WorkspaceStorageState['history'][number]

function date(at: number) { return formatDateTime(at * 1000) }
function ago(at: number) {
  const seconds = Math.max(0, Date.now() / 1000 - at)
  const units: [Intl.RelativeTimeFormatUnit, number][] = [['day', 86400], ['hour', 3600], ['minute', 60]]
  const [unit, size] = units.find(([, size]) => seconds >= size) ?? ['minute', 60]
  return seconds < 60 ? 'just now' : new Intl.RelativeTimeFormat('en', { numeric: 'auto' }).format(-Math.floor(seconds / size), unit)
}
const reclaimTriggerLabels = new Map([
  ['manual', 'Manual'], ['scheduled', 'Scheduled'], ['beforeStop', 'Before stop'],
  ['afterStart', 'After start'], ['legacy', 'Earlier free-up'],
])
function trigger(value: string) { return reclaimTriggerLabels.get(value) ?? 'Automatic' }

interface StoragePanelProps {
  computerId: string
  /** Names the computer in the system notification. */
  computerName?: string
  running: boolean
  /** The device that owns the computer; omitted for a computer on this device. */
  deviceName?: string
  disabled?: boolean
  read: (computerId: string) => Promise<WorkspaceStorageState>
  reclaim?: (computerId: string) => Promise<WorkspaceStorageState>
}

export function WorkspaceStoragePanel(props: StoragePanelProps) {
  return <WorkspaceStorageContent key={`${props.computerId}:${props.running}`} {...props} />
}

function WorkspaceStorageContent({ computerId, computerName, running, deviceName, disabled = false, read, reclaim }: StoragePanelProps) {
  const location = deviceName ?? 'This device'
  const where = deviceName ? `on ${deviceName}` : 'on this device'

  const [storage, setStorage] = useState<WorkspaceStorageState | null>(null)
  const [historyOpen, setHistoryOpen] = useState(false)
  const [reclaiming, setReclaiming] = useState(false)
  const [busy, setBusy] = useState(true)
  const requests = useRef({ generation: 0, busy: true })
  const latestLoad = useRef<((reclaimSpace: boolean) => Promise<void>) | null>(null)
  const readInitial = useEffectEvent(() => {
    const active = requests.current
    const request = ++active.generation
    void read(computerId).then(value => {
      if (active.generation === request) setStorage(value)
    }, cause => {
      if (active.generation === request) showOperationFailure(`storage-read:${computerId}`, 'Could not read storage', { description: errorMessage(cause), retry: () => void latestLoad.current?.(false), native: false })
    }).finally(() => {
      if (active.generation === request) { active.busy = false; setBusy(false) }
    })
    return () => { active.generation++ }
  })
  useEffect(() => readInitial(), [requests])

  async function load(reclaimSpace: boolean) {
    if (requests.current.busy || disabled || (reclaimSpace && (!running || !reclaim))) return
    requests.current.busy = true
    const request = ++requests.current.generation
    setBusy(true)
    setReclaiming(reclaimSpace)
    const toastId = `storage-reclaim:${computerId}`
    const noticeComputer = computerName ? { id: computerId, name: computerName } : undefined
    if (reclaimSpace) showOperationProgress(toastId, { title: 'Freeing up space', step: 'Your files stay available' })
    try {
      const value = await (reclaimSpace ? reclaim! : read)(computerId)
      const current = requests.current.generation === request
      if (current) {
        setStorage(value)
        dismissOperationToast(`storage-read:${computerId}`)
      }
      // The operation's notification outlives the panel that started it.
      if (reclaimSpace) {
        if (value.lastError) showOperationFailure(toastId, 'Could not free up space', { noticeComputer, description: value.lastError, retry: current ? () => void latestLoad.current?.(true) : undefined })
        else showOperationSuccess(toastId, `Freed ${formatBytes(value.lastReclaimedBytes ?? 0)}`, { description: `Released ${where}.`, persist: true, noticeComputer })
      }
    } catch (cause) {
      const current = requests.current.generation === request
      if (reclaimSpace) showOperationFailure(toastId, 'Could not free up space', { noticeComputer, description: errorMessage(cause), retry: current ? () => void latestLoad.current?.(true) : undefined })
      if (current) {
        if (!reclaimSpace) showOperationFailure(`storage-read:${computerId}`, 'Could not read storage', { description: errorMessage(cause), retry: () => void latestLoad.current?.(false), native: false })
        if (reclaimSpace) {
          try {
            const value = await read(computerId)
            if (requests.current.generation === request) setStorage(value)
          } catch { /* Preserve the original operation error if refreshing also fails. */ }
        }
      }
    } finally {
      if (requests.current.generation === request) { requests.current.busy = false; setBusy(false); setReclaiming(false) }
    }
  }

  useLayoutEffect(() => {
    latestLoad.current = load
    return () => { latestLoad.current = null }
  })

  const loading = !storage && busy
  const guest = (value: number | null | undefined) => running && value != null ? formatBytes(value) : '—'
  // A disk Silo could not find is unknown, never a misleading 0 B.
  const host = (value: number | null | undefined) => !storage ? '—' : value == null ? 'Unknown' : formatBytes(value)
  const metrics: StorageMetricProps[] = [
    { icon: HardDrive, label: 'Workspace on disk', value: host(storage?.workspaceHostBytes), help: `Space the workspace disk takes ${where}. Deleted files keep using this space until it is freed up.` },
    { icon: Database, label: 'Runtime on disk', value: host(storage?.runtimeHostBytes), help: 'The computer’s operating system and runtime files. Freeing up space does not shrink it.' },
    { icon: Folder, label: 'Workspace files', value: guest(storage?.workspaceUsedBytes), help: running ? 'Used inside the computer, including filesystem overhead.' : 'Start the computer to measure usage.' },
    { icon: Gauge, label: 'Workspace capacity', value: guest(storage?.workspaceCapacityBytes), help: running ? `The most the computer can hold. This is a limit, not space used ${where}.` : 'Start the computer to measure capacity.' },
  ]
  const checkpointCount = storage?.checkpointCount ?? 0
  const checkpointMetric: StorageMetricProps = {
    icon: Layers,
    label: 'Checkpoints',
    value: host(storage?.checkpointHostBytes),
    help: !storage
      ? loading ? 'Reading saved checkpoint usage…' : 'Refresh storage to check saved checkpoints.'
      : checkpointCount === 0
      ? `No checkpoints are saved ${where}.`
      : `${checkpointCount === 1 ? '1 checkpoint' : `${checkpointCount} checkpoints`} saved ${where}, each counted in full; copies that share blocks can use less. Delete ones you no longer need in Checkpoints.`,
  }

  return <TooltipProvider delayDuration={150}>
    <section aria-label="Computer storage" className="@container grid gap-3 text-xs" aria-busy={busy}>
      <div className="flex min-h-6 items-center justify-between">
        <span className="text-muted-foreground">{location}</span>
        <Tooltip>
          <TooltipTrigger asChild>
            <Button variant="ghost" size="icon-xs" aria-label="Refresh storage" disabled={busy || disabled} onClick={() => void load(false)}>
              <RefreshCw className={busy && !reclaiming ? 'animate-spin' : undefined} />
            </Button>
          </TooltipTrigger>
          <TooltipContent>Refresh storage measurements</TooltipContent>
        </Tooltip>
      </div>

      <div className="grid grid-cols-2 gap-2 @min-[640px]:grid-cols-4">
        {metrics.map(metric => <StorageMetric key={metric.label} {...metric} loading={loading} />)}
        <StorageMetric {...checkpointMetric} loading={loading} className="col-span-2 @min-[640px]:col-span-4" />
      </div>

      <ReclaimControls
        where={where}
        latest={storage?.history[0]}
        running={running}
        disabled={busy || disabled || !running || !storage || !reclaim}
        onReclaim={() => void load(true)}
      />
      {loading && <div role="status" aria-label="Reading storage" className="sr-only">Reading storage…</div>}
      {storage?.lastError && !reclaiming && <p className="flex items-start gap-2 text-destructive"><CircleAlert aria-hidden="true" className="mt-0.5 size-3.5 shrink-0" />{storage.lastError}</p>}

      {storage && <ReclaimHistory history={storage.history} open={historyOpen} onOpenChange={setHistoryOpen} />}
    </section>
  </TooltipProvider>
}

interface StorageMetricProps {
  icon: LucideIcon
  label: string
  value: ReactNode
  /** Visible explanation of what the measurement counts. */
  help: string
}

function StorageMetric({ icon: Icon, label, value, help, loading, className }: StorageMetricProps & { loading: boolean; className?: string }) {
  return <div className={`grid content-start gap-1 rounded-md border border-border bg-background/40 p-3 ${className ?? ''}`}>
    <div className="flex items-center gap-2 text-muted-foreground"><Icon aria-hidden="true" className="size-3.5" /><span>{label}</span></div>
    <div className="mt-1 text-lg font-medium tabular-nums tracking-tight">
      {loading ? <span aria-hidden="true" className="inline-block h-6 w-16 animate-pulse rounded bg-muted" /> : value}
    </div>
    <p className="text-[11px] leading-4 text-muted-foreground">{help}</p>
  </div>
}

function ReclaimControls({ where, latest, running, disabled, onReclaim }: { where: string; latest: ReclaimEntry | undefined; running: boolean; disabled: boolean; onReclaim: () => void }) {
  const result = !latest ? 'Not freed yet'
    : latest.error ? `Last attempt failed · ${ago(latest.at)}`
    : latest.reclaimedBytes ? `Last freed ${formatBytes(latest.reclaimedBytes)} · ${ago(latest.at)}`
    : `Nothing to free · ${ago(latest.at)}`
  return <div className="flex items-center justify-between gap-3 rounded-md border border-border bg-background/40 p-3">
    <div className="grid gap-0.5">
      <span className="font-medium">Unused space</span>
      <span className="max-w-md text-[11px] leading-4 text-muted-foreground">Releases unused blocks {where}; files and capacity stay the same. Silo does this automatically after 7 days of running, or when the computer stops once 24 hours have passed, and waits 24 hours after a failed attempt.</span>
    </div>
    <span className={`ml-auto text-right tabular-nums ${latest?.error ? 'text-destructive/80' : 'text-muted-foreground'}`}>{result}</span>
    <DisabledReason reason={disabled && !running ? 'Start the computer first.' : undefined}>
      <Button variant="outline" size="xs" disabled={disabled} onClick={onReclaim}>Free up space</Button>
    </DisabledReason>
  </div>
}

function ReclaimHistory({ history, open, onOpenChange }: { history: ReclaimEntry[]; open: boolean; onOpenChange: (open: boolean) => void }) {
  const listId = useId()
  const latest = history[0]
  const summary = latest ? `${latest.error ? 'Failed' : `${formatBytes(latest.reclaimedBytes ?? 0)} freed`} · ${date(latest.at)}` : 'No history yet'
  return <div className="border-t border-border pt-2">
    <button type="button" aria-label={`History, ${history.length} ${history.length === 1 ? 'attempt' : 'attempts'}`} aria-expanded={open} aria-controls={listId} onClick={() => onOpenChange(!open)} className="flex w-full items-center gap-2 py-1 text-muted-foreground hover:text-foreground">
      <History aria-hidden="true" className="size-3.5" />
      <span>History</span>
      <span className="rounded bg-muted px-1.5 text-[10px]">{history.length}</span>
      <span className="ml-auto text-[11px]">{summary}</span>
      <ChevronDown aria-hidden="true" className={`size-3 transition-transform ${open ? 'rotate-180' : ''}`} />
    </button>
    {open && <div id={listId} className="mt-2 max-h-56 overflow-y-auto" aria-label="History entries">
      {history.length
        ? history.map((entry, index) => <ReclaimHistoryEntry key={`${entry.at}:${index}`} entry={entry} index={index} />)
        : <p className="py-3 text-muted-foreground">Free-ups will appear here. The latest 50 attempts are retained.</p>}
    </div>}
  </div>
}

function ReclaimHistoryEntry({ entry, index }: { entry: ReclaimEntry; index: number }) {
  const [open, setOpen] = useState(false)
  const detailsId = useId()
  const title = entry.error ? 'Did not complete' : entry.reclaimedBytes === 0 ? 'Nothing to free' : `${formatBytes(entry.reclaimedBytes ?? 0)} freed`
  const details = entry.error ?? 'Completed successfully. Unused blocks were released; workspace files and capacity were preserved.'
  return <div className="border-t border-border/50 py-2.5 pr-2">
    <div className="flex items-center gap-2.5">
      {entry.error ? <CircleAlert aria-hidden="true" className="size-3.5 text-destructive" /> : <Check aria-hidden="true" className="size-3.5 text-muted-foreground" />}
      <div className="min-w-0 flex-1">
        <div>{title}</div>
        <div className="mt-0.5 text-[11px] text-muted-foreground">{trigger(entry.trigger)} · {date(entry.at)}</div>
      </div>
      <button type="button" aria-label={`Details for free-up ${index + 1}`} aria-expanded={open} aria-controls={detailsId} onClick={() => setOpen(!open)} className="text-[11px] text-muted-foreground underline decoration-dotted underline-offset-4 hover:text-foreground">Details</button>
    </div>
    {open && <p id={detailsId} className={`mt-1.5 pl-6 text-[11px] leading-4 ${entry.error ? 'text-destructive' : 'text-muted-foreground'}`}>{details}</p>}
  </div>
}
