import { Cpu, OctagonX, Trash2 } from "lucide-react"

import type { MenuAction, MenuPopovers } from "@/components/actions-menu"
import { ConfirmBody } from "@/components/confirm-popover"
import { MacosResourcesForm } from "./macos-resources-form"
import { showActionFailure } from "@/lib/operation-toast"
import type { MacosComputer, MacosComputerAction, MacosComputersStore } from "../model/macos-computers"

const verbs: Record<Exclude<MacosComputerAction, object>, string> = { start: "start", stop: "stop", "force-stop": "force stop", delete: "delete", setup: "set up" }

/** Runs a macOS computer action and reports a failure on screen; resolves to whether it succeeded. */
export async function performMacosAction(store: MacosComputersStore, computer: MacosComputer, action: Exclude<MacosComputerAction, object>) {
  try {
    await store.action(computer.id, action)
    return true
  } catch (error) {
    showActionFailure(`Could not ${verbs[action]} ${computer.name}`, error, undefined, { native: false })
    return false
  }
}

/**
 * The ⋯ menu items a macOS computer's page adds to the shared ones: Edit CPU & memory while it is
 * stopped, Force stop, and Delete. `unavailable` is why changes must wait, when they must.
 */
export function macosDetailMenu({ computer, store, unavailable, onDeleted }: {
  computer: MacosComputer
  store: MacosComputersStore
  unavailable?: string
  onDeleted: () => void
}): { items: MenuAction[]; popovers: MenuPopovers } {
  const settled = computer.state === "stopped" || computer.state === "failed"
  const operating = computer.checkpointOperation?.status === "running"
  const waiting = unavailable ?? (operating ? "Wait for the checkpoint to finish." : undefined)
  const items: MenuAction[] = []
  if (computer.state === "stopped") items.push({
    label: "Edit CPU & memory",
    icon: Cpu,
    separatorBefore: true,
    accessibleLabel: `Edit CPU and memory for ${computer.name}`,
    popover: "resources",
    disabled: Boolean(waiting) || Boolean(computer.pendingRestore?.memory),
    tooltip: computer.pendingRestore?.memory ? "Finish restoring saved memory before changing CPU or memory." : waiting,
  })
  if (computer.state === "running" || computer.state === "stopping") items.push({
    label: "Force stop",
    icon: OctagonX,
    accessibleLabel: `Force stop ${computer.name}`,
    disabled: Boolean(waiting),
    tooltip: waiting,
    onSelect: () => { void performMacosAction(store, computer, "force-stop") },
  })
  if (settled) items.push({
    label: "Delete…",
    icon: Trash2,
    accessibleLabel: `Delete ${computer.name}`,
    destructive: true,
    popover: "delete",
    disabled: Boolean(waiting),
    tooltip: waiting,
  })
  return {
    items,
    popovers: {
      resources: close => <MacosResourcesForm computer={computer} store={store} onClose={close} />,
      delete: close => <ConfirmBody
        title={`Delete ${computer.name} permanently?`}
        description="Its files will be deleted. This can't be undone."
        confirmLabel="Delete permanently"
        tone="destructive"
        onClose={close}
        onConfirm={async () => { if (await performMacosAction(store, computer, "delete")) onDeleted() }}
      />,
    },
  }
}

