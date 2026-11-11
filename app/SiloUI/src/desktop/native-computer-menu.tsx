import { useEffect, useRef } from "react"
import { Menu, type MenuOptions } from "@tauri-apps/api/menu"
import { LogicalPosition } from "@tauri-apps/api/dpi"
import { MoreHorizontal } from "lucide-react"
import { Button } from "@/components/ui/button"
import { showActionFailure, showQuickConfirmation } from "@/lib/operation-toast"
import { computerTarget } from "@/features/application/model/connections"
import type { ComputerMenuProps } from "@/features/status-bar/status-bar-types"
import { computerMenuItems, type ComputerMenuItem } from "@/features/status-bar/computer-menu-items"
import { openableAgents } from "@/features/agents/model/agents"

type NativeItems = NonNullable<MenuOptions["items"]>

// The same items as the browser preview's Radix menu, as native menu entries.
function nativeItems(items: ComputerMenuItem[], current: () => boolean): NativeItems {
  return items.map((item): NativeItems[number] => {
    if (item.kind === "separator") return { item: "Separator" }
    if (item.kind === "submenu") return { text: item.label, enabled: item.enabled, items: nativeItems(item.items, current) }
    if (item.kind === "copy") {
      return { text: item.label, action: () => {
        if (!current()) return
        void navigator.clipboard.writeText(item.value).then(
          () => { if (current()) showQuickConfirmation(item.copied) },
          (error) => { if (current()) showActionFailure(item.failed, error, undefined, { native: false }) },
        )
      } }
    }
    return { text: item.label, enabled: item.enabled, action: () => { if (current()) item.run() } }
  })
}

// HTML portals cannot draw outside the status webview. Let the OS own the
// popup and its submenus, including screen-edge placement and keyboard tracking.
export function NativeComputerMenu({ computer, source, actions, onFolders, onConfirm, agents }: ComputerMenuProps) {
  const opening = useRef(false)
  const sequence = useRef(0)
  const activeMenu = useRef<Menu | null>(null)
  const buttonRef = useRef<HTMLButtonElement>(null)
  const { configuration } = computer
  const target = computerTarget(computer)
  useEffect(() => () => {
    sequence.current++
    opening.current = false
    const menu = activeMenu.current
    activeMenu.current = null
    void menu?.close().catch(console.error)
  }, [target])

  async function open(button: HTMLButtonElement) {
    if (opening.current) return
    opening.current = true
    const request = ++sequence.current
    const current = () => request === sequence.current && buttonRef.current === button
    const bounds = button.getBoundingClientRect()
    const items = nativeItems(computerMenuItems(computer, source, {
      start: () => actions.startComputer(target),
      confirm: onConfirm,
      openTerminal: () => actions.openTerminal(target),
      agents: openableAgents(agents),
      openAgent: actions.openAgent && ((agent) => actions.openAgent?.(target, agent)),
      chooseFolder: onFolders,
      openSite: (port) => actions.openSite(target, port),
    }), current)
    let menu: Menu | undefined
    let tracked = false
    try {
      menu = await Menu.new({ items })
      if (!current() || !button.isConnected) return
      activeMenu.current = menu
      tracked = true
      await menu.popup(new LogicalPosition(bounds.left, bounds.bottom))
    } catch (error) {
      if (!current()) return
      console.error("Silo status menu:", error)
      showActionFailure("Could not open computer actions", error, () => { if (current() && buttonRef.current) void open(buttonRef.current) }, { native: false })
    } finally {
      if (current()) opening.current = false
      // On macOS popup resolves after native menu tracking ends.
      if (menu && (!tracked || activeMenu.current === menu)) {
        if (activeMenu.current === menu) activeMenu.current = null
        void menu.close().catch(console.error)
      }
    }
  }

  return <>
    <Button ref={buttonRef} variant="ghost" size="icon-xs" aria-label={`Actions for ${configuration.name}`} aria-haspopup="menu"
      onClick={(event) => { void open(event.currentTarget) }}><MoreHorizontal /></Button>
  </>
}
