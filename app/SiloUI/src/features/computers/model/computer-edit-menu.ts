import { CopyPlus, Monitor, Pencil, Trash2 } from "lucide-react"

import type { MenuAction } from "@/components/actions-menu"
import type { SetupComputerConfiguration } from "@/contracts/silo"
import { computerCan } from "@/features/computers/model/computer-capabilities"

export interface ComputerEditMenuOptions {
  configuration: SetupComputerConfiguration
  /** Names the computer with its device when remote ("dev on Office"). */
  displayName: string
  disabled: boolean
  /** Why edits, desktop installation and deletion must wait. Duplication remains available. */
  busyReason?: string
  /** The computer exists in the runtime, so a Linux desktop can be added to it. */
  created: boolean
  /** A running computer must be stopped before it can be deleted. */
  running: boolean
  separatorBefore: boolean
  onEdit: () => void
  onDuplicate?: () => void
  onAddDesktop: (configuration: SetupComputerConfiguration) => void
}

/**
 * The Edit, Duplicate, Add Linux desktop and Delete items of a computer's ⋯ menu, built once
 * for its list row and its page. Delete opens the menu's "delete" popover.
 */
export function computerEditMenu({ configuration, displayName, disabled, busyReason, created, running, separatorBefore, onEdit, onDuplicate, onAddDesktop }: ComputerEditMenuOptions): MenuAction[] {
  const runningVM = running
  return [
    { label: "Edit", separatorBefore, icon: Pencil, accessibleLabel: `Edit ${configuration.name}`, disabled: disabled || Boolean(busyReason), tooltip: busyReason, onSelect: onEdit },
    { label: "Duplicate settings", icon: CopyPlus, accessibleLabel: `Duplicate settings for ${configuration.name}`, disabled: disabled || !onDuplicate, onSelect: () => onDuplicate?.() },
    ...(!configuration.desktop && created && computerCan(configuration, "desktop") ? [{ label: "Add Linux desktop", icon: Monitor, disabled: disabled || Boolean(busyReason), tooltip: busyReason, onSelect: () => onAddDesktop(configuration) }] : []),
    // Delete always confirms (decision 6), so its label ends with an ellipsis (decision 8).
    { label: "Delete…", icon: Trash2, accessibleLabel: `Delete ${displayName}`, destructive: true, disabled: disabled || runningVM || Boolean(busyReason), tooltip: runningVM ? "Stop the computer before deleting it." : busyReason, popover: "delete" },
  ]
}
