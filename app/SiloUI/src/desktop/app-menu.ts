import { useEffect, useEffectEvent, useState } from "react"
import { invoke, isTauri } from "@tauri-apps/api/core"
import { listen } from "@tauri-apps/api/event"
import { isMac } from "@/lib/platform"
import { desktopShortcutCommand } from "@/lib/shortcuts"

export interface AppMenuState {
  ready: boolean
  busy: boolean
  canGoBack: boolean
  canGoForward: boolean
  canCreateComputer: boolean
  canImport: boolean
  canCheckUpdates: boolean
  sidebarCollapsed: boolean
}

export function useAppMenu(state: AppMenuState, onCommand: (command: string) => void) {
  const [connected, setConnected] = useState(false)
  const receive = useEffectEvent((command: string) => {
    if (command !== "search" && document.querySelector('[role="dialog"], [role="alertdialog"]')) return
    onCommand(command)
  })
  useEffect(() => {
    if (!isTauri() || isMac() || connected) return
    const onKeyDown = (event: KeyboardEvent) => {
      const command = desktopShortcutCommand(event)
      if (command) { event.preventDefault(); receive(command) }
    }
    window.addEventListener("keydown", onKeyDown)
    return () => window.removeEventListener("keydown", onKeyDown)
  }, [connected])
  useEffect(() => {
    if (!isTauri()) return
    let disposed = false
    let stop: (() => void) | undefined
    void listen<string>("silo://menu-command", ({ payload }) => {
      if (!disposed) receive(payload)
    }).then((unlisten) => {
      if (disposed) unlisten()
      else { stop = unlisten; setConnected(true) }
    }).catch(console.error)
    return () => {
      disposed = true
      stop?.()
      void invoke("set_app_menu_state", { state: { ...state, ready: false } }).catch(console.error)
    }
  // Native subscription lasts for this application view; the receiver always sees current state.
  // oxlint-disable-next-line react-hooks/exhaustive-deps
  }, [])
  const serialized = JSON.stringify(state)
  useEffect(() => {
    if (connected) void invoke("set_app_menu_state", { state: JSON.parse(serialized) }).catch(console.error)
  }, [connected, serialized])
  return connected
}
