import { useId, useState } from "react"
import { Monitor, Play, Plus, Square } from "lucide-react"

import { ActionsMenu, type MenuAction } from "@/components/actions-menu"
import { ConfirmBody, ConfirmPopover, FormPopover } from "@/components/confirm-popover"
import { ListHeader, listHeadingClassName } from "@/components/list-header"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Progress } from "@/components/ui/progress"
import { ComputerAction, ComputerList, ComputerListItem, ComputerListRow, type ComputerRowTone } from "@/features/computers/components/computer-list"
import type { DeviceCapacity } from "@/features/computers/model/computer-limits"
import { showActionFailure } from "@/lib/operation-toast"
import {
  isMacosCreating,
  macosDefaults,
  macosResources,
  macosStateLabel,
  useMacosComputers,
  validateMacosRequest,
  type MacosComputer,
  type MacosComputerAction,
  type MacosComputerRequest,
  type MacosComputersStore,
} from "../model/macos-computers"

export const macosLicenseNotice = "Silo downloads macOS from Apple (about 20 GB) and installs it. Apple's macOS license allows up to two macOS virtual computers per Mac, for software development, testing, or personal non-commercial use. After installation, finish macOS Setup Assistant in the computer's screen."

const tones: Record<MacosComputer["state"], ComputerRowTone> = {
  preparing: "starting",
  downloading: "starting",
  installing: "starting",
  stopped: "stopped",
  starting: "starting",
  running: "running",
  stopping: "starting",
  failed: "error",
}

const actionVerbs: Record<MacosComputerAction, string> = { start: "start", stop: "stop", "force-stop": "force stop", delete: "delete" }

function parseNumber(text: string) {
  return /^\d+$/.test(text.trim()) ? Number(text.trim()) : Number.NaN
}

function NewMacosComputer({ store, existingNames, capacity }: { store: MacosComputersStore; existingNames: readonly string[]; capacity?: DeviceCapacity }) {
  const [open, setOpen] = useState(false)
  const [name, setName] = useState("")
  const [cpus, setCpus] = useState(String(Math.min(macosDefaults.cpus, capacity?.logicalCPUs ?? macosDefaults.cpus)))
  const [memory, setMemory] = useState(String(Math.min(macosDefaults.memoryGiB, capacity?.memoryGiB ?? macosDefaults.memoryGiB)))
  const [disk, setDisk] = useState(String(macosDefaults.diskGiB))
  const noticeId = useId()
  const request: MacosComputerRequest = { name, cpus: parseNumber(cpus), memoryGiB: parseNumber(memory), diskGiB: parseNumber(disk) }
  const errors = validateMacosRequest(request, existingNames, { maxCPUs: capacity?.logicalCPUs, maxMemoryGiB: capacity?.memoryGiB })
  const invalid = Object.keys(errors).length > 0

  function field(label: string, key: keyof MacosComputerRequest, value: string, onChange: (value: string) => void, numeric: boolean) {
    // An untouched name is incomplete, not wrong.
    const error = key === "name" && name === "" ? undefined : errors[key]
    const errorId = `${noticeId}-${key}`
    return <label className="grid gap-1 text-[11px] font-medium text-muted-foreground">
      {label}
      <Input technical aria-label={label} aria-invalid={Boolean(error)} aria-describedby={error ? errorId : undefined} type={numeric ? "number" : "text"} inputMode={numeric ? "numeric" : undefined} value={value} onChange={event => onChange(event.target.value)} />
      {error && <span id={errorId} className="text-destructive">{error}</span>}
    </label>
  }

  return <FormPopover
    open={open}
    onOpenChange={next => { setOpen(next); if (next) setName("") }}
    align="end"
    title="New macOS computer"
    confirmLabel="Create"
    canSubmit={!invalid}
    anchor={<Button type="button" variant="outline" size="xs" onClick={() => setOpen(true)}><Plus aria-hidden="true" data-icon="inline-start" /> New macOS computer</Button>}
    fields={<div className="grid gap-2">
      {field("Name", "name", name, setName, false)}
      <div className="grid grid-cols-3 gap-2">
        {field("CPUs", "cpus", cpus, setCpus, true)}
        {field("Memory (GiB)", "memoryGiB", memory, setMemory, true)}
        {field("Disk (GiB)", "diskGiB", disk, setDisk, true)}
      </div>
      <p className="text-[11px] text-muted-foreground">{macosLicenseNotice}</p>
    </div>}
    onSubmit={async () => {
      try { await store.create(request) } catch (error) { showActionFailure(`Could not create ${request.name}`, error, undefined, { native: false }) }
    }}
  />
}

function MacosComputerRow({ computer, store }: { computer: MacosComputer; store: MacosComputersStore }) {
  const [pending, setPending] = useState(false)

  async function run(action: MacosComputerAction) {
    setPending(true)
    try { await store.action(computer.id, action) } catch (error) { showActionFailure(`Could not ${actionVerbs[action]} ${computer.name}`, error, undefined, { native: false }) } finally { setPending(false) }
  }

  async function showScreen() {
    try { await store.openDisplay(computer.id) } catch (error) { showActionFailure(`Could not show the screen of ${computer.name}`, error, undefined, { native: false }) }
  }

  const creating = isMacosCreating(computer)
  const settled = computer.state === "stopped" || computer.state === "failed"
  const label = macosStateLabel(computer)
  const items: MenuAction[] = []
  if (computer.state === "running" || computer.state === "stopping") items.push({ label: "Force stop", accessibleLabel: `Force stop ${computer.name}`, disabled: pending, onSelect: () => void run("force-stop") })
  if (settled) items.push({ label: "Delete", accessibleLabel: `Delete ${computer.name}`, destructive: true, popover: "delete" })

  const detail = <span className="grid gap-1">
    <span className="truncate">{label}{computer.osVersion && ` · macOS ${computer.osVersion}`} · {macosResources(computer)}</span>
    {computer.state === "failed" && computer.detail && <span role="alert" className="whitespace-normal">{computer.detail}</span>}
    {creating && <Progress value={computer.progress == null ? null : computer.progress * 100} aria-label={`${computer.name} progress`} />}
  </span>

  return <ComputerListItem data-macos-computer-id={computer.id} aria-busy={creating || pending || undefined}>
    <ComputerListRow
      name={computer.name}
      tone={tones[computer.state]}
      iconState={computer.state === "failed" ? "error" : "normal"}
      detail={detail}
      detailClassName="overflow-visible"
      actions={<>
        {computer.state === "stopped" && <ComputerAction label={`Start ${computer.name}`} disabled={pending} onClick={() => void run("start")}><Play /></ComputerAction>}
        {computer.state === "running" && <ComputerAction label={`Show screen of ${computer.name}`} onClick={() => void showScreen()}><Monitor /></ComputerAction>}
        {computer.state === "running" && <ComputerAction label={`Stop ${computer.name}`} disabled={pending} onClick={() => void run("stop")}><Square /></ComputerAction>}
        {creating && <ConfirmPopover
          align="end"
          tone="destructive"
          title={`Cancel creating ${computer.name}?`}
          description="The download and installation stop and the computer is removed."
          confirmLabel="Cancel creation"
          cancelLabel="Keep creating"
          onConfirm={() => run("delete")}
        >
          <Button type="button" variant="ghost" size="xs" aria-label={`Cancel creating ${computer.name}`} disabled={pending}>Cancel</Button>
        </ConfirmPopover>}
        {items.length > 0 && <ActionsMenu label={`More actions for ${computer.name}`} items={items} popovers={{
          delete: close => <ConfirmBody
            title={`Delete ${computer.name} permanently?`}
            description="Its files will be deleted. This can't be undone."
            confirmLabel="Delete permanently"
            tone="destructive"
            onClose={close}
            onConfirm={() => run("delete")}
          />,
        }} />}
      </>}
    />
  </ComputerListItem>
}

/** The macOS computers of this device. Renders nothing where this build has none or the Mac cannot run them. */
export function MacosComputersSection({ capacity }: { capacity?: DeviceCapacity }) {
  const macos = useMacosComputers()
  const headingId = useId()
  if (!macos) return null
  const { store, snapshot: { state, error, warning } } = macos
  const problem = error && <div role="alert" className="mb-2 flex items-center justify-between gap-2 rounded-md border border-destructive/30 p-2 text-xs text-destructive">
    <span className="min-w-0">Could not read the macOS computers. {error}</span>
    <Button type="button" variant="outline" size="xs" onClick={() => void store.refresh()}>Retry</Button>
  </div>
  // Without a successful read, whether this Mac supports macOS computers is unknown.
  if (!state) return problem ? <div className="shrink-0" data-testid="macos-computers">{problem}</div> : null
  if (!state.supported) return null
  const count = state.computers.length
  return <div role="group" aria-labelledby={headingId} className="shrink-0" data-testid="macos-computers">
    <ListHeader
      heading={<h3 id={headingId} className={listHeadingClassName}>macOS computers</h3>}
      subtitle={`${count} ${count === 1 ? "computer" : "computers"} on this device`}
      actions={<NewMacosComputer store={store} existingNames={state.computers.map(({ name }) => name)} capacity={capacity} />}
    />
    {problem}
    {warning && <p role="status" className="mb-2 text-[11px] text-amber-700 dark:text-amber-400">{warning}</p>}
    {count > 0 && <ComputerList label="macOS computers" className="max-h-80">
      {state.computers.map(computer => <MacosComputerRow key={computer.id} computer={computer} store={store} />)}
    </ComputerList>}
  </div>
}
