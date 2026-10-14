import { useState } from "react"
import { Download, History, Monitor, Play, Square } from "lucide-react"

import { ActionsMenu, type MenuAction } from "@/components/actions-menu"
import { ConfirmBody, ConfirmPopover } from "@/components/confirm-popover"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Progress } from "@/components/ui/progress"
import { ComputerAction, ComputerListItem, ComputerListRow, type ComputerRowTone } from "@/features/computers/components/computer-list"
import { DeviceBadge } from "@/features/computers/components/device-badge"
import type { ComputerDevice } from "@/features/application/model/connections"
import { clipboardFeedback } from "@/desktop/viewer-clipboard-feedback"
import { showActionFailure, showQuickConfirmation } from "@/lib/operation-toast"
import { MacosCheckpointPanel } from "./macos-checkpoint-panel"
import {
  canCheckpointMacos,
  canRetryMacosSetup,
  isMacosCreating,
  isMacosSettingUp,
  macosResources,
  macosStateLabel,
  type MacosComputer,
  type MacosClipboardDirection,
  type MacosComputerAction,
  type MacosComputersStore,
} from "../model/macos-computers"

const tones: Record<MacosComputer["state"], ComputerRowTone> = {
  preparing: "starting",
  copying: "starting",
  downloading: "starting",
  installing: "starting",
  "setting-up": "starting",
  stopped: "stopped",
  starting: "starting",
  running: "running",
  stopping: "starting",
  failed: "error",
}

const actionVerbs: Record<Exclude<MacosComputerAction, object>, string> = { start: "start", stop: "stop", "force-stop": "force stop", delete: "delete", setup: "set up" }

function actionVerb(action: MacosComputerAction) {
  return typeof action === "string" ? actionVerbs[action] : "change CPU and memory for"
}

/**
 * A macOS computer in the computers list. It has no detail page, terminal, editor or reorder handle;
 * its checkpoints open under the row, with the list the Linux detail page's Checkpoints tab shows.
 * `takenNames` are the computer names on this device, for naming a fork.
 * A computer of another device (`device`, with the id it has in the unified list) shows that device's
 * badge and has no clipboard transfer or checkpoints; while the device is offline or updating its
 * state is the last known one and its actions are disabled.
 */
export function MacosComputerRow({ computer, store, takenNames = [], device, onExport }: { computer: MacosComputer; store: MacosComputersStore; takenNames?: readonly string[]; device?: ComputerDevice; onExport?: (name: string, checkpoint?: { id: string; name: string }) => void | Promise<unknown> }) {
  const [pending, setPending] = useState(false)
  const [checkpointsOpen, setCheckpointsOpen] = useState(false)
  const [cpus, setCpus] = useState(String(computer.cpus))
  const [memoryGiB, setMemoryGiB] = useState(String(computer.memoryGiB))
  const [expectedResources, setExpectedResources] = useState({ cpus: computer.cpus, memoryGiB: computer.memoryGiB })
  const [resourceError, setResourceError] = useState<string | null>(null)
  const unavailable = Boolean(device && (!device.connected || device.busy))
  const checkpointsAvailable = !device && (canCheckpointMacos(computer) || (computer.checkpoints?.length ?? 0) > 0)
  const operating = computer.checkpointOperation?.status === "running"

  async function run(action: MacosComputerAction) {
    setPending(true)
    try { await store.action(computer.id, action); return true } catch (error) { showActionFailure(`Could not ${actionVerb(action)} ${computer.name}`, error, undefined, { native: false }); return false } finally { setPending(false) }
  }

  async function showScreen() {
    try { await store.openDisplay(computer.id) } catch (error) { showActionFailure(`Could not show the screen of ${computer.name}`, error, undefined, { native: false }) }
  }

  async function transferClipboard(direction: MacosClipboardDirection) {
    const verb = direction === "paste-into" ? "paste into" : "copy from"
    try {
      const feedback = clipboardFeedback(await store.clipboard(computer.id, direction), computer.name)
      if (feedback.error) showActionFailure(`Could not ${verb} ${computer.name}`, feedback.text, undefined, { native: false })
      else showQuickConfirmation(feedback.text)
    } catch (error) { showActionFailure(`Could not ${verb} ${computer.name}`, error, undefined, { native: false }) }
  }

  const creating = isMacosCreating(computer)
  const settingUp = isMacosSettingUp(computer)
  const settled = computer.state === "stopped" || computer.state === "failed"
  const label = macosStateLabel(computer)
  const items: MenuAction[] = []
  if (!device && onExport && canCheckpointMacos(computer)) items.push({
    label: "Export…",
    accessibleLabel: `Export ${computer.name}`,
    icon: Download,
    disabled: pending || operating || unavailable,
    onSelect: () => { void onExport(computer.name) },
  })
  if (computer.state === "stopped") items.push({
    label: "Edit CPU & memory",
    accessibleLabel: `Edit CPU and memory for ${computer.name}`,
    popover: "resources",
    disabled: unavailable || Boolean(computer.pendingRestore?.memory),
    tooltip: computer.pendingRestore?.memory ? "Finish restoring saved memory before changing CPU or memory." : undefined,
    onSelect: () => {
      setCpus(String(computer.cpus))
      setMemoryGiB(String(computer.memoryGiB))
      setExpectedResources({ cpus: computer.cpus, memoryGiB: computer.memoryGiB })
      setResourceError(null)
    },
  })
  // Clipboard transfers go over the guest account that setup creates.
  if (!device && computer.state === "running" && computer.setupComplete) items.push(
    { label: "Paste into computer", accessibleLabel: `Paste into ${computer.name}`, onSelect: () => void transferClipboard("paste-into") },
    { label: "Copy from computer", accessibleLabel: `Copy from ${computer.name}`, onSelect: () => void transferClipboard("copy-from") },
  )
  if (computer.state === "running" || computer.state === "stopping") items.push({ label: "Force stop", accessibleLabel: `Force stop ${computer.name}`, disabled: pending || unavailable, onSelect: () => void run("force-stop") })
  if (settled) items.push({ label: "Delete", accessibleLabel: `Delete ${computer.name}`, destructive: true, popover: "delete", disabled: operating || unavailable })

  const detail = <span className="grid gap-1">
    <span className="truncate">{label}{computer.osVersion && ` · macOS ${computer.osVersion}`} · {macosResources(computer)}{device && unavailable && ` · ${device.busy ? "Updating…" : "Offline · last known status"}`}</span>
    {(computer.state === "failed" || settingUp) && computer.detail && <span role={settingUp ? undefined : "alert"} className="whitespace-normal">{computer.detail}</span>}
    {(computer.state === "running" || computer.state === "stopping") && computer.detail && <span className="whitespace-normal">{computer.detail}</span>}
    {operating && <span>{computer.checkpointOperation?.stage}…</span>}
    {(creating || settingUp) && <Progress value={computer.progress == null ? null : computer.progress * 100} aria-label={`${computer.name} progress`} />}
  </span>

  return <ComputerListItem data-macos-computer-id={computer.id} aria-busy={creating || settingUp || pending || operating || undefined}>
    <ComputerListRow
      name={computer.name}
      os="macos"
      remote={Boolean(device)}
      kindBadge={device ? <DeviceBadge device={device} /> : undefined}
      leading={<span aria-hidden="true" className="size-7 shrink-0" />}
      tone={tones[computer.state]}
      iconState={computer.state === "failed" ? "error" : "normal"}
      detail={detail}
      detailClassName="overflow-visible"
      actions={<>
        {(computer.state === "stopped" || (computer.state === "failed" && computer.installed)) && !computer.needsPersonalizing && <ComputerAction label={`Start ${computer.name}`} disabled={pending || operating || unavailable} onClick={() => void run("start")}><Play /></ComputerAction>}
        {checkpointsAvailable && <ComputerAction label={`Checkpoints of ${computer.name}`} aria-expanded={checkpointsOpen} onClick={() => setCheckpointsOpen(open => !open)}><History /></ComputerAction>}
        {computer.state === "running" && <ComputerAction label={`Show screen of ${computer.name}`} disabled={unavailable} onClick={() => void showScreen()}><Monitor /></ComputerAction>}
        {computer.state === "running" && <ComputerAction label={`Stop ${computer.name}`} disabled={pending || operating || unavailable} onClick={() => void run("stop")}><Square /></ComputerAction>}
        {canRetryMacosSetup(computer) && <Button type="button" variant="ghost" size="xs" aria-label={`Retry setup of ${computer.name}`} disabled={pending || unavailable} onClick={() => void run("setup")}>Retry setup</Button>}
        {settingUp && <ConfirmPopover
          align="end"
          tone="destructive"
          title={`Cancel setting up ${computer.name}?`}
          description="The setup stops and the computer is removed."
          confirmLabel="Cancel setup"
          cancelLabel="Keep setting up"
          onConfirm={async () => { await run("delete") }}
        >
          <Button type="button" variant="ghost" size="xs" aria-label={`Cancel setting up ${computer.name}`} disabled={pending || unavailable}>Cancel</Button>
        </ConfirmPopover>}
        {creating && <ConfirmPopover
          align="end"
          tone="destructive"
          title={`Cancel creating ${computer.name}?`}
          description={computer.state === "copying" ? "The copy stops and the computer is removed." : "The download and installation stop and the computer is removed."}
          confirmLabel="Cancel creation"
          cancelLabel="Keep creating"
          onConfirm={async () => { await run("delete") }}
        >
          <Button type="button" variant="ghost" size="xs" aria-label={`Cancel creating ${computer.name}`} disabled={pending || unavailable}>Cancel</Button>
        </ConfirmPopover>}
        {items.length > 0 && <ActionsMenu label={`More actions for ${computer.name}`} items={items} popovers={{
          resources: close => <form className="grid gap-3" onSubmit={event => {
            event.preventDefault()
            const parsedCpus = Number(cpus)
            const parsedMemory = Number(memoryGiB)
            if (!Number.isSafeInteger(parsedCpus) || parsedCpus < 2 || !Number.isSafeInteger(parsedMemory) || parsedMemory < 4) {
              setResourceError("Enter at least 2 CPUs and 4 GiB of memory.")
              return
            }
            void run({ "change-resources": {
              cpus: parsedCpus,
              memoryGiB: parsedMemory,
              expected: expectedResources,
            } }).then(saved => { if (saved) close() })
          }}>
            <div className="grid gap-1.5">
              <label htmlFor={`macos-cpus-${computer.id}`} className="font-medium">CPUs</label>
              <Input id={`macos-cpus-${computer.id}`} type="number" min={2} step={1} required value={cpus} onChange={event => setCpus(event.target.value)} />
            </div>
            <div className="grid gap-1.5">
              <label htmlFor={`macos-memory-${computer.id}`} className="font-medium">Memory (GiB)</label>
              <Input id={`macos-memory-${computer.id}`} type="number" min={4} step={1} required value={memoryGiB} onChange={event => setMemoryGiB(event.target.value)} />
            </div>
            <p className="text-muted-foreground">Changes apply the next time this computer starts.</p>
            {resourceError && <p role="alert" className="text-destructive">{resourceError}</p>}
            <div className="flex justify-end gap-2">
              <Button type="button" variant="ghost" size="sm" onClick={close}>Cancel</Button>
              <Button type="submit" size="sm" disabled={pending}>Save</Button>
            </div>
          </form>,
          delete: close => <ConfirmBody
            title={`Delete ${computer.name} permanently?`}
            description="Its files will be deleted. This can't be undone."
            confirmLabel="Delete permanently"
            tone="destructive"
            onClose={close}
            onConfirm={async () => { await run("delete") }}
          />,
        }} />}
      </>}
    />
    {checkpointsOpen && checkpointsAvailable && <div className="border-t border-border px-3 py-2.5"><MacosCheckpointPanel computer={computer} store={store} takenNames={takenNames} onStart={() => void run("start")} onExport={onExport} /></div>}
  </ComputerListItem>
}
