import { ShutdownBoundary } from "./shutdown-boundary"
import { useEffect, useRef, useState } from "react"
import { invoke } from "@tauri-apps/api/core"
import { listen } from "@tauri-apps/api/event"

import { NativeComputerMenu } from "./native-computer-menu"
import { useStatusPanelSize } from "./use-status-panel-size"
import { desktopCommand } from "./commands"
import { Toaster } from "@/components/ui/sonner"
import { TooltipProvider } from "@/components/ui/tooltip"
import type { ApplicationSource } from "@/features/application/model/application-source"
import { StatusBarContent } from "@/features/status-bar/status-bar"
import { statusBarHealth } from "@/features/status-bar/status-bar-model"
import { useSettings } from "@/features/preferences/settings-store"
import type { StatusBarActions } from "@/features/status-bar/status-bar-types"

export function StatusPanel({ source: input, actions, notice }: { source: ApplicationSource; actions: StatusBarActions; notice?: string }) {
  const { settings } = useSettings(input.preferences)
  const source = { ...input, preferences: { ...input.preferences, ...settings } }
  const content = useRef<HTMLDivElement>(null)
  const [opening, setOpening] = useState(0)
  const health = statusBarHealth(source)
  useStatusPanelSize(content)

  useEffect(() => {
    const element = content.current!
    let disposed = false
    let stop: (() => void) | undefined
    void listen("desktop:status-opened", () => {
      if (disposed) return
      setOpening((current) => current + 1)
      element.focus()
    }).then(unlisten => { if (disposed) unlisten(); else stop = unlisten })
      .catch(error => console.error("Silo status events:", error))
    return () => { disposed = true; stop?.() }
  }, [])

  useEffect(() => {
    void invoke("update_tray", { tone: health.tone, label: health.label }).catch(console.error)
  }, [health.tone, health.label])

  function dismissThen(action: () => void) { void desktopCommand("hide_status").then(action) }
  const nativeActions: StatusBarActions = {
    ...actions,
    quit: () => { void desktopCommand("quit_app") },
    openTerminal: (name) => dismissThen(() => actions.openTerminal(name)),
    openEditor: (name, path) => dismissThen(() => actions.openEditor(name, path)),
    openSite: (name, port) => dismissThen(() => actions.openSite(name, port)),
  }

  return <TooltipProvider delayDuration={150} reduceMotion={source.preferences.reduceMotion}>
    <div ref={content} role="dialog" aria-label="Silo" tabIndex={-1}
      className="silo-window status-panel flex flex-col overflow-hidden rounded-xl border border-border bg-popover text-popover-foreground outline-none"
      data-reduce-motion={source.preferences.reduceMotion}
      onKeyDown={(event) => {
        // Nested menus consume Escape first; the next Escape dismisses the panel.
        if (event.key === "Escape" && !event.defaultPrevented) { event.preventDefault(); void desktopCommand("hide_status") }
      }}>
      <ShutdownBoundary compact>{notice && <p role="status" className="shrink-0 px-3 pt-2 text-caption text-muted-foreground">{notice}</p>}<StatusBarContent computerMenu={NativeComputerMenu} key={opening} source={source} actions={nativeActions} focusContent={() => content.current?.focus()} /></ShutdownBoundary>
    </div>
    <Toaster position="bottom-center" offset={8} mobileOffset={8} visibleToasts={3} expand={false} reduceMotion={source.preferences.reduceMotion} toastOptions={{ classNames: { toast: "cn-toast !w-[calc(100vw-16px)] max-w-[364px]" } }} />
  </TooltipProvider>
}
