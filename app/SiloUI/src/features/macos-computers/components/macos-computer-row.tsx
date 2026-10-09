import { useState } from "react"
import { Monitor, Play, Square } from "lucide-react"

import { ActionsMenu, type MenuAction } from "@/components/actions-menu"
import { ConfirmBody, ConfirmPopover } from "@/components/confirm-popover"
import { Button } from "@/components/ui/button"
import { Progress } from "@/components/ui/progress"
import { ComputerAction, ComputerListItem, ComputerListRow, type ComputerRowTone } from "@/features/computers/components/computer-list"
import { clipboardFeedback } from "@/desktop/viewer-clipboard-feedback"
import { showActionFailure, showQuickConfirmation } from "@/lib/operation-toast"
import {
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
  downloading: "starting",
  installing: "starting",
  "setting-up": "starting",
  stopped: "stopped",
  starting: "starting",
  running: "running",
  stopping: "starting",
  failed: "error",
}

const actionVerbs: Record<MacosComputerAction, string> = { start: "start", stop: "stop", "force-stop": "force stop", delete: "delete", setup: "set up" }

/** A macOS computer in the computers list. It has no detail page, terminal, editor or reorder handle. */
export function MacosComputerRow({ computer, store }: { computer: MacosComputer; store: MacosComputersStore }) {
  const [pending, setPending] = useState(false)

  async function run(action: MacosComputerAction) {
    setPending(true)
    try { await store.action(computer.id, action) } catch (error) { showActionFailure(`Could not ${actionVerbs[action]} ${computer.name}`, error, undefined, { native: false }) } finally { setPending(false) }
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
  // Clipboard transfers go over the guest account that setup creates.
  if (computer.state === "running" && computer.setupComplete) items.push(
    { label: "Paste into computer", accessibleLabel: `Paste into ${computer.name}`, onSelect: () => void transferClipboard("paste-into") },
    { label: "Copy from computer", accessibleLabel: `Copy from ${computer.name}`, onSelect: () => void transferClipboard("copy-from") },
  )
  if (computer.state === "running" || computer.state === "stopping") items.push({ label: "Force stop", accessibleLabel: `Force stop ${computer.name}`, disabled: pending, onSelect: () => void run("force-stop") })
  if (settled) items.push({ label: "Delete", accessibleLabel: `Delete ${computer.name}`, destructive: true, popover: "delete" })

  const detail = <span className="grid gap-1">
    <span className="truncate">{label}{computer.osVersion && ` · macOS ${computer.osVersion}`} · {macosResources(computer)}</span>
    {(computer.state === "failed" || settingUp) && computer.detail && <span role={settingUp ? undefined : "alert"} className="whitespace-normal">{computer.detail}</span>}
    {(computer.state === "running" || computer.state === "stopping") && computer.detail && <span className="whitespace-normal">{computer.detail}</span>}
    {(creating || settingUp) && <Progress value={computer.progress == null ? null : computer.progress * 100} aria-label={`${computer.name} progress`} />}
  </span>

  return <ComputerListItem data-macos-computer-id={computer.id} aria-busy={creating || settingUp || pending || undefined}>
    <ComputerListRow
      name={computer.name}
      os="macos"
      leading={<span aria-hidden="true" className="size-7 shrink-0" />}
      tone={tones[computer.state]}
      iconState={computer.state === "failed" ? "error" : "normal"}
      detail={detail}
      detailClassName="overflow-visible"
      actions={<>
        {(computer.state === "stopped" || (computer.state === "failed" && computer.installed)) && <ComputerAction label={`Start ${computer.name}`} disabled={pending} onClick={() => void run("start")}><Play /></ComputerAction>}
        {computer.state === "running" && <ComputerAction label={`Show screen of ${computer.name}`} onClick={() => void showScreen()}><Monitor /></ComputerAction>}
        {computer.state === "running" && <ComputerAction label={`Stop ${computer.name}`} disabled={pending} onClick={() => void run("stop")}><Square /></ComputerAction>}
        {canRetryMacosSetup(computer) && <Button type="button" variant="ghost" size="xs" aria-label={`Retry setup of ${computer.name}`} disabled={pending} onClick={() => void run("setup")}>Retry setup</Button>}
        {settingUp && <ConfirmPopover
          align="end"
          tone="destructive"
          title={`Cancel setting up ${computer.name}?`}
          description="The setup stops and the computer is removed."
          confirmLabel="Cancel setup"
          cancelLabel="Keep setting up"
          onConfirm={() => run("delete")}
        >
          <Button type="button" variant="ghost" size="xs" aria-label={`Cancel setting up ${computer.name}`} disabled={pending}>Cancel</Button>
        </ConfirmPopover>}
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

