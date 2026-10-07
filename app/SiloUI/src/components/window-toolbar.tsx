import { isMac } from "@/lib/platform"
import type { ComponentProps, ReactNode, RefObject } from "react"
import { isTauri } from "@tauri-apps/api/core"
import { getCurrentWindow } from "@tauri-apps/api/window"
import { PanelLeft } from "lucide-react"
import { LinuxMenuButton } from "@/desktop/linux-menu-button"

import type { KeyboardShortcut } from "@/lib/shortcuts"
import { Button } from "@/components/ui/button"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"

export function WindowControls() {
  const desktop = isTauri()
  if (desktop && isMac()) {
    // macOS draws its system controls over this space in the webview.
    return <div className="h-3.5 w-[3.625rem] shrink-0" aria-hidden="true" data-window-controls />
  }
  const controls = [
    { action: "close", label: "Close window", color: "border-[#e0443e] bg-[#ff5f57]" },
    { action: "minimize", label: "Minimize window", color: "border-[#dea123] bg-[#febc2e]" },
    { action: "toggleMaximize", label: "Maximize or restore window", color: "border-[#1aab29] bg-[#28c840]" },
  ] as const
  return <div className="flex shrink-0 items-center gap-2" aria-hidden={desktop ? undefined : true} data-window-controls>
    {controls.map(({ action, label, color }) => desktop
      ? <button key={action} type="button" aria-label={label} className={`size-3.5 rounded-full border focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring ${color}`} onClick={() => {
          void getCurrentWindow()[action]().catch((error: unknown) => console.error(`Window ${action} failed`, error))
        }} />
      : <span key={action} className={`size-3.5 rounded-full border ${color}`} />)}
  </div>
}

export function WindowTitleBar({ title }: { title: string }) {
  const dragRegion = isTauri() || undefined
  return <header aria-label="Window toolbar" data-tauri-drag-region={dragRegion} className="grid h-11 shrink-0 grid-cols-[1fr_auto_1fr] items-center border-b border-border bg-background px-3 select-none">
    <WindowControls />
    <h1 data-tauri-drag-region={dragRegion} className="text-[13px] font-medium">{title}</h1>
    <div data-tauri-drag-region={dragRegion} className="flex justify-end"><LinuxMenuButton /></div>
  </header>
}

export function ToolbarButton({ label, showTooltip = true, shortcut, ...props }: ComponentProps<typeof Button> & { label: string; showTooltip?: boolean; shortcut?: KeyboardShortcut }) {
  return <Tooltip>
    <TooltipTrigger asChild>
      <Button variant="ghost" size="icon-sm" className="size-7 text-muted-foreground hover:text-foreground disabled:opacity-35 [&_svg]:size-4" aria-label={label} aria-keyshortcuts={shortcut?.aria} {...props} />
    </TooltipTrigger>
    <TooltipContent side="bottom" hidden={!showTooltip} shortcut={shortcut}>{label}</TooltipContent>
  </Tooltip>
}

interface WindowToolbarProps {
  sidebarDisabled?: boolean
  sidebarShortcut?: KeyboardShortcut
  title: string
  sidebarId: string
  collapsed: boolean
  previewing: boolean
  toggleRef: RefObject<HTMLButtonElement | null>
  onToggleSidebar: () => void
  onPreviewEnter: () => void
  onPreviewLeave: () => void
  navigation?: ReactNode
  children?: ReactNode
}

export function WindowToolbar({ title, sidebarId, collapsed, previewing, toggleRef, onToggleSidebar, onPreviewEnter, onPreviewLeave, navigation, children, sidebarDisabled = false, sidebarShortcut }: WindowToolbarProps) {
  const dragRegion = isTauri() || undefined
  return <header aria-label="Window toolbar" data-tauri-drag-region={dragRegion} className="flex h-11 shrink-0 items-center border-b border-border bg-background select-none">
    {children && <h1 className="sr-only">{title}</h1>}
    <div data-tauri-drag-region={dragRegion} className="silo-titlebar-controls flex h-full shrink-0 items-center gap-3 pr-2 pl-3">
      <WindowControls />
      <div data-tauri-drag-region={dragRegion} className="flex items-center gap-1">
        <ToolbarButton
          ref={toggleRef}
          shortcut={sidebarShortcut}
          disabled={sidebarDisabled}
          label={collapsed ? previewing ? "Keep sidebar open" : "Expand sidebar" : "Collapse sidebar"}
          showTooltip={!previewing}
          aria-expanded={!collapsed || previewing}
          aria-controls={sidebarId}
          onPointerEnter={(event) => { if (event.pointerType !== "touch") onPreviewEnter() }}
          onPointerLeave={onPreviewLeave}
          onClick={onToggleSidebar}
        ><PanelLeft /></ToolbarButton>
        {navigation}
      </div>
      <span aria-hidden="true" data-tauri-drag-region={dragRegion} className="silo-toolbar-divider" />
    </div>
    <div data-tauri-drag-region={dragRegion} className="flex min-w-0 flex-1 items-center">
      {children ? <div data-tauri-drag-region={dragRegion} className="mx-auto flex w-full max-w-4xl items-center px-4 sm:px-6">{children}</div> : <h1 data-tauri-drag-region={dragRegion} className="px-4 text-[13px] font-medium sm:px-6">{title}</h1>}
    </div>
    <LinuxMenuButton disabled={sidebarDisabled} />
  </header>
}
