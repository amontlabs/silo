import { useRef, useState } from "react"
import { CircleAlert, LoaderCircle, TriangleAlert } from "lucide-react"

import { SiloMark } from "@/components/silo-mark"
import { Button } from "@/components/ui/button"
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover"
import { TooltipProvider } from "@/components/ui/tooltip"
import type { ApplicationSource } from "@/features/application/model/application-source"
import { cn } from "@/lib/utils"
import { StatusBarContent } from "./status-bar"
import { statusBarHealth } from "./status-bar-model"
import type { StatusBarActions } from "./status-bar-types"

function StatusBarIcon({ tone, reduceMotion }: { tone: ReturnType<typeof statusBarHealth>["tone"]; reduceMotion: boolean }) {
  const color = tone === "error" ? "text-destructive"
    : tone === "warning" || tone === "busy" ? "text-warning"
      : tone === "neutral" ? "text-muted-foreground" : "text-foreground"
  const Indicator = tone === "busy" ? LoaderCircle : tone === "error" ? CircleAlert : tone === "warning" ? TriangleAlert : null
  return (
    <span className={cn("relative size-4", color)} aria-hidden="true">
      <SiloMark className={cn("size-4", color, tone !== "success" && "[&_path]:stroke-current")} />
      {Indicator && <span className="absolute -top-1 -right-1 grid size-3 place-items-center rounded-full bg-background ring-1 ring-background">
        <Indicator strokeWidth={2.5} className={cn("size-2.5", tone === "busy" && !reduceMotion && "animate-spin motion-reduce:animate-none")} />
      </span>}
    </span>
  )
}

/**
 * Browser-preview host for the status panel (`?view=status-bar`, see UI-PATTERNS.md): a
 * menu-bar-style button and popover around `StatusBarContent` with the Radix computer menu.
 * The desktop app does not use it: its tray opens `desktop/status-panel.tsx`, which renders
 * the same content with the native computer menu built from the same items.
 */
export function StatusBar({ source, actions, defaultOpen = false }: { source: ApplicationSource; actions: StatusBarActions; defaultOpen?: boolean }) {
  const [open, setOpen] = useState(defaultOpen)
  const content = useRef<HTMLDivElement>(null)
  const health = statusBarHealth(source)
  function dismissThen(action: () => void) { setOpen(false); action() }
  const dismissingActions: StatusBarActions = {
    ...actions,
    openSilo: (route) => dismissThen(() => actions.openSilo(route)),
    quit: () => dismissThen(actions.quit),
    openTerminal: (name) => dismissThen(() => actions.openTerminal(name)),
    openEditor: (name, path) => dismissThen(() => actions.openEditor(name, path)),
    openSite: (name, port) => dismissThen(() => actions.openSite(name, port)),
  }
  return (
    <TooltipProvider delayDuration={150} reduceMotion={source.preferences.reduceMotion}>
      <Popover open={open} onOpenChange={setOpen}>
        <PopoverTrigger asChild>
          <Button variant="ghost" size="icon-sm" className="relative rounded-md" aria-label="Silo status bar" aria-description={health.label} title={`Silo · ${health.label}`}>
            <StatusBarIcon tone={health.tone} reduceMotion={source.preferences.reduceMotion} />
          </Button>
        </PopoverTrigger>
        <PopoverContent
          ref={content}
          aria-label="Silo"
          align="end"
          sideOffset={8}
          collisionPadding={10}
          className="silo-window flex max-h-[min(520px,var(--radix-popover-content-available-height))] w-[380px] max-w-[calc(100vw-20px)] flex-col overflow-hidden rounded-xl p-0 shadow-lg"
          data-reduce-motion={source.preferences.reduceMotion}
          onOpenAutoFocus={(event) => { event.preventDefault(); content.current?.focus() }}
        >
          <StatusBarContent source={source} actions={dismissingActions} focusContent={() => content.current?.focus()} />
        </PopoverContent>
      </Popover>
    </TooltipProvider>
  )
}
