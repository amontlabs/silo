import { useEffect, useEffectEvent, useRef, useState } from "react"
import { Command, defaultFilter } from "cmdk"
import { ArrowDown, ArrowUp, CornerDownLeft, Search } from "lucide-react"
import { Dialog } from "radix-ui"

import { ShortcutBadge } from "@/components/shortcut-badge"
import { shortcutFor } from "@/lib/shortcuts"
import { Button } from "@/components/ui/button"
import { restoreFocus } from "@/lib/focus"
import type { ApplicationCommand } from "./application-commands"

const groups = ["Go to", "Computers", "Actions"] as const

/** Items are keyed by command id (same-named computers stay distinct); the label is the first keyword. */
function filterItem(_value: string, search: string, keywords: string[] = []) {
  const [label = "", ...rest] = keywords
  return filterCommand(label, search, rest)
}

function filterCommand(label: string, search: string, keywords: string[] = []) {
  const text = [label, ...keywords].join(" ").toLowerCase()
  const terms = search.toLowerCase().trim().split(/\s+/)
  // Match whole search terms before ranking, so "dev" cannot span unrelated aliases.
  if (!terms.every((term) => text.includes(term))) return 0
  return defaultFilter(label, search, keywords) || 0.5
}

export function ApplicationCommandMenu({ commands, disabled = false, openRequest, nativeShortcuts = false }: { commands: readonly ApplicationCommand[]; disabled?: boolean; openRequest?: number; nativeShortcuts?: boolean }) {
  const [open, setOpenState] = useState(false)
  const previousFocus = useRef<HTMLElement | null>(null)
  // A command with a question asks it here, in place of the list, before it runs.
  const [confirmingId, setConfirming] = useState<string | null>(null)
  const confirming = commands.find(command => command.id === confirmingId)
  // A snapshot can remove the action or its need for confirmation while the question is open.
  if (confirmingId !== null && !confirming?.confirm) setConfirming(null)
  const setOpen = (next: boolean | ((current: boolean) => boolean)) => {
    if (next !== false && !contentRef.current) {
      previousFocus.current = document.activeElement instanceof HTMLElement && document.activeElement !== document.body ? document.activeElement : null
    }
    setOpenState(next)
    setConfirming(null)
  }
  const consumedOpenRequest = useRef(0)
  // A command that opens a popover or picker keeps focus there instead of on the palette's trigger.
  const keepFocus = useRef(false)
  const openRequested = useEffectEvent(() => {
    if (disabled) return
    const focusedDialog = document.activeElement?.closest('[role="dialog"], [role="alertdialog"]')
    if (focusedDialog && focusedDialog !== contentRef.current) return
    setOpen((current) => !current)
  })
  useEffect(() => {
    if (!openRequest || consumedOpenRequest.current === openRequest) return
    consumedOpenRequest.current = openRequest
    openRequested()
  }, [openRequest])
  const contentRef = useRef<HTMLDivElement>(null)
  const shortcut = shortcutFor("search")

  useEffect(() => {
    // oxlint-disable-next-line react/set-state-in-effect
    if (disabled) setOpen(false)
  }, [disabled])

  useEffect(() => {
    function toggleCommands(event: KeyboardEvent) {
      if (disabled || nativeShortcuts) return
      if (!(event.metaKey || event.ctrlKey) || event.key.toLowerCase() !== "k" || event.altKey || event.isComposing || event.repeat || event.defaultPrevented) return
      const focusedDialog = document.activeElement?.closest('[role="dialog"], [role="alertdialog"]')
      if (focusedDialog && focusedDialog !== contentRef.current) return
      event.preventDefault()
      setOpen((current) => !current)
    }
    window.addEventListener("keydown", toggleCommands)
    return () => window.removeEventListener("keydown", toggleCommands)
  }, [disabled, nativeShortcuts])

  return <Dialog.Root open={open && !disabled} onOpenChange={setOpen}>
    <Dialog.Trigger asChild>
      <button type="button" disabled={disabled} aria-label="Search or jump to" aria-keyshortcuts={shortcut?.aria} className="relative flex h-7 w-full max-w-md items-center gap-2 rounded-md border border-border bg-muted/30 px-2 text-xs text-muted-foreground outline-none hover:bg-muted/50 focus-visible:border-ring focus-visible:ring-2 focus-visible:ring-ring/30">
        <Search aria-hidden="true" className="size-3.5 shrink-0" />
        <span className="min-w-0 flex-1 truncate text-left">Search or jump to…</span>
        {shortcut && <ShortcutBadge shortcut={shortcut} />}
      </button>
    </Dialog.Trigger>
    <Dialog.Portal>
      <Dialog.Overlay className="fixed inset-0 z-50 bg-black/20" />
      <Dialog.Content ref={contentRef} aria-describedby={undefined} onCloseAutoFocus={(event) => {
        if (keepFocus.current) event.preventDefault()
        else if (previousFocus.current?.isConnected && !previousFocus.current.matches(":disabled")) {
          event.preventDefault()
          restoreFocus(previousFocus.current)
        }
        keepFocus.current = false
      }} onEscapeKeyDown={(event) => {
        // Escape leaves the question for the list first, then closes the palette.
        if (!confirming) return
        event.preventDefault()
        setConfirming(null)
      }} className="fixed top-[min(18dvh,8rem)] left-1/2 z-50 w-[calc(100%-2rem)] max-w-lg -translate-x-1/2 overflow-hidden rounded-xl border border-border bg-popover text-popover-foreground shadow-2xl outline-none">
        <Dialog.Title className="sr-only">Commands</Dialog.Title>
        {confirming?.confirm ? <CommandConfirmationPanel command={confirming} onCancel={() => setConfirming(null)} onConfirm={() => { if (disabled) return; setOpen(false); confirming.run() }} /> : <Command label="Search commands" filter={filterItem} loop vimBindings={false}>
          <div className="flex items-center gap-3 border-b border-border px-4">
            <Search aria-hidden="true" className="size-4 shrink-0 text-muted-foreground" />
            <Command.Input aria-label="Search commands" placeholder="Search pages, computers, and actions…" autoFocus autoComplete="off" spellCheck={false} className="h-12 min-w-0 flex-1 bg-transparent text-ui outline-none placeholder:text-muted-foreground" />
            <Dialog.Close aria-label="Close commands" className="rounded border border-border px-1.5 py-0.5 text-caption text-muted-foreground hover:bg-muted focus-ring">Esc</Dialog.Close>
          </div>
          <Command.List className="max-h-[min(22rem,50dvh)] overflow-y-auto overscroll-contain scroll-py-2 p-1.5" label="Commands">
            <Command.Empty className="px-4 py-10 text-center text-xs text-muted-foreground">No commands found.</Command.Empty>
            {groups.map((group) => <Command.Group key={group} value={group.replaceAll(" ", "-")} heading={group} className="[&_[cmdk-group-heading]]:px-2.5 [&_[cmdk-group-heading]]:pt-3 [&_[cmdk-group-heading]]:pb-1.5 [&_[cmdk-group-heading]]:text-caption [&_[cmdk-group-heading]]:font-medium [&_[cmdk-group-heading]]:text-muted-foreground">
              {commands.filter((command) => command.group === group).map((command) => <Command.Item key={command.id} value={command.id} keywords={[command.label, ...(command.keywords ?? [])]} onSelect={() => {
                if (disabled) return
                if (command.confirm) { setConfirming(command.id); return }
                keepFocus.current = Boolean(command.opensPanel)
                setOpen(false)
                command.run()
              }} className="flex cursor-pointer items-center gap-2.5 rounded-md px-2.5 py-2 text-xs outline-none select-none data-[selected=true]:bg-accent data-[selected=true]:text-accent-foreground">
                <command.icon aria-hidden="true" className="size-3.5 shrink-0 text-muted-foreground" />
                <span className="truncate">{command.label}</span>
              </Command.Item>)}
            </Command.Group>)}
          </Command.List>
          <div aria-hidden="true" className="flex items-center gap-4 border-t border-border bg-muted/30 px-4 py-2 text-caption text-muted-foreground">
            <span className="flex items-center gap-1"><ArrowUp className="size-3" /><ArrowDown className="size-3" /> Navigate</span>
            <span className="flex items-center gap-1"><CornerDownLeft className="size-3" /> Run command</span>
          </div>
        </Command>}
      </Dialog.Content>
    </Dialog.Portal>
  </Dialog.Root>
}

/** The palette's in-place question: the same words as the control's own confirmation. */
function CommandConfirmationPanel({ command, onCancel, onConfirm }: { command: ApplicationCommand; onCancel: () => void; onConfirm: () => void }) {
  const confirm = command.confirm!
  return <div role="group" aria-label={confirm.title} className="grid gap-2 p-4 text-xs">
    <p className="text-ui font-medium">{confirm.title}</p>
    <p className="text-muted-foreground">{confirm.description}</p>
    <div className="mt-1 flex justify-end gap-2">
      <Button type="button" variant="ghost" size="sm" onClick={onCancel}>Cancel</Button>
      <Button type="button" size="sm" variant={confirm.tone === "destructive" ? "destructive" : "default"} autoFocus onClick={onConfirm}>{confirm.confirmLabel}</Button>
    </div>
  </div>
}
